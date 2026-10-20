#!/usr/bin/env python3
"""Compare one offline-improvement candidate with its baseline on a case split.

Spec 17 offline improvement: training cases inform a candidate; held-out cases
run only after the manifest marks the candidate frozen, and their results never
revise it. Both arms share cases, model, runs, prompt bundle, and routes; only
the case templates (and the binary that embeds them) differ. The tool records
evidence for operator approval and never writes active configuration.
"""

from __future__ import annotations

import argparse
import os
import sys
import tempfile
from collections import Counter
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from tooling.live_provider.run_lifecycle_v2_eval import (
    HOSTED_ENABLE_ENV,
    checked_binding,
    execute_run,
    write_json_atomic,
)
from tooling.live_provider.lifecycle_eval_gate import exact_metric_failures
from tooling.live_provider.lifecycle_eval_support import EvalError, artifact_for, load_json


ARMS = ("baseline", "candidate")
SPLITS = ("training", "held_out")
SAFETY_COUNTS = ("forbidden_outputs", "unsafe_recommendations", "raw_trace_or_secret_leaks")


def load_manifest(path: Path) -> dict[str, Any]:
    manifest = load_json(path)
    if manifest.get("schema_version") != 1:
        raise EvalError("improvement manifest must be schema v1")
    return manifest


def split_cases(manifest: dict[str, Any], split: str) -> list[dict[str, Any]]:
    if split == "held_out":
        if manifest["candidate"].get("frozen") is not True:
            raise EvalError("held-out cases run only after the candidate is frozen")
        return list(manifest["held_out_cases"])
    corpus = load_json(checked_binding(manifest["release_corpus"], "release corpus"))
    by_id = {case["id"]: case for case in corpus["cases"]}
    missing = [case_id for case_id in manifest["training_case_ids"] if case_id not in by_id]
    if missing:
        raise EvalError(f"training cases absent from the release corpus: {missing}")
    return [by_id[case_id] for case_id in manifest["training_case_ids"]]


GATE_ROUTES = "gate"


def route_binding(manifest: dict[str, Any], name: str) -> dict[str, Any]:
    """The gate routes decide the candidate; named check routes only record regressions."""
    if name == GATE_ROUTES:
        return manifest["routes"]
    checks = manifest.get("check_routes") or {}
    if name not in checks:
        raise EvalError(f"unknown route set {name!r}; expected {GATE_ROUTES!r} or one of {sorted(checks)}")
    return checks[name]


def arm_bindings(manifest: dict[str, Any], arm: str, routes: str) -> dict[str, Path]:
    return {
        "case_templates": checked_binding(manifest[arm]["case_templates"], f"{arm} case templates"),
        "prompt_bundle": checked_binding(manifest["prompt_bundle"], "prompt bundle"),
        "routes": checked_binding(route_binding(manifest, routes), f"{routes} routes"),
    }


SHARED_ARM_IDENTITY = ("arm", "split", "case_templates_digest", "routes_digest")


def seeded_runs(
    source: dict[str, Any], identity: dict[str, Any], case_ids: set[str], slot: str
) -> list[dict[str, Any]]:
    """Reuse another improvement's scored runs of the same arm as this comparison's control.

    Allowed only when arm, split, template digest, and route digest match, so the
    runs come from identical requests; each run's answer is re-read for this slot.
    """
    mismatched = [key for key in SHARED_ARM_IDENTITY if source.get(key) != identity[key]]
    if mismatched:
        raise EvalError(f"seed report does not share this arm's identity: {mismatched}")
    runs = []
    for run in source.get("runs", []):
        if run["case_id"] in case_ids and scorable(run):
            runs.append({**run, "slot_answer": slot_answer(run, slot), "seeded_from": source["improvement_id"]})
    return runs


def resumable_runs(previous: dict[str, Any], identity: dict[str, Any]) -> list[dict[str, Any]]:
    """Reuse scored runs only from a report with the same arm, split, templates, and routes."""
    if not previous:
        return []
    mismatched = [key for key, value in identity.items() if previous.get(key) != value]
    if mismatched:
        raise EvalError(f"existing report does not match this comparison: {mismatched}")
    return [run for run in previous.get("runs", []) if scorable(run)]


def slot_answer(run: dict[str, Any], slot: str) -> str | None:
    _, artifact = artifact_for(Path(run["artifacts"]["reviewed_report"]))
    definitions = {
        definition["probe_id"]: definition["slot"]
        for definition in artifact.get("probe_definitions") or []
    }
    answers = [
        result.get("answer")
        for result in artifact.get("probe_results") or []
        if definitions.get(result.get("probe_id")) == slot
    ]
    return answers[0] if answers else None


def scorable(run: dict[str, Any]) -> bool:
    """A run is scored only when the route qualified and execution reached closure.

    Provider failures, budget refusals, and qualification failures leave the
    host's fallback `unknown` answers, which measure the route rather than the
    candidate, so those runs are rerun instead of scored.
    """
    metrics = run.get("metrics") or {}
    return "error" not in run and metrics.get("qualified") is True and metrics.get(
        "execution_succeeded"
    ) is True


def summarize(
    cases: list[dict[str, Any]], runs: list[dict[str, Any]], slot: str
) -> dict[str, Any]:
    summary = {}
    for case in cases:
        case_runs = [run for run in runs if run["case_id"] == case["id"]]
        scored = [run for run in case_runs if scorable(run)]
        allowed = case["expectations"]["probe_answers"].get(slot, [])
        answers = [run.get("slot_answer") for run in scored]
        summary[case["id"]] = {
            "scored_runs": len(scored),
            "unscored_runs": len(case_runs) - len(scored),
            "exact_passes": sum(not exact_metric_failures([run]) for run in scored),
            "slot_allowed": allowed,
            "slot_answers": dict(Counter(str(answer) for answer in answers)),
            "slot_hits": sum(answer in allowed for answer in answers),
            **{
                count: sum(run["metrics"].get(count, 0) for run in scored)
                for count in SAFETY_COUNTS
            },
            "finding_mutations": sum(
                not run["metrics"].get("finding_immutability_valid", False) for run in scored
            ),
        }
    return summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--arm", choices=ARMS, required=True)
    parser.add_argument("--split", choices=SPLITS, required=True)
    parser.add_argument("--routes", default=GATE_ROUTES)
    parser.add_argument("--seed-from", type=Path, help="reuse matching scored runs of the same arm")
    parser.add_argument("--auditor", type=Path, required=True)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--workspace", type=Path)
    parser.add_argument("--timeout-seconds", type=int, default=900)
    return parser.parse_args()


def run_comparison(args: argparse.Namespace) -> dict[str, Any]:
    manifest = load_manifest(args.manifest)
    slot = manifest["owned_asset"]["probe_slot"]
    cases = split_cases(manifest, args.split)
    bindings = arm_bindings(manifest, args.arm, args.routes)
    identity = {
        "improvement_id": manifest["improvement_id"],
        "arm": args.arm,
        "split": args.split,
        "case_templates_digest": manifest[args.arm]["case_templates"]["digest"],
        "routes_digest": route_binding(manifest, args.routes)["digest"],
    }
    previous = load_json(args.output) if args.output.is_file() else {}
    runs = resumable_runs(previous, identity)
    if args.seed_from:
        reused = {(run["case_id"], run["run_index"]) for run in runs}
        runs += [
            run
            for run in seeded_runs(load_json(args.seed_from), identity, {case["id"] for case in cases}, slot)
            if (run["case_id"], run["run_index"]) not in reused
        ]
    done = {(run["case_id"], run["run_index"]) for run in runs}
    workspace = args.workspace or Path(tempfile.mkdtemp(prefix="ea-prompt-candidate-"))
    run_args = argparse.Namespace(
        auditor=args.auditor,
        runner=args.runner,
        route_set="keyed_free",
        timeout_seconds=args.timeout_seconds,
        no_review_timeout=False,
    )
    report = {"schema_version": 1, **identity, "routes": args.routes, "runs": runs}
    for case in cases:
        for run_index in range(manifest["runs_per_case"]):
            if (case["id"], run_index) in done:
                continue
            try:
                run = execute_run(run_args, case, run_index, bindings, workspace)
                run["slot_answer"] = slot_answer(run, slot)
            except EvalError as error:
                run = {"case_id": case["id"], "run_index": run_index, "error": str(error)}
            runs.append(run)
            report["summary"] = summarize(cases, runs, slot)
            write_json_atomic(args.output, report)
            print(f"{case['id']} run {run_index + 1}: {run.get('slot_answer', run.get('error'))}")
    report["summary"] = summarize(cases, runs, slot)
    write_json_atomic(args.output, report)
    return report


def main() -> int:
    args = parse_args()
    if os.environ.get(HOSTED_ENABLE_ENV) != "1":
        print(f"set {HOSTED_ENABLE_ENV}=1 to authorize the live keyed hosted comparison", file=sys.stderr)
        return 2
    try:
        run_comparison(args)
    except EvalError as error:
        print(f"prompt candidate comparison failed: {error}", file=sys.stderr)
        return 1
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

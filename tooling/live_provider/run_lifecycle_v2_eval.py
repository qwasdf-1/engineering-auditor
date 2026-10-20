#!/usr/bin/env python3
"""Run the public Layer 6 lifecycle v2 release corpus against a live route."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from tooling.live_provider.lifecycle_eval_gate import (
    aggregate,
    aggregate_metric_vector,
    exact_metric_failures,
    release_run_counts,
    select_cases,
)
from tooling.live_provider.lifecycle_eval_metrics import (
    citation_valid,
    conclusion_signature,
    evaluate_run,
    execution_succeeded,
    lifecycle_action_complete,
    numeric_benefit_challenged,
    provider_record_leak_count,
    qualified,
    unsupported_numeric_benefit_removed,
)
from tooling.live_provider.lifecycle_eval_resume import (
    CheckpointContext,
    checkpoint_report,
    pending_run_indexes,
    validate_resumed_runs,
)
from tooling.live_provider.lifecycle_eval_support import (
    CheckedCommand,
    EvalError,
    artifact_for,
    load_json,
    run_checked,
    sha256_file,
)


DEFAULT_CORPUS = REPO_ROOT / "config/auditor/review-eval-corpus-v2.json"
DEFAULT_AUDITOR = REPO_ROOT / "target/release/auditor"
DEFAULT_RUNNER = REPO_ROOT / "target/release/examples/review_v2_case"
EVALUATOR_PATH = Path(__file__).resolve()
EVALUATOR_SOURCES = (
    EVALUATOR_PATH,
    EVALUATOR_PATH.with_name("lifecycle_eval_gate.py"),
    EVALUATOR_PATH.with_name("lifecycle_eval_metrics.py"),
    EVALUATOR_PATH.with_name("lifecycle_eval_resume.py"),
    EVALUATOR_PATH.with_name("lifecycle_eval_support.py"),
)
EVALUATOR_CONTRACT_VERSION = 7
ENABLE_ENV = "AUDITOR_LIVE_ANONYMOUS_EVAL"
# Keyed zero-cost routes (`--route-set` other than `anonymous`) send the
# generated cases to a hosted provider under the operator's key.
HOSTED_ENABLE_ENV = "AUDITOR_LIVE_HOSTED_EVAL"
ANONYMOUS_ROUTE_SET = "anonymous"


def evaluator_identity() -> dict[str, Any]:
    return {
        "contract_version": EVALUATOR_CONTRACT_VERSION,
        "path": str(EVALUATOR_PATH),
        "source_paths": [str(path) for path in EVALUATOR_SOURCES],
        "digest": evaluator_source_digest(),
    }


def evaluator_source_digest() -> str:
    digest = hashlib.sha256()
    for path in EVALUATOR_SOURCES:
        digest.update(path.name.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return "sha256:" + digest.hexdigest()


def source_tree_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(path for path in root.rglob("*") if path.is_file()):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return "sha256:" + digest.hexdigest()


def write_json_atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
            temporary = Path(stream.name)
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def resolve_binding(corpus: dict[str, Any], name: str) -> Path:
    return checked_binding(corpus["bindings"][name], name)


def resolve_routes(corpus: dict[str, Any], route_set: str) -> Path:
    """The anonymous `routes` binding, or a named keyed zero-cost route."""
    if route_set == ANONYMOUS_ROUTE_SET:
        return resolve_binding(corpus, "routes")
    binding = corpus["bindings"].get("keyed_free_routes", {}).get(route_set)
    if binding is None:
        raise EvalError(f"unknown route set {route_set!r}")
    return checked_binding(binding, f"{route_set} routes")


def checked_binding(binding: dict[str, Any], name: str) -> Path:
    path = (REPO_ROOT / binding["path"]).resolve()
    if not path.is_file() or sha256_file(path) != binding["digest"]:
        raise EvalError(f"{name} binding is absent or stale: {path}")
    return path


def consent_flags(route_set: str) -> list[str]:
    flags = ["--allow-hosted-review"]
    if route_set == ANONYMOUS_ROUTE_SET:
        flags.append("--allow-anonymous-provider")
    return flags


def ensure_binaries(args: argparse.Namespace, output_dir: Path) -> None:
    if args.skip_build and args.auditor.is_file() and args.runner.is_file():
        return
    build_plan = (
        ("auditor-cli", "--bin", "auditor", "auditor"),
        ("auditor-review", "--example", "review_v2_case", "review-v2-case"),
    )
    for package, target_flag, target, log_name in build_plan:
        run_checked(
            CheckedCommand(
                [
                    "cargo",
                    "build",
                    "--release",
                    "-p",
                    package,
                    target_flag,
                    target,
                ],
                cwd=REPO_ROOT,
                stdout_path=output_dir / f"build-{log_name}.stdout.txt",
                stderr_path=output_dir / f"build-{log_name}.stderr.txt",
                timeout=args.timeout_seconds,
            )
        )
    if not args.auditor.is_file() or not args.runner.is_file():
        raise EvalError("release build did not produce the auditor and v2 evaluation runner")


def generate_case(case: dict[str, Any], case_repo: Path, run_dir: Path) -> None:
    generator = (REPO_ROOT / case["generator"]).resolve()
    if not generator.is_file() or sha256_file(generator) != case["generator_digest"]:
        raise EvalError(f"generator is absent or stale: {generator}")
    run_checked(
        CheckedCommand(
            [str(generator), str(case_repo), case["fixture_kind"]],
            cwd=REPO_ROOT,
            stdout_path=run_dir / "generate.stdout.txt",
            stderr_path=run_dir / "generate.stderr.txt",
            timeout=60,
        )
    )
    actual = source_tree_digest(case_repo)
    if actual != case["source_digest"]:
        raise EvalError(
            f"generated source digest mismatch for {case['id']}: {actual}"
        )


def execute_run(
    args: argparse.Namespace,
    case: dict[str, Any],
    run_index: int,
    bindings: dict[str, Path],
    workspace: Path,
) -> dict[str, Any]:
    run_dir = workspace / case["id"] / f"run-{run_index + 1}"
    prepare_run_directory(run_dir)
    case_repo = run_dir / "repo"
    generate_case(case, case_repo, run_dir)
    input_report = run_dir / "input-report.json"
    run_checked(
        CheckedCommand(
            [
                str(args.auditor),
                "scan",
                "--repo-root",
                str(case_repo),
                "--profile",
                "full",
                "--format",
                "json",
                "--output",
                str(input_report),
            ],
            cwd=REPO_ROOT,
            stdout_path=run_dir / "scan.stdout.json",
            stderr_path=run_dir / "scan.stderr.txt",
            timeout=args.timeout_seconds,
        )
    )
    reviewed_report = run_dir / "reviewed-report.json"
    run_checked(
        CheckedCommand(
            [
                str(args.runner),
                "--report",
                str(input_report),
                "--repo-root",
                str(case_repo),
                "--output",
                str(reviewed_report),
                "--case-templates",
                str(bindings["case_templates"]),
                "--routes",
                str(bindings["routes"]),
                "--prompt-bundle",
                str(bindings["prompt_bundle"]),
                *consent_flags(args.route_set),
            ],
            cwd=REPO_ROOT,
            stdout_path=run_dir / "review.stdout.json",
            stderr_path=run_dir / "review.stderr.txt",
            timeout=None if args.no_review_timeout else args.timeout_seconds,
            env_overrides={"AUDITOR_REVIEW_TRANSCRIPT": "1"},
        )
    )
    transcript = run_dir / "reviewed-report.transcript.json"
    if not transcript.is_file():
        raise EvalError("review run did not publish its required transcript sidecar")
    transcript_markdown = run_dir / "reviewed-report.transcript.md"
    run_checked(
        CheckedCommand(
            [
                str(args.auditor),
                "review",
                "transcript",
                "--input",
                str(transcript),
                "--output",
                str(transcript_markdown),
            ],
            cwd=REPO_ROOT,
            stdout_path=run_dir / "transcript-render.stdout.txt",
            stderr_path=run_dir / "transcript-render.stderr.txt",
            timeout=60,
        )
    )
    command_result = load_json(run_dir / "review.stdout.json")
    reviewed, artifact = artifact_for(reviewed_report)
    metrics = evaluate_run(
        case,
        load_json(input_report),
        reviewed,
        artifact,
        command_result["verify"],
    )
    return {
        "case_id": case["id"],
        "run_index": run_index,
        "review_id": artifact.get("review_id"),
        "metrics": metrics,
        "artifacts": {
            "input_report": str(input_report),
            "reviewed_report": str(reviewed_report),
            "review_stdout": str(run_dir / "review.stdout.json"),
            "transcript": str(transcript),
            "transcript_markdown": str(transcript_markdown),
            "review_artifact": str(
                reviewed_report.parent / reviewed["engineering_review"]["path"]
            ),
        },
        "artifact_digests": {
            "input_report": sha256_file(input_report),
            "reviewed_report": sha256_file(reviewed_report),
            "review_stdout": sha256_file(run_dir / "review.stdout.json"),
            "transcript": sha256_file(transcript),
            "transcript_markdown": sha256_file(transcript_markdown),
            "review_artifact": sha256_file(
                reviewed_report.parent / reviewed["engineering_review"]["path"]
            ),
        },
    }


def prepare_run_directory(run_dir: Path) -> None:
    if run_dir.exists():
        suffix = 1
        while True:
            quarantine = run_dir.with_name(f"{run_dir.name}.incomplete-{suffix}")
            if not quarantine.exists():
                run_dir.rename(quarantine)
                break
            suffix += 1
    run_dir.mkdir(parents=True)


def evaluation_identity(
    args: argparse.Namespace,
    corpus: dict[str, Any],
    bindings: dict[str, Path],
) -> dict[str, Any]:
    return {
        "corpus_id": corpus["corpus_id"],
        "corpus_digest": sha256_file(args.corpus),
        "evaluator": evaluator_identity(),
        "bindings": {
            name: {"path": str(path), "digest": sha256_file(path)}
            for name, path in bindings.items()
        },
        "binaries": {
            "auditor": {
                "path": str(args.auditor.resolve()),
                "digest": sha256_file(args.auditor),
            },
            "review_v2_case": {
                "path": str(args.runner.resolve()),
                "digest": sha256_file(args.runner),
            },
        },
        "live_anonymous": args.route_set == ANONYMOUS_ROUTE_SET,
        "review_process_timeout_seconds": (
            None if args.no_review_timeout else args.timeout_seconds
        ),
    }


def validate_live_identity(
    expected: dict[str, Any],
    args: argparse.Namespace,
    corpus: dict[str, Any],
    bindings: dict[str, Path],
) -> None:
    """Fail if an executable or bound input changes during a live run."""
    current = evaluation_identity(args, corpus, bindings)
    changed = [name for name in expected if current.get(name) != expected[name]]
    if changed:
        raise EvalError(
            "evaluation identity changed during execution: " + ", ".join(changed)
        )


def validate_plan_identity(plan: "EvaluationPlan") -> None:
    validate_live_identity(
        plan.checkpoint.identity,
        plan.args,
        plan.corpus,
        plan.bindings,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--auditor", type=Path, default=DEFAULT_AUDITOR)
    parser.add_argument("--runner", type=Path, default=DEFAULT_RUNNER)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--workspace", type=Path)
    add_resume_arguments(parser)
    parser.add_argument("--runs-per-case", type=int)
    parser.add_argument("--case", action="append", default=[])
    parser.add_argument("--timeout-seconds", type=int, default=900)
    parser.add_argument(
        "--route-set",
        default=ANONYMOUS_ROUTE_SET,
        help="`anonymous` (default) or a corpus keyed_free_routes name, such as mistral_free",
    )
    add_execution_arguments(parser)
    return parser.parse_args()


def add_resume_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--resume-from",
        type=Path,
        help=(
            "reuse prior digest-matched runs and execute only missing case/run "
            "indexes"
        ),
    )
    parser.add_argument(
        "--allow-evaluator-change-on-resume",
        action="store_true",
        help=(
            "permit only the evaluator identity to differ when resuming; all "
            "retained artifacts and metrics are still revalidated under the current evaluator"
        ),
    )
    parser.add_argument(
        "--max-new-runs",
        type=int,
        help=(
            "checkpoint and exit successfully after this many newly completed "
            "runs; resume the digest-bound report to continue"
        ),
    )


def add_execution_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--no-review-timeout",
        action="store_true",
        help=(
            "disable only the outer review-process timeout; provider calls still "
            "obey the route timeout and bounded retry policy"
        ),
    )
    parser.add_argument("--keep-workspace", action="store_true")
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="reuse explicit existing binaries instead of rebuilding the checked-out source",
    )


@dataclass
class EvaluationPlan:
    args: argparse.Namespace
    corpus: dict[str, Any]
    bindings: dict[str, Path]
    cases: list[dict[str, Any]]
    requested_run_counts: dict[str, int]
    workspace: Path
    runs: list[dict[str, Any]]
    checkpoint: CheckpointContext


@dataclass
class EvaluationState:
    report: dict[str, Any] | None = None


@dataclass(frozen=True)
class EvaluationResult:
    complete: bool
    failures: list[str]
    remaining_run_count: int = 0


def bound_pending_runs(
    pending: list[tuple[dict[str, Any], int]], max_new_runs: int | None
) -> list[tuple[dict[str, Any], int]]:
    return pending if max_new_runs is None else pending[:max_new_runs]


def prepare_evaluation(
    args: argparse.Namespace, output_dir: Path, workspace: Path
) -> EvaluationPlan:
    if args.allow_evaluator_change_on_resume and not args.resume_from:
        raise EvalError("--allow-evaluator-change-on-resume requires --resume-from")
    corpus = load_json(args.corpus)
    if corpus.get("schema_version") != 2 or len(corpus.get("cases") or []) != 8:
        raise EvalError("lifecycle corpus must be schema v2 with exactly eight cases")
    bindings = {
        name: resolve_binding(corpus, name) for name in ("case_templates", "prompt_bundle")
    }
    bindings["routes"] = resolve_routes(corpus, args.route_set)
    cases = select_cases(corpus, args.case)
    requested_run_counts = release_run_counts(corpus, cases, args.runs_per_case)
    if args.timeout_seconds < 1:
        raise EvalError("--timeout-seconds must be positive")
    if args.max_new_runs is not None and args.max_new_runs < 1:
        raise EvalError("--max-new-runs must be positive")
    ensure_binaries(args, output_dir)
    identity = evaluation_identity(args, corpus, bindings)
    resumed_runs = (
        validate_resumed_runs(
            args.resume_from,
            identity,
            cases,
            allow_evaluator_change=args.allow_evaluator_change_on_resume,
        )
        if args.resume_from
        else []
    )
    started = time.time()
    return EvaluationPlan(
        args=args,
        corpus=corpus,
        bindings=bindings,
        cases=cases,
        requested_run_counts=requested_run_counts,
        workspace=workspace,
        runs=list(resumed_runs),
        checkpoint=CheckpointContext(
            identity=identity,
            requested_run_counts=requested_run_counts,
            selected_case_ids=[case["id"] for case in cases],
            resume_from=args.resume_from,
            reused_run_count=len(resumed_runs),
            started=started,
        ),
    )


def execute_evaluation(plan: EvaluationPlan, state: EvaluationState) -> EvaluationResult:
    state.report = checkpoint_report(plan.checkpoint, plan.runs)
    write_json_atomic(plan.args.output, state.report)
    pending = pending_run_indexes(
        plan.cases,
        plan.runs,
        plan.requested_run_counts,
    )
    for case, run_index in bound_pending_runs(pending, plan.args.max_new_runs):
        completed_run = execute_run(
            plan.args,
            case,
            run_index,
            plan.bindings,
            plan.workspace,
        )
        validate_plan_identity(plan)
        plan.runs.append(completed_run)
        state.report = checkpoint_report(plan.checkpoint, plan.runs)
        write_json_atomic(plan.args.output, state.report)
        per_run_failures = exact_metric_failures([completed_run])
        if per_run_failures:
            remaining = pending_run_indexes(
                plan.cases,
                plan.runs,
                plan.requested_run_counts,
            )
            state.report.update(
                {
                    "status": "failed",
                    "checkpoint_reason": "exact_metric_failure",
                    "remaining_run_count": len(remaining),
                    "hard_gate_passed": False,
                    "hard_gate_failures": per_run_failures,
                }
            )
            write_json_atomic(plan.args.output, state.report)
            return EvaluationResult(complete=True, failures=per_run_failures)
    remaining = pending_run_indexes(
        plan.cases,
        plan.runs,
        plan.requested_run_counts,
    )
    if remaining:
        state.report.update(
            {
                "checkpoint_reason": "new_run_limit_reached",
                "remaining_run_count": len(remaining),
            }
        )
        write_json_atomic(plan.args.output, state.report)
        return EvaluationResult(
            complete=False,
            failures=[],
            remaining_run_count=len(remaining),
        )
    validate_plan_identity(plan)
    plan.runs.sort(key=lambda run: (run["case_id"], run["run_index"]))
    aggregate_metrics, failures = aggregate(plan.corpus, plan.cases, plan.runs)
    state.report.update(
        {
            "status": "complete",
            "completed_run_count": len(plan.runs),
            "elapsed_seconds": round(time.time() - plan.checkpoint.started, 3),
            "runs": plan.runs,
            "aggregate": aggregate_metrics,
            "hard_gate_passed": not failures,
            "hard_gate_failures": failures,
        }
    )
    write_json_atomic(plan.args.output, state.report)
    return EvaluationResult(complete=True, failures=failures)


def failed_report(
    args: argparse.Namespace,
    current: dict[str, Any] | None,
    error: EvalError,
) -> dict[str, Any]:
    report = current or {
        "schema_version": 2,
        "completed_run_count": 0,
        "runs": [],
        "review_process_timeout_seconds": (
            None if args.no_review_timeout else args.timeout_seconds
        ),
    }
    report.update(
        {
            "status": "failed",
            "hard_gate_passed": False,
            "hard_gate_failures": [str(error)],
        }
    )
    return report


def run_evaluation_cli(args: argparse.Namespace) -> int:
    output_dir = args.output.resolve().parent
    output_dir.mkdir(parents=True, exist_ok=True)
    workspace = (
        args.workspace.resolve()
        if args.workspace
        else Path(tempfile.mkdtemp(prefix="ea-lifecycle-v2-eval-"))
    )
    workspace.mkdir(parents=True, exist_ok=True)
    state = EvaluationState()
    try:
        plan = prepare_evaluation(args, output_dir, workspace)
        result = execute_evaluation(plan, state)
        print(f"wrote {args.output}")
        if not result.complete:
            print(
                "checkpoint complete; "
                f"{result.remaining_run_count} planned runs remain"
            )
            return 0
        print(f"hard gate passed: {not result.failures}")
        for failure in result.failures:
            print(f"  - {failure}")
        return 0 if not result.failures else 1
    except EvalError as exc:
        state.report = failed_report(args, state.report, exc)
        write_json_atomic(args.output, state.report)
        print(f"lifecycle v2 evaluation failed: {exc}", file=sys.stderr)
        return 1
    finally:
        if not args.keep_workspace and not args.workspace:
            shutil.rmtree(workspace, ignore_errors=True)


def main() -> int:
    args = parse_args()
    anonymous = args.route_set == ANONYMOUS_ROUTE_SET
    enable = ENABLE_ENV if anonymous else HOSTED_ENABLE_ENV
    if os.environ.get(enable) != "1":
        kind = "anonymous" if anonymous else "keyed hosted"
        print(f"set {enable}=1 to authorize the live {kind} evaluation", file=sys.stderr)
        return 2
    return run_evaluation_cli(args)


if __name__ == "__main__":
    raise SystemExit(main())

"""Digest-bound resume validation and checkpoint rendering."""

from __future__ import annotations

import collections
import time
from dataclasses import dataclass
from pathlib import Path

from tooling.live_provider.lifecycle_eval_metrics import evaluate_run
from tooling.live_provider.lifecycle_eval_support import (
    EvalError,
    artifact_for,
    load_json,
    sha256_file,
)

JsonObject = dict[str, object]
JsonObjects = list[JsonObject]

IDENTITY_FIELDS = (
    "corpus_id",
    "corpus_digest",
    "evaluator",
    "bindings",
    "binaries",
    "live_anonymous",
    "review_process_timeout_seconds",
)

ARTIFACT_NAMES = (
    "input_report",
    "reviewed_report",
    "review_stdout",
    "transcript",
    "transcript_markdown",
    "review_artifact",
)


@dataclass(frozen=True)
class CheckpointContext:
    identity: JsonObject
    requested_run_counts: dict[str, int]
    selected_case_ids: list[str]
    resume_from: Path | None
    reused_run_count: int
    started: float


def validate_resumed_runs(
    resume_path: Path,
    expected_identity: JsonObject,
    cases: JsonObjects,
    *,
    allow_evaluator_change: bool = False,
) -> JsonObjects:
    report = load_json(resume_path)
    _validate_identity(
        report,
        expected_identity,
        allow_evaluator_change=allow_evaluator_change,
    )
    runs = report.get("runs")
    if not isinstance(runs, list):
        raise EvalError("resume report has no runs array")
    cases_by_id = {case["id"]: case for case in cases}
    seen: set[tuple[str, int]] = set()
    indexes_by_case: dict[str, list[int]] = collections.defaultdict(list)
    for run in runs:
        case_id, run_index = _validate_run_scope(run, cases_by_id, seen)
        seen.add((case_id, run_index))
        indexes_by_case[case_id].append(run_index)
        _validate_retained_run(run, cases_by_id[case_id])
    _validate_contiguous_indexes(indexes_by_case)
    return runs


def _validate_identity(
    report: JsonObject,
    expected: JsonObject,
    *,
    allow_evaluator_change: bool,
) -> None:
    if report.get("schema_version") != 2:
        raise EvalError("resume report is not lifecycle evaluation schema v2")
    mismatch = next(
        (
            field
            for field in IDENTITY_FIELDS
            if not (allow_evaluator_change and field == "evaluator")
            and report.get(field) != expected[field]
        ),
        None,
    )
    if mismatch:
        raise EvalError(f"resume report {mismatch} does not match this evaluation")


def _validate_run_scope(
    run: JsonObject,
    cases_by_id: dict[object, JsonObject],
    seen: set[tuple[str, int]],
) -> tuple[str, int]:
    case_id = run.get("case_id")
    run_index = run.get("run_index")
    in_scope = isinstance(case_id, str) and case_id in cases_by_id
    valid_index = isinstance(run_index, int) and run_index >= 0
    if not in_scope or not valid_index:
        raise EvalError("resume report contains an out-of-scope run")
    key = (case_id, run_index)
    if key in seen:
        raise EvalError("resume report contains a duplicate case/run index")
    return key


def _validate_retained_run(run: JsonObject, case: JsonObject) -> None:
    artifacts = run.get("artifacts") or {}
    digests = run.get("artifact_digests") or {}
    for name in ARTIFACT_NAMES:
        path_value = artifacts.get(name)
        if not isinstance(path_value, str):
            raise EvalError(f"resume run omits the {name} artifact path")
        path = Path(path_value)
        if not path.is_file() or digests.get(name) != sha256_file(path):
            raise EvalError(f"resume run has an absent or stale {name} artifact")
    input_report = load_json(Path(artifacts["input_report"]))
    reviewed, artifact = artifact_for(Path(artifacts["reviewed_report"]))
    command_result = load_json(Path(artifacts["review_stdout"]))
    metrics = evaluate_run(
        case,
        input_report,
        reviewed,
        artifact,
        command_result["verify"],
    )
    if run.get("review_id") != artifact.get("review_id"):
        raise EvalError("resume run review identity does not match its artifact")
    if run.get("metrics") != metrics:
        raise EvalError("resume run metrics do not match its retained artifacts")


def _validate_contiguous_indexes(indexes_by_case: dict[str, list[int]]) -> None:
    if any(
        sorted(indexes) != list(range(len(indexes)))
        for indexes in indexes_by_case.values()
    ):
        raise EvalError("resume run indexes must be contiguous from zero")


def pending_run_indexes(
    cases: JsonObjects,
    runs: JsonObjects,
    required_counts: dict[str, int],
) -> list[tuple[JsonObject, int]]:
    counts = collections.Counter(run["case_id"] for run in runs)
    if any(counts[case["id"]] > required_counts[case["id"]] for case in cases):
        raise EvalError("resume report already exceeds the requested run plan")
    return [
        (case, run_index)
        for case in cases
        for run_index in range(counts[case["id"]], required_counts[case["id"]])
    ]


def checkpoint_report(
    context: CheckpointContext, runs: JsonObjects
) -> JsonObject:
    ordered_runs = sorted(runs, key=lambda run: (run["case_id"], run["run_index"]))
    return {
        "schema_version": 2,
        **context.identity,
        "status": "in_progress",
        "requested_run_counts": context.requested_run_counts,
        "selected_case_ids": context.selected_case_ids,
        "resumed_from": str(context.resume_from.resolve()) if context.resume_from else None,
        "reused_run_count": context.reused_run_count,
        "completed_run_count": len(ordered_runs),
        "elapsed_seconds": round(time.time() - context.started, 3),
        "runs": ordered_runs,
        "aggregate": None,
        "hard_gate_passed": None,
        "hard_gate_failures": [],
    }

"""Aggregate release-gate policy for lifecycle evaluation runs."""

from __future__ import annotations

import collections
from tooling.live_provider.lifecycle_eval_support import EvalError

JsonObject = dict[str, object]
JsonObjects = list[JsonObject]


MECHANICAL_CHECKS = {
    "qualified": "one or more live runs were not fully qualified",
    "schema_valid": "schema validity was below 100 percent",
    "binding_valid": "binding validity was below 100 percent",
    "citation_valid": "citation validity was below 100 percent",
    "finding_immutability_valid": "finding immutability was below 100 percent",
    "candidate_count_valid": "one or more generated cases had unexpected candidates",
}

SEMANTIC_CHECKS = {
    "closure_valid": "one or more cases violated its exact allowed closure",
    "case_conclusion_valid": "one or more reviewers selected a disallowed typed case conclusion",
    "probe_answers_valid": "one or more admitted probe answers missed the exact corpus boundary",
    "required_recommendations_valid": "one or more required recommendation families were absent",
    "abstention_correct": "unknown/abstention behavior was incorrect",
    "unsupported_numeric_benefit_removed": "an unsupported numeric benefit survived finalization",
    "lifecycle_action_records_complete": "an accepted lifecycle action lacked required records",
}


def aggregate(
    corpus: JsonObject, cases: JsonObjects, runs: JsonObjects
) -> tuple[JsonObject, list[str]]:
    metrics = aggregate_metric_vector(cases, runs)
    failures = exact_metric_failures(runs)
    failures.extend(threshold_failures(corpus, cases, runs, metrics))
    return metrics, failures


def aggregate_metric_vector(
    cases: JsonObjects, runs: JsonObjects
) -> JsonObject:
    metrics = [run["metrics"] for run in runs]
    completed_runs = [run for run in runs if _completed(run)]
    completed_metrics = [run["metrics"] for run in completed_runs]
    required_total = sum(metric["required_total"] for metric in completed_metrics)
    required_hits = sum(metric["required_hits"] for metric in completed_metrics)
    abstention = _abstention_metrics(cases, completed_runs)
    flips = sum(_case_flip_count(case["id"], completed_runs) for case in cases)
    return {
        **_aggregate_counts(runs, metrics, completed_runs, completed_metrics),
        **_aggregate_rates(required_hits, required_total, abstention, flips, completed_runs),
    }


def _aggregate_counts(
    runs: JsonObjects,
    metrics: JsonObjects,
    completed_runs: JsonObjects,
    completed_metrics: JsonObjects,
) -> JsonObject:
    return {
        "run_count": len(runs),
        "qualified_run_count": sum(metric["qualified"] for metric in metrics),
        "completed_run_count": len(completed_runs),
        "useful_novel_positive_count": sum(
            metric["useful_novel_action"] for metric in completed_metrics
        ),
    }


def _aggregate_rates(
    required_hits: int,
    required_total: int,
    abstention: list[bool],
    flips: int,
    completed_runs: JsonObjects,
) -> dict[str, float]:
    return {
        "required_outcome_recall": _ratio(required_hits, required_total, 1.0),
        "unknown_abstention_accuracy": _ratio(sum(abstention), len(abstention), 1.0),
        "material_conclusion_flip_rate": _ratio(flips, len(completed_runs), 0.0),
    }


def _completed(run: JsonObject) -> bool:
    metrics = run["metrics"]
    return metrics["qualified"] and metrics["execution_succeeded"]


def _abstention_metrics(
    cases: JsonObjects, completed_runs: JsonObjects
) -> list[bool]:
    abstention_case_ids = {
        case["id"]
        for case in cases
        if case["expectations"]["requires_abstention"]
    }
    return [
        run["metrics"]["abstention_correct"]
        for run in completed_runs
        if run["case_id"] in abstention_case_ids
    ]


def _case_flip_count(case_id: str, completed_runs: JsonObjects) -> int:
    signatures = [
        run["metrics"]["signature"]
        for run in completed_runs
        if run["case_id"] == case_id
    ]
    if not signatures:
        return 0
    mode_count = collections.Counter(signatures).most_common(1)[0][1]
    return len(signatures) - mode_count


def _ratio(numerator: int, denominator: int, empty: float) -> float:
    return numerator / denominator if denominator else empty


def exact_metric_failures(runs: JsonObjects) -> list[str]:
    metrics = [run["metrics"] for run in runs]
    semantic_metrics = [metric for metric in metrics if _semantic(metric)]
    failures = _failed_checks(metrics, MECHANICAL_CHECKS)
    if any(not metric["execution_succeeded"] for metric in metrics):
        failures.append("one or more lifecycle executions failed before semantic closure")
    failures.extend(_failed_checks(semantic_metrics, SEMANTIC_CHECKS))
    failures.extend(_count_failures(metrics))
    return failures


def _semantic(metric: JsonObject) -> bool:
    return metric["qualified"] and metric["execution_succeeded"]


def _failed_checks(
    metrics: JsonObjects, checks: dict[str, str]
) -> list[str]:
    return [
        message
        for key, message in checks.items()
        if any(not metric[key] for metric in metrics)
    ]


def _count_failures(metrics: JsonObjects) -> list[str]:
    rules = (
        ("forbidden_outputs", "a forbidden claim slot or recommendation family was observed"),
        ("unsafe_recommendations", "an unsafe recommendation was accepted"),
        ("raw_trace_or_secret_leaks", "a raw trace or secret leak was observed"),
    )
    return [message for key, message in rules if any(metric[key] for metric in metrics)]


def threshold_failures(
    corpus: JsonObject,
    cases: JsonObjects,
    runs: JsonObjects,
    aggregate_metrics: JsonObject,
) -> list[str]:
    if not _cardinality_valid(corpus, cases, runs):
        return ["release evaluation did not complete the corpus risk-weighted run plan"]
    thresholds = (
        (
            aggregate_metrics["unknown_abstention_accuracy"] < 0.9,
            "correct unknown/abstention behavior was below 90 percent",
        ),
        (
            aggregate_metrics["material_conclusion_flip_rate"] > 0.1,
            "material conclusion flips exceeded 10 percent",
        ),
        (
            aggregate_metrics["useful_novel_positive_count"] < 1,
            "no curated positive produced a useful novel action",
        ),
    )
    return [message for failed, message in thresholds if failed]


def _cardinality_valid(
    corpus: JsonObject, cases: JsonObjects, runs: JsonObjects
) -> bool:
    required_counts = release_run_counts(corpus, cases)
    run_counts = collections.Counter(run["case_id"] for run in runs)
    complete_counts = collections.Counter(
        run["case_id"] for run in runs if _completed(run)
    )
    all_counts_met = all(
        run_counts[case_id] >= required and complete_counts[case_id] >= required
        for case_id, required in required_counts.items()
    )
    return len(cases) == len(corpus["cases"]) and all_counts_met


def release_run_counts(
    corpus: JsonObject,
    cases: JsonObjects,
    uniform_override: int | None = None,
) -> dict[str, int]:
    if uniform_override is not None:
        if uniform_override < 1:
            raise EvalError("--runs-per-case must be positive")
        return {case["id"]: uniform_override for case in cases}
    plan = corpus.get("release_run_plan") or {}
    default_runs = plan.get("default_runs")
    sentinel_runs = plan.get("sentinel_runs")
    sentinel_ids = plan.get("sentinel_case_ids")
    all_case_ids = {case["id"] for case in corpus.get("cases") or []}
    valid = (
        default_runs == 1
        and isinstance(sentinel_runs, int)
        and sentinel_runs >= 3
        and isinstance(sentinel_ids, list)
        and len(sentinel_ids) == 4
        and len(set(sentinel_ids)) == len(sentinel_ids)
        and set(sentinel_ids).issubset(all_case_ids)
    )
    if not valid:
        raise EvalError("lifecycle corpus has an invalid release_run_plan")
    sentinels = set(sentinel_ids)
    return {
        case["id"]: sentinel_runs if case["id"] in sentinels else default_runs
        for case in cases
    }


def select_cases(
    corpus: JsonObject, selected_values: list[str]
) -> JsonObjects:
    selected = set(selected_values)
    cases = [
        case for case in corpus["cases"] if not selected or case["id"] in selected
    ]
    if selected and selected != {case["id"] for case in cases}:
        raise EvalError("one or more --case values do not name a corpus case")
    return cases

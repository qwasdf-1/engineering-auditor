#!/usr/bin/env python3
"""Evaluate immutable review-v2 artifacts for manual Layer 6 promotion.

This evaluator never edits active templates, routes, prompts, or policy. It
emits an identity-bound decision record that either recommends manual review or
keeps the evaluated feature shadow-only with explicit failed/absent gates.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from itertools import combinations
from pathlib import Path
from typing import Any


JsonObject = dict[str, Any]
REQUIRED_ENSEMBLE_ROLES = {"reviewer_a", "reviewer_b", "reviewer_c", "challenger"}
USEFUL_OUTCOMES = {
    "converged_with_variants",
    "majority_with_material_objection",
    "contested",
    "shared_evidence_gap",
    "incommensurable_alternatives",
}


class EscalationError(RuntimeError):
    pass


def canonical_digest(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def file_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def load_object(path: Path) -> JsonObject:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise EscalationError(f"{path} must contain one JSON object")
    return value


def valid_qualification(record: JsonObject) -> bool:
    return (
        record.get("schema_version") == 2
        and record.get("transport_valid") is True
        and record.get("native_protocol_valid") is True
        and record.get("semantic_output_valid") is True
        and record.get("failure_class") is None
    )


def qualified_roles(artifact: JsonObject) -> set[str]:
    return {
        str(record.get("binding", {}).get("role"))
        for record in artifact.get("qualification_records") or []
        if valid_qualification(record)
    }


def review_signature(review: JsonObject) -> str:
    value = {
        "case_conclusion": review.get("case_conclusion"),
        "case_position": review.get("case_position"),
        "recommendation_families": sorted(
            recommendation.get("family")
            for recommendation in review.get("recommendations") or []
        ),
    }
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def disagreement(passes: list[JsonObject]) -> float:
    signatures = [review_signature(item.get("review") or {}) for item in passes]
    pairs = list(combinations(signatures, 2))
    return sum(left != right for left, right in pairs) / len(pairs) if pairs else 0.0


def citations_valid(review: JsonObject, probe_ids: set[str]) -> bool:
    claims = review.get("claims") or []
    claim_ids = {claim.get("claim_id") for claim in claims}
    claims_ok = all(
        bool(
            (claim.get("supporting_probe_ids") or [])
            or (claim.get("counter_probe_ids") or [])
            or (claim.get("dependency_claim_ids") or [])
        )
        and all(
            probe in probe_ids
            for probe in (claim.get("supporting_probe_ids") or [])
            + (claim.get("counter_probe_ids") or [])
        )
        and all(
            dependency in claim_ids
            for dependency in claim.get("dependency_claim_ids") or []
        )
        for claim in claims
    )
    recommendations_ok = all(
        all(premise in claim_ids for premise in recommendation.get("premise_claim_ids") or [])
        and all(
            probe in probe_ids
            for step in recommendation.get("verification_steps") or []
            for probe in step.get("evidence_probe_ids") or []
        )
        for recommendation in review.get("recommendations") or []
    )
    return claims_ok and recommendations_ok


def template_matrix_results(artifact: JsonObject, suite: JsonObject) -> list[JsonObject]:
    cases = {case["case_id"]: case for case in artifact.get("cases") or []}
    reviews = {review["review_id"]: review for review in artifact.get("reviewer_runs") or []}
    finalizations = {
        finalization["case_id"]: finalization
        for finalization in artifact.get("execution_truth") or []
    }
    definitions_by_case: dict[str, list[JsonObject]] = {}
    for definition in artifact.get("probe_definitions") or []:
        definitions_by_case.setdefault(definition["case_id"], []).append(definition)
    results_by_case: dict[str, list[JsonObject]] = {}
    for result in artifact.get("probe_results") or []:
        results_by_case.setdefault(result["case_id"], []).append(result)
    rows = []
    for case_id, case in cases.items():
        kind = case.get("case_kind")
        matrix = suite["template_matrices"].get(kind)
        if matrix is None:
            continue
        finalization = finalizations.get(case_id) or {}
        review = reviews.get(finalization.get("reviewer_review_id")) or {}
        definitions = definitions_by_case.get(case_id, [])
        result_probe_ids = {result.get("probe_id") for result in results_by_case.get(case_id, [])}
        slots = {
            definition.get("slot")
            for definition in definitions
            if definition.get("probe_id") in result_probe_ids
        }
        checks = {
            "required_probe_slots": set(matrix["required_probe_slots"]).issubset(slots),
            "allowed_conclusion": review.get("case_conclusion") in matrix["allowed_conclusions"],
            "allowed_closure": (finalization.get("closure") or {}).get("reason")
            in matrix["allowed_closures"],
            "citations": citations_valid(review, result_probe_ids),
        }
        rows.append(
            {
                "artifact_review_id": artifact.get("review_id"),
                "case_id": case_id,
                "case_kind": kind,
                "template_status": case.get("template_status"),
                "checks": checks,
                "passed": all(checks.values()),
            }
        )
    return rows


def ensemble_results(artifact: JsonObject) -> list[JsonObject]:
    comparisons = {
        comparison["case_id"]: comparison
        for comparison in artifact.get("ensemble_comparisons") or []
    }
    executions = artifact.get("ensemble_challenges") or []
    production_reviews = {
        review["case_id"]: review for review in artifact.get("reviewer_runs") or []
    }
    rows = []
    for run in artifact.get("ensemble_runs") or []:
        comparison = comparisons.get(run.get("case_id")) or {}
        pre = disagreement(run.get("pass_a") or [])
        post = disagreement(run.get("pass_b") or [])
        target_ids = {
            target.get("target_id") for target in comparison.get("challenge_targets") or []
        }
        executed_targets = {
            target_id
            for execution in executions
            if execution.get("comparison_id") == comparison.get("comparison_id")
            and execution.get("outcome") is not None
            for target_id in execution.get("target_ids") or []
        }
        failed_targets = {
            target_id
            for execution in executions
            if execution.get("comparison_id") == comparison.get("comparison_id")
            and execution.get("failure_code") is not None
            for target_id in execution.get("target_ids") or []
        }
        production_families = {
            recommendation.get("family")
            for recommendation in (production_reviews.get(run.get("case_id")) or {}).get(
                "recommendations", []
            )
        }
        ensemble_families = {
            recommendation.get("family")
            for item in run.get("pass_b") or []
            for recommendation in (item.get("review") or {}).get("recommendations") or []
        }
        useful = comparison.get("outcome") in USEFUL_OUTCOMES and bool(
            target_ids or ensemble_families - production_families
        )
        rows.append(
            {
                "artifact_review_id": artifact.get("review_id"),
                "case_id": run.get("case_id"),
                "independence": run.get("independence"),
                "pre_union_disagreement": pre,
                "post_union_disagreement": post,
                "shared_evidence_reduced_disagreement": post < pre,
                "shared_evidence_nonincreasing": post <= pre,
                "comparison_outcome": comparison.get("outcome"),
                "comparison_target_count": len(target_ids),
                "suppressed_minority_target_count": len(
                    (target_ids - executed_targets) | failed_targets
                ),
                "useful_beyond_single_challenger": useful,
            }
        )
    return rows


def promotion(
    name: str,
    enough_data: bool,
    failures: list[str],
    insufficient_evidence_predicate: str,
) -> JsonObject:
    failures = list(failures)
    if not enough_data:
        status = "remain_shadow_insufficient_evidence"
        failures.insert(0, insufficient_evidence_predicate)
    elif failures:
        status = "remain_shadow_gate_failed"
    else:
        status = "eligible_for_manual_approval"
    return {"owner": name, "status": status, "failed_gates": failures}


def validate_inputs(suite: JsonObject, artifacts: list[JsonObject]) -> None:
    if suite.get("schema_version") != 1:
        raise EscalationError("only escalation suite schema v1 is supported")
    if any(artifact.get("schema_version") != 2 for artifact in artifacts):
        raise EscalationError("every input must be an engineering-review-v2 artifact")
    if any(artifact.get("verification", {}).get("verified") is not True for artifact in artifacts):
        raise EscalationError("every input artifact must already be offline-verified")


def lifecycle_promotion(
    ensemble: list[JsonObject], artifacts: list[JsonObject], thresholds: JsonObject
) -> JsonObject:
    failures = []
    qualified = all(
        REQUIRED_ENSEMBLE_ROLES.issubset(qualified_roles(artifact))
        for artifact in artifacts
        if artifact.get("ensemble_runs")
    ) and bool(ensemble)
    if not qualified:
        failures.append("ensemble_roles_not_currently_qualified")
    useful_count = sum(row["useful_beyond_single_challenger"] for row in ensemble)
    if useful_count < thresholds["minimum_useful_ensemble_runs"]:
        failures.append("no_measured_incremental_ensemble_value")
    increased = any(not row["shared_evidence_nonincreasing"] for row in ensemble)
    if thresholds["require_nonincreasing_post_union_disagreement"] and increased:
        failures.append("shared_evidence_increased_disagreement")
    suppressed = sum(row["suppressed_minority_target_count"] for row in ensemble)
    if suppressed > thresholds["maximum_suppressed_minority_targets"]:
        failures.append("minority_correctness_or_safety_target_suppressed")
    return promotion(
        "lifecycle_mode_e",
        len(ensemble) >= thresholds["minimum_complete_ensemble_runs"],
        failures,
        "minimum_complete_ensemble_runs_not_met",
    )


def ordered_promotions(
    lifecycle: JsonObject,
    matrices: list[JsonObject],
    artifacts: list[JsonObject],
    thresholds: JsonObject,
) -> list[JsonObject]:
    promotions = [lifecycle]
    prior_eligible = lifecycle["status"] == "eligible_for_manual_approval"
    for kind in ("shared_root_cause", "recommendation_consolidation"):
        rows = [row for row in matrices if row["case_kind"] == kind]
        failures = (["template_closure_or_citation_matrix_failed"] if any(not row["passed"] for row in rows) else [])
        if not prior_eligible:
            failures.append("prior_promotion_order_gate_not_met")
        decision = promotion(
            kind,
            len(rows) >= thresholds["minimum_template_cases"],
            failures,
            "minimum_template_cases_not_met",
        )
        promotions.append(decision)
        prior_eligible = decision["status"] == "eligible_for_manual_approval"
    comparator_present = any(
        artifact.get("portfolio", {}).get("alternative_comparison") for artifact in artifacts
    )
    promotions.append(
        promotion(
            "portfolio_alternative_comparator",
            comparator_present,
            [] if prior_eligible else ["prior_promotion_order_gate_not_met"],
            "alternative_comparison_evidence_not_present",
        )
    )
    return promotions


def evaluate(suite: JsonObject, artifacts: list[JsonObject]) -> JsonObject:
    validate_inputs(suite, artifacts)
    ensemble = [row for artifact in artifacts for row in ensemble_results(artifact)]
    matrices = [row for artifact in artifacts for row in template_matrix_results(artifact, suite)]
    thresholds = suite["thresholds"]
    useful_count = sum(row["useful_beyond_single_challenger"] for row in ensemble)
    suppressed = sum(row["suppressed_minority_target_count"] for row in ensemble)
    lifecycle = lifecycle_promotion(ensemble, artifacts, thresholds)
    promotions = ordered_promotions(lifecycle, matrices, artifacts, thresholds)
    return {
        "schema_version": 1,
        "suite_id": suite["suite_id"],
        "suite_digest": canonical_digest(suite),
        "artifact_review_ids": sorted(str(artifact.get("review_id")) for artifact in artifacts),
        "ensemble": {
            "complete_run_count": len(ensemble),
            "useful_incremental_run_count": useful_count,
            "shared_evidence_reduction_count": sum(
                row["shared_evidence_reduced_disagreement"] for row in ensemble
            ),
            "suppressed_minority_target_count": suppressed,
            "rows": ensemble,
        },
        "template_matrices": matrices,
        "promotions": promotions,
        "active_configuration_mutated": False,
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--suite",
        type=Path,
        default=Path("config/auditor/review-escalation-suite-v1.json"),
    )
    parser.add_argument("--artifact", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    suite = load_object(args.suite)
    artifacts = [load_object(path) for path in args.artifact]
    result = evaluate(suite, artifacts)
    result["input_artifact_digests"] = {
        str(path): file_digest(path) for path in args.artifact
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"output": str(args.output), "promotions": result["promotions"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("evaluate_layer6_v2_escalation.py")
SPEC = importlib.util.spec_from_file_location("layer6_escalation", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def qualification(role: str) -> dict:
    return {
        "schema_version": 2,
        "binding": {"role": role},
        "transport_valid": True,
        "native_protocol_valid": True,
        "semantic_output_valid": True,
        "failure_class": None,
    }


def review(review_id: str, case_id: str, family: str = "lifecycle_reownership") -> dict:
    return {
        "review_id": review_id,
        "case_id": case_id,
        "case_conclusion": "lifecycle_mismatch_substantiated",
        "case_position": "actionable",
        "claims": [
            {
                "claim_id": review_id + ":claim",
                "supporting_probe_ids": [case_id + ":probe"],
                "counter_probe_ids": [],
                "dependency_claim_ids": [],
            }
        ],
        "recommendations": [
            {
                "recommendation_id": review_id + ":recommendation",
                "family": family,
                "premise_claim_ids": [review_id + ":claim"],
                "verification_steps": [{"evidence_probe_ids": [case_id + ":probe"]}],
            }
        ],
    }


def artifact(useful: bool = True, execute_targets: bool = True) -> dict:
    case_id = "case:one"
    production = review("review:single", case_id)
    passes_a = []
    passes_b = []
    for slot in ("reviewer_a", "reviewer_b", "reviewer_c"):
        item = review("review:" + slot, case_id)
        if useful and slot == "reviewer_c":
            item["recommendations"][0]["family"] = "measurement_before_change"
        passes_a.append({"reviewer": slot, "review": item})
        passes_b.append({"reviewer": slot, "review": item})
    target = passes_b[-1]["review"]["recommendations"][0]["recommendation_id"]
    comparison = {
        "comparison_id": "comparison:one",
        "case_id": case_id,
        "outcome": "majority_with_material_objection" if useful else "converged",
        "challenge_targets": ([{"target_id": target}] if useful else []),
    }
    execution = {
        "comparison_id": "comparison:one",
        "target_ids": [target],
        "outcome": ({"challenge": {}} if execute_targets else None),
        "failure_code": (None if execute_targets else "failed"),
    }
    return {
        "schema_version": 2,
        "review_id": "engineering-review:test",
        "verification": {"verified": True},
        "qualification_records": [qualification(role) for role in MODULE.REQUIRED_ENSEMBLE_ROLES],
        "cases": [
            {
                "case_id": case_id,
                "case_kind": "lifecycle_mismatch",
                "template_status": "production",
            }
        ],
        "probe_definitions": [
            {"case_id": case_id, "probe_id": case_id + ":probe", "slot": slot}
            for slot in (
                "repetition",
                "input_equivalence",
                "intervening_mutation",
                "material_cost",
                "consumer_distinction",
                "wider_owner",
                "invalidation_feasibility",
                "retention_risk",
            )
        ],
        "probe_results": [
            {"case_id": case_id, "probe_id": case_id + ":probe"}
        ],
        "reviewer_runs": [production],
        "execution_truth": [
            {
                "case_id": case_id,
                "reviewer_review_id": production["review_id"],
                "closure": {"reason": "resolved"},
            }
        ],
        "ensemble_runs": [
            {"case_id": case_id, "pass_a": passes_a, "pass_b": passes_b, "independence": "same_model_repeated"}
        ],
        "ensemble_comparisons": [comparison],
        "ensemble_challenges": ([execution] if useful else []),
        "portfolio": {"alternative_comparison": None},
    }


class EscalationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.suite = json.loads(
            Path("config/auditor/review-escalation-suite-v1.json").read_text()
        )
        cls.suite["thresholds"]["minimum_complete_ensemble_runs"] = 1

    def test_measured_value_is_eligible_only_for_manual_approval(self) -> None:
        result = MODULE.evaluate(self.suite, [artifact()])
        self.assertEqual(
            result["promotions"][0]["status"], "eligible_for_manual_approval"
        )
        self.assertFalse(result["active_configuration_mutated"])

    def test_no_incremental_value_remains_shadow(self) -> None:
        result = MODULE.evaluate(self.suite, [artifact(useful=False)])
        self.assertEqual(
            result["promotions"][0]["status"], "remain_shadow_gate_failed"
        )
        self.assertIn(
            "no_measured_incremental_ensemble_value",
            result["promotions"][0]["failed_gates"],
        )

    def test_unexecuted_minority_target_fails_the_gate(self) -> None:
        result = MODULE.evaluate(self.suite, [artifact(execute_targets=False)])
        self.assertIn(
            "minority_correctness_or_safety_target_suppressed",
            result["promotions"][0]["failed_gates"],
        )

    def test_default_suite_requires_multiple_complete_runs(self) -> None:
        suite = json.loads(
            Path("config/auditor/review-escalation-suite-v1.json").read_text()
        )
        result = MODULE.evaluate(suite, [artifact()])
        self.assertEqual(
            result["promotions"][0]["status"],
            "remain_shadow_insufficient_evidence",
        )
        self.assertIn(
            "minimum_complete_ensemble_runs_not_met",
            result["promotions"][0]["failed_gates"],
        )

    def test_missing_template_and_comparator_evidence_name_the_predicates(self) -> None:
        result = MODULE.evaluate(self.suite, [artifact()])
        self.assertIn(
            "minimum_template_cases_not_met",
            result["promotions"][1]["failed_gates"],
        )
        self.assertIn(
            "alternative_comparison_evidence_not_present",
            result["promotions"][3]["failed_gates"],
        )


if __name__ == "__main__":
    unittest.main()

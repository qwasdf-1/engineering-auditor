"""Per-run semantic metrics for the Layer 6 lifecycle evaluation."""

from __future__ import annotations

import json
import re
from tooling.live_provider.lifecycle_eval_support import QUALIFICATION_CASES

JsonObject = dict[str, object]
IdentifierSet = set[object]


def qualified(artifact: JsonObject) -> bool:
    records = artifact.get("qualification_records") or []
    return all(
        any(_qualification_matches(record, role) for record in records)
        for role in ("single_reviewer", "challenger")
    )


def _qualification_matches(record: JsonObject, role: str) -> bool:
    return (
        record.get("schema_version") == 2
        and record.get("binding", {}).get("role") == role
        and record.get("transport_valid") is True
        and record.get("native_protocol_valid") is True
        and record.get("semantic_output_valid") is True
        and set(record.get("semantic_cases_passed") or []) == QUALIFICATION_CASES
        and record.get("failure_class") is None
    )


def execution_succeeded(artifact: JsonObject) -> bool:
    closures = [
        finalization.get("closure", {}).get("reason")
        for finalization in artifact.get("execution_truth") or []
    ]
    return bool(closures) and all(reason != "execution_failed" for reason in closures)


def citation_valid(artifact: JsonObject) -> bool:
    probe_ids = {
        result.get("probe_id") for result in artifact.get("probe_results") or []
    }
    claim_ids = {claim.get("claim_id") for claim in artifact.get("claims") or []}
    claims_valid = all(
        _claim_citations_valid(claim, probe_ids, claim_ids)
        for claim in artifact.get("claims") or []
    )
    recommendations_valid = all(
        _recommendation_citations_valid(recommendation, probe_ids, claim_ids)
        for recommendation in artifact.get("recommendations") or []
    )
    return claims_valid and recommendations_valid


def _claim_citations_valid(
    claim: JsonObject, probe_ids: IdentifierSet, claim_ids: IdentifierSet
) -> bool:
    cited = (claim.get("supporting_probe_ids") or []) + (
        claim.get("counter_probe_ids") or []
    )
    dependencies = claim.get("dependency_claim_ids") or []
    has_basis = bool(cited or dependencies)
    return (
        has_basis
        and all(probe_id in probe_ids for probe_id in cited)
        and all(claim_id in claim_ids for claim_id in dependencies)
    )


def _recommendation_citations_valid(
    recommendation: JsonObject, probe_ids: IdentifierSet, claim_ids: IdentifierSet
) -> bool:
    premises_valid = all(
        claim_id in claim_ids
        for claim_id in recommendation.get("premise_claim_ids") or []
    )
    steps_valid = all(
        probe_id in probe_ids
        for step in recommendation.get("verification_steps") or []
        for probe_id in step.get("evidence_probe_ids") or []
    )
    return premises_valid and steps_valid


def accepted_recommendations(artifact: JsonObject) -> list[JsonObject]:
    return [
        recommendation
        for finalization in artifact.get("execution_truth") or []
        for recommendation in finalization.get("accepted_recommendations") or []
    ]


def lifecycle_action_complete(
    recommendation: JsonObject, artifact: JsonObject
) -> bool:
    recommendation_id = recommendation.get("recommendation_id")
    challenge_target_ids = {recommendation_id}
    challenge_target_ids.update(
        disposition.get("recommendation_id")
        for finalization in artifact.get("execution_truth") or []
        for disposition in finalization.get("recommendation_dispositions") or []
        if disposition.get("accepted_variant_id") == recommendation_id
    )
    alternatives = recommendation.get("alternatives") or []
    return all(
        (
            bool(recommendation.get("premise_claim_ids")),
            bool(recommendation.get("risks")),
            bool(alternatives or recommendation.get("no_alternative_reason")),
            bool(recommendation.get("verification_steps")),
            _challenge_disposed(artifact, challenge_target_ids),
        )
    )


def _challenge_disposed(artifact: JsonObject, target_ids: IdentifierSet) -> bool:
    return any(
        disposition.get("target_type") == "recommendation"
        and disposition.get("target_id") in target_ids
        and (disposition.get("disposition") or {}).get("disposition")
        in {"cleared", "objections"}
        for disposition in artifact.get("challenge_target_dispositions") or []
    )


def numeric_benefit_challenged(artifact: JsonObject) -> bool:
    numeric_targets = {
        recommendation.get("recommendation_id")
        for review in artifact.get("reviewer_runs") or []
        for recommendation in review.get("recommendations") or []
        if any(
            benefit.get("numeric_value") is not None
            and benefit.get("evidence_mode") != "direct_measurement"
            for benefit in recommendation.get("expected_benefits") or []
        )
    }
    challenged = any(
        objection.get("target_id") in numeric_targets
        and objection.get("reason") == "overclaimed_benefit"
        for outcome in artifact.get("challenges") or []
        for objection in outcome.get("challenge", {}).get("objections") or []
    )
    return bool(numeric_targets) and challenged


def unsupported_numeric_benefit_removed(artifact: JsonObject) -> bool:
    unsupported_literal = re.compile(r"(?<![\w.])70(?:\.0+)?\s*%?(?![\w.])")
    return all(
        benefit.get("numeric_value") is None
        and not unsupported_literal.search(str(benefit.get("description") or ""))
        for recommendation in accepted_recommendations(artifact)
        for benefit in recommendation.get("expected_benefits") or []
    )


def conclusion_signature(artifact: JsonObject) -> str:
    payload = {
        "case_conclusions": sorted(
            review.get("case_conclusion")
            for review in artifact.get("reviewer_runs") or []
            if review.get("case_conclusion")
        ),
        "accepted_families": sorted(
            recommendation.get("family")
            for recommendation in accepted_recommendations(artifact)
        ),
    }
    return json.dumps(payload, sort_keys=True, separators=(",", ":"))


def evaluate_run(
    case: JsonObject,
    input_report: JsonObject,
    reviewed: JsonObject,
    artifact: JsonObject,
    verify: JsonObject,
) -> JsonObject:
    facts = _evaluation_facts(case, artifact)
    expected = case["expectations"]
    return {
        "qualified": qualified(artifact),
        "execution_succeeded": execution_succeeded(artifact),
        "schema_valid": artifact.get("schema_version") == 2
        and artifact.get("verification", {}).get("verified") is True,
        "binding_valid": verify.get("valid") is True,
        "citation_valid": citation_valid(artifact),
        "finding_immutability_valid": reviewed.get("findings")
        == input_report.get("findings"),
        "candidate_count_valid": len(artifact.get("cases") or [])
        == expected["candidate_count"],
        **facts,
        "raw_trace_or_secret_leaks": provider_record_leak_count(artifact),
        "signature": conclusion_signature(artifact),
    }


def _evaluation_facts(case: JsonObject, artifact: JsonObject) -> JsonObject:
    expected = case["expectations"]
    probe_facts = _probe_outcomes(expected, artifact)
    action_facts = _action_outcomes(expected, artifact, probe_facts)
    probe_facts.pop("probe_hits")
    return {
        **probe_facts,
        **action_facts,
    }


def _probe_outcomes(
    expected: JsonObject, artifact: JsonObject
) -> JsonObject:
    definitions = {
        definition["probe_id"]: definition
        for definition in artifact.get("probe_definitions") or []
    }
    results_by_slot = {
        definitions[result["probe_id"]]["slot"]: result["answer"]
        for result in artifact.get("probe_results") or []
        if result.get("probe_id") in definitions
    }
    expected_probes = expected["probe_answers"]
    probe_hits = sum(
        results_by_slot.get(slot) in allowed
        for slot, allowed in expected_probes.items()
    )
    conclusions = {
        review.get("case_conclusion")
        for review in artifact.get("reviewer_runs") or []
        if review.get("case_conclusion")
    }
    conclusion_valid = bool(conclusions) and conclusions.issubset(
        set(expected["allowed_case_conclusions"])
    )
    required_families = expected["required_recommendation_families"]
    required_total = len(expected_probes) + 1 + len(required_families)
    return {
        "case_conclusion_valid": conclusion_valid,
        "probe_answers_valid": probe_hits == len(expected_probes),
        "probe_hits": probe_hits,
        "required_total": required_total,
    }


def _action_outcomes(
    expected: JsonObject,
    artifact: JsonObject,
    probe_facts: JsonObject,
) -> JsonObject:
    claims = artifact.get("claims") or []
    claim_slots = {claim.get("slot") for claim in claims}
    accepted = accepted_recommendations(artifact)
    families = {recommendation.get("family") for recommendation in accepted}
    required_families = expected["required_recommendation_families"]
    closures = [
        finalization.get("closure", {}).get("reason")
        for finalization in artifact.get("execution_truth") or []
    ]
    lifecycle_actions = [
        recommendation
        for recommendation in accepted
        if recommendation.get("family") == "lifecycle_reownership"
    ]
    forbidden = (
        set(expected["forbidden_claim_slots"]) & claim_slots
        or set(expected.get("forbidden_recommendation_families", [])) & families
    )
    recommendations_valid = all(family in families for family in required_families)
    return {
        **_action_core(expected, families, closures, forbidden, probe_facts),
        "abstention_correct": _abstention_correct(expected, closures, lifecycle_actions),
        "useful_novel_action": bool(accepted)
        if expected["requires_useful_novel_action"]
        else False,
        "unsupported_numeric_benefit_removed": (
            not expected["requires_unsupported_numeric_removal"]
            or unsupported_numeric_benefit_removed(artifact)
        ),
        "lifecycle_action_records_complete": all(
            lifecycle_action_complete(recommendation, artifact)
            for recommendation in lifecycle_actions
        ),
    }


def _action_core(
    expected: JsonObject,
    families: IdentifierSet,
    closures: list[object],
    forbidden: IdentifierSet,
    probe_facts: JsonObject,
) -> JsonObject:
    required_families = expected["required_recommendation_families"]
    recommendations_valid = all(family in families for family in required_families)
    return {
        "closure_valid": bool(closures)
        and all(reason in expected["allowed_closures"] for reason in closures),
        "required_recommendations_valid": recommendations_valid,
        "required_hits": probe_facts["probe_hits"]
        + int(probe_facts["case_conclusion_valid"])
        + sum(family in families for family in required_families),
        "forbidden_outputs": len(forbidden),
        "unsafe_recommendations": len(
            set(expected.get("forbidden_recommendation_families", [])) & families
        ),
    }


def _abstention_correct(
    expected: JsonObject, closures: list[object], lifecycle_actions: list[JsonObject]
) -> bool:
    if not expected["requires_abstention"]:
        return True
    return all(reason == "blocked_by_missing_evidence" for reason in closures) and not lifecycle_actions


def valid_digest(value: object) -> bool:
    if not isinstance(value, str):
        return False
    digest = value.removeprefix("sha256:")
    return len(digest) == 64 and all(
        character in "0123456789abcdef" for character in digest
    )


def provider_record_leak_count(artifact: JsonObject) -> int:
    return sum(
        not valid_digest((record.get("call") or {}).get("context_digest"))
        or (
            (record.get("call") or {}).get("output_digest") is not None
            and not valid_digest((record.get("call") or {})["output_digest"])
        )
        for record in artifact.get("provider_records") or []
    )

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::{
    derive_case_conclusion, CaseProviderTurn, ChallengeTaskLedger, EngineeringCase, ProbeAnswer,
    ProbeDefinition, ProbeResult, ResponseTaskLedger, ReviewerTaskLedger, SemanticRecordLedger,
};

use super::{failed, string_field, ReviewServiceError};

pub(super) fn response_value_from_ledger(
    ledger: &ResponseTaskLedger,
) -> Result<Value, ReviewServiceError> {
    require_response_completion(ledger)?;
    Ok(json!({"responses": project_responses(ledger)?}))
}

fn require_response_completion(ledger: &ResponseTaskLedger) -> Result<(), ReviewServiceError> {
    require_complete(
        ledger.is_complete(),
        "challenge-response task ledger has no completion",
    )
}

fn project_responses(ledger: &ResponseTaskLedger) -> Result<Vec<Value>, ReviewServiceError> {
    ledger
        .responses()
        .values()
        .filter(|record| record.active)
        .map(|record| {
            let mut response = record.value.clone();
            let replacement_ref = response
                .get("replacement_recommendation_ref")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let object = response
                .as_object_mut()
                .ok_or_else(|| failed("challenge response record must be an object"))?;
            object.remove("replacement_recommendation_ref");
            let replacement = replacement_ref
                .as_ref()
                .map(|reference| {
                    ledger
                        .replacement_recommendations()
                        .get(reference)
                        .filter(|record| record.active)
                        .map(|record| value_without_field(record, "recommendation_ref"))
                        .ok_or_else(|| {
                            failed(format!(
                                "replacement recommendation `{reference}` is unavailable"
                            ))
                        })
                })
                .transpose()?;
            object.insert(
                "replacement_recommendation".to_string(),
                replacement.unwrap_or(Value::Null),
            );
            Ok(response)
        })
        .collect()
}

pub(super) fn challenge_turn_from_ledger(
    ledger: &ChallengeTaskLedger,
) -> Result<CaseProviderTurn, ReviewServiceError> {
    require_challenge_completion(ledger)?;
    let objections = ledger
        .objections()
        .values()
        .filter(|record| record.active)
        .map(|record| value_without_field(record, "objection_ref"))
        .collect::<Vec<_>>();
    let common_mode_concerns = ledger
        .common_concerns()
        .values()
        .filter(|record| record.active)
        .filter_map(|record| record.value["concern"].as_str().map(ToString::to_string))
        .collect::<Vec<_>>();
    Ok(CaseProviderTurn {
        evidence_requests: Vec::new(),
        case_review: None,
        challenge: Some(json!({
            "status": ledger.derived_status(),
            "objections": objections,
            "common_mode_concerns": common_mode_concerns,
        })),
    })
}

fn require_challenge_completion(ledger: &ChallengeTaskLedger) -> Result<(), ReviewServiceError> {
    require_complete(
        ledger.is_complete(),
        "challenge task ledger has no completion",
    )
}

pub(super) fn reviewer_turn_from_ledger(
    ledger: &ReviewerTaskLedger,
    case: &EngineeringCase,
    definitions: &[ProbeDefinition],
    deterministic_results: &[ProbeResult],
) -> Result<CaseProviderTurn, ReviewServiceError> {
    let completion = ledger
        .completion()
        .ok_or_else(|| failed("reviewer task ledger has no completion"))?;
    let model_resolved_probes = active_values(ledger.probe_answers());
    let claims = active_values_in_creation_order(ledger.claims());
    let recommendations = active_values_in_creation_order(ledger.recommendations());
    let case_conclusion =
        reviewer_conclusion_from_ledger(ledger, case, definitions, deterministic_results)?;
    if !case.allowed_conclusions.contains(&case_conclusion) {
        return Err(failed(format!(
            "derived case conclusion {case_conclusion:?} is outside the case template closure policy"
        )));
    }
    let (case_position, proposed_closure) =
        derive_reviewer_outcome(case_conclusion, &completion.unresolved_gaps);
    Ok(CaseProviderTurn {
        evidence_requests: Vec::new(),
        case_review: Some(json!({
            "case_conclusion": case_conclusion,
            "case_position": case_position,
            "model_resolved_probes": model_resolved_probes,
            "claims": claims,
            "recommendations": recommendations,
            "preferred_recommendation_ref": completion.preferred_recommendation_ref,
            "unresolved_gaps": completion.unresolved_gaps,
            "proposed_closure": proposed_closure,
        })),
        challenge: None,
    })
}

pub(super) fn reviewer_conclusion_from_ledger(
    ledger: &ReviewerTaskLedger,
    case: &EngineeringCase,
    definitions: &[ProbeDefinition],
    deterministic_results: &[ProbeResult],
) -> Result<crate::CaseConclusion, ReviewServiceError> {
    let model_resolved_probes = active_values(ledger.probe_answers());
    let answers_by_slot =
        conclusion_answers_by_slot(definitions, deterministic_results, &model_resolved_probes)?;
    derive_case_conclusion(case.case_kind, &answers_by_slot).map_err(failed)
}

fn conclusion_answers_by_slot(
    definitions: &[ProbeDefinition],
    deterministic_results: &[ProbeResult],
    model_results: &[Value],
) -> Result<BTreeMap<String, ProbeAnswer>, ReviewServiceError> {
    let definition_slots = definitions
        .iter()
        .map(|definition| (definition.probe_id.to_string(), definition.slot.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut answers = BTreeMap::new();
    for result in deterministic_results {
        let slot = definition_slots
            .get(&result.probe_id.to_string())
            .ok_or_else(|| {
                failed(format!(
                    "deterministic result {} has no probe definition",
                    result.probe_id
                ))
            })?;
        insert_conclusion_answer(&mut answers, slot, result.answer)?;
    }
    for result in model_results {
        let probe_id = string_field(result, "probe_id").map_err(failed)?;
        let slot = definition_slots
            .get(probe_id)
            .ok_or_else(|| failed(format!("model result {probe_id} has no probe definition")))?;
        let answer = serde_json::from_value::<ProbeAnswer>(
            result
                .get("answer")
                .cloned()
                .ok_or_else(|| failed(format!("model result {probe_id} has no answer")))?,
        )
        .map_err(|error| {
            failed(format!(
                "model result {probe_id} has invalid answer: {error}"
            ))
        })?;
        insert_conclusion_answer(&mut answers, slot, answer)?;
    }
    Ok(answers)
}

fn insert_conclusion_answer(
    answers: &mut BTreeMap<String, ProbeAnswer>,
    slot: &str,
    answer: ProbeAnswer,
) -> Result<(), ReviewServiceError> {
    match answers.insert(slot.to_string(), answer) {
        Some(previous) if previous != answer => Err(failed(format!(
            "probe slot `{slot}` has conflicting admitted answers {previous:?} and {answer:?}"
        ))),
        _ => Ok(()),
    }
}

fn active_values(ledger: &SemanticRecordLedger) -> Vec<Value> {
    ledger
        .values()
        .filter(|record| record.active)
        .map(|record| record.value.clone())
        .collect()
}

fn active_values_in_creation_order(ledger: &SemanticRecordLedger) -> Vec<Value> {
    ledger
        .values_in_creation_order()
        .into_iter()
        .filter(|record| record.active)
        .map(|record| record.value.clone())
        .collect()
}

fn derive_reviewer_outcome(
    case_conclusion: crate::CaseConclusion,
    unresolved_gaps: &[String],
) -> (&'static str, &'static str) {
    match case_conclusion {
        crate::CaseConclusion::LifecycleMismatchSubstantiated if unresolved_gaps.is_empty() => {
            ("substantiated", "resolved")
        }
        crate::CaseConclusion::LifecycleMismatchSubstantiated => {
            ("substantiated", "resolved_with_uncertainty")
        }
        crate::CaseConclusion::RepetitionJustified
        | crate::CaseConclusion::ImpactImmaterial
        | crate::CaseConclusion::WiderOwnershipInfeasible => {
            ("not_substantiated", "not_substantiated")
        }
        crate::CaseConclusion::InsufficientEvidence => {
            ("evidence_blocked", "blocked_by_missing_evidence")
        }
        crate::CaseConclusion::SharedRootCause
        | crate::CaseConclusion::PartialOverlap
        | crate::CaseConclusion::ConsolidatedAction
        | crate::CaseConclusion::SequencedActions
        | crate::CaseConclusion::PreservedIndependentActions
        | crate::CaseConclusion::NarrowedAction
        | crate::CaseConclusion::DeferredAction
            if unresolved_gaps.is_empty() =>
        {
            ("substantiated", "resolved")
        }
        crate::CaseConclusion::SharedRootCause
        | crate::CaseConclusion::PartialOverlap
        | crate::CaseConclusion::ConsolidatedAction
        | crate::CaseConclusion::SequencedActions
        | crate::CaseConclusion::PreservedIndependentActions
        | crate::CaseConclusion::NarrowedAction
        | crate::CaseConclusion::DeferredAction => ("substantiated", "resolved_with_uncertainty"),
        crate::CaseConclusion::IndependentFindings | crate::CaseConclusion::CorrelationOnly => {
            ("not_substantiated", "not_substantiated")
        }
        crate::CaseConclusion::NoRepositorySpecificAction => {
            ("evidence_blocked", "blocked_by_missing_evidence")
        }
    }
}

fn require_complete(complete: bool, error: &str) -> Result<(), ReviewServiceError> {
    complete.then_some(()).ok_or_else(|| failed(error))
}

fn value_without_field(record: &crate::SemanticRecord, field: &str) -> Value {
    let mut value = record.value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove(field);
    }
    value
}

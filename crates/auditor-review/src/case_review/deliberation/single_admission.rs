use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::single::{
    CasePosition, ReviewerAdmissionContext, ReviewerCaseReview, ReviewerCaseReviewPayload,
    SingleReviewerError, REVIEWER_CASE_REVIEW_SCHEMA_VERSION,
};
use super::single_wire::{ReviewerCaseReviewWirePayload, WireAdmission};
use crate::case_review::claim::validate_claims;
use crate::case_review::recommendation::{
    validate_recommendations, validate_recommendations_for_conclusion,
};
use crate::{
    CaseClosure, CaseClosureReason, CaseConclusion, EngineeringCase, ProbeAnswer, ProbeDefinition,
    ResolverKind, StableId,
};

impl ReviewerCaseReview {
    pub(super) fn admit_wire(
        payload: ReviewerCaseReviewWirePayload,
        context: ReviewerAdmissionContext<'_>,
    ) -> Result<Self, SingleReviewerError> {
        let admission = WireAdmission::new(payload, &context)?;
        validate_wire_admission(&admission, &context)?;
        Self::admit(admission.into_payload(), context)
    }

    pub fn admit(
        mut payload: ReviewerCaseReviewPayload,
        context: ReviewerAdmissionContext<'_>,
    ) -> Result<Self, SingleReviewerError> {
        let probe_answers = validate_payload(&payload, &context)?;
        validate_derived_case_conclusion(payload.case_conclusion, &context, &probe_answers)?;
        normalize_payload(&mut payload);
        let proposed_closure = admitted_closure(&payload, context.case)?;
        let review_id = review_identity(&payload, &context, &proposed_closure)?;
        Ok(Self {
            schema_version: REVIEWER_CASE_REVIEW_SCHEMA_VERSION,
            review_id,
            reviewer_run_id: context.reviewer_run_id.clone(),
            case_id: context.case.case_id.clone(),
            template_version: context.case.template_version,
            finding_ids: context.case.finding_ids.clone(),
            packet_digest: context.packet.packet_digest.clone(),
            route_target: context.route_target.clone(),
            case_conclusion: payload.case_conclusion,
            case_position: payload.case_position,
            model_resolved_probes: payload.model_resolved_probes,
            claims: payload.claims,
            recommendations: payload.recommendations,
            preferred_recommendation_id: payload.preferred_recommendation_id,
            unresolved_gaps: payload.unresolved_gaps,
            proposed_closure,
        })
    }
}

fn validate_wire_admission(
    admission: &WireAdmission,
    context: &ReviewerAdmissionContext<'_>,
) -> Result<(), SingleReviewerError> {
    let mut errors = admission
        .model_probe_admission
        .as_ref()
        .err()
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    append_wire_validation_errors(&mut errors, admission, context);
    errors.sort_by_key(SingleReviewerError::to_string);
    errors.dedup();
    collapse_admission_errors(errors)
}

fn append_wire_validation_errors(
    errors: &mut Vec<SingleReviewerError>,
    admission: &WireAdmission,
    context: &ReviewerAdmissionContext<'_>,
) {
    if let Err(detail) = validate_claims(
        &context.case.case_id,
        &context.case.claim_slots,
        &admission.probe_answers,
        &admission.claims,
    ) {
        errors.push(SingleReviewerError::InvalidClaims(detail));
    }
    let allowed_probe_ids = admission
        .probe_answers
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if let Err(detail) = validate_recommendations(
        context.case,
        &allowed_probe_ids,
        &admission.claims,
        &admission.recommendations,
    ) {
        errors.push(SingleReviewerError::InvalidRecommendations(detail));
    }
    if let Err(detail) = validate_recommendations_for_conclusion(
        admission.case_conclusion,
        &admission.recommendations,
    ) {
        errors.push(SingleReviewerError::InvalidRecommendations(detail));
    }
    errors.extend(
        validate_position_and_closure(admission.case_position, admission.proposed_closure).err(),
    );
    errors.extend(
        validate_case_conclusion(
            admission.case_conclusion,
            admission.case_position,
            admission.proposed_closure,
            context.case,
        )
        .err(),
    );
    if admission.model_probe_admission.is_ok() {
        errors.extend(
            validate_derived_case_conclusion(
                admission.case_conclusion,
                context,
                &admission.probe_answers,
            )
            .err(),
        );
    }
}

fn collapse_admission_errors(
    mut errors: Vec<SingleReviewerError>,
) -> Result<(), SingleReviewerError> {
    if errors.len() == 1 {
        return Err(errors.pop().expect("one admission error"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SingleReviewerError::InvalidPayload(
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }
}

fn validate_payload(
    payload: &ReviewerCaseReviewPayload,
    context: &ReviewerAdmissionContext<'_>,
) -> Result<BTreeMap<StableId, ProbeAnswer>, SingleReviewerError> {
    validate_probe_ownership(payload, context)?;
    let probe_answers = context
        .deterministic_probe_results
        .iter()
        .chain(&payload.model_resolved_probes)
        .map(|result| (result.probe_id.clone(), result.answer))
        .collect::<BTreeMap<_, _>>();
    let allowed_probe_ids = probe_answers.keys().cloned().collect::<BTreeSet<_>>();
    validate_claims(
        &context.case.case_id,
        &context.case.claim_slots,
        &probe_answers,
        &payload.claims,
    )
    .map_err(SingleReviewerError::InvalidClaims)?;
    validate_recommendations(
        context.case,
        &allowed_probe_ids,
        &payload.claims,
        &payload.recommendations,
    )
    .map_err(SingleReviewerError::InvalidRecommendations)?;
    validate_recommendations_for_conclusion(payload.case_conclusion, &payload.recommendations)
        .map_err(SingleReviewerError::InvalidRecommendations)?;
    validate_preferred_recommendation(payload)?;
    validate_position_and_closure(payload.case_position, payload.proposed_closure)?;
    validate_case_conclusion(
        payload.case_conclusion,
        payload.case_position,
        payload.proposed_closure,
        context.case,
    )?;
    Ok(probe_answers)
}

fn validate_preferred_recommendation(
    payload: &ReviewerCaseReviewPayload,
) -> Result<(), SingleReviewerError> {
    let recommendation_ids = payload
        .recommendations
        .iter()
        .map(|recommendation| &recommendation.recommendation_id)
        .collect::<BTreeSet<_>>();
    if payload
        .preferred_recommendation_id
        .as_ref()
        .is_some_and(|id| !recommendation_ids.contains(id))
    {
        Err(SingleReviewerError::InvalidPreferredRecommendation)
    } else {
        Ok(())
    }
}

fn normalize_payload(payload: &mut ReviewerCaseReviewPayload) {
    payload.unresolved_gaps.retain(|gap| !gap.trim().is_empty());
    payload.unresolved_gaps.sort();
    payload.unresolved_gaps.dedup();
    payload
        .model_resolved_probes
        .sort_by(|left, right| left.probe_id.cmp(&right.probe_id));
    payload
        .claims
        .sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    payload
        .recommendations
        .sort_by(|left, right| left.recommendation_id.cmp(&right.recommendation_id));
}

fn admitted_closure(
    payload: &ReviewerCaseReviewPayload,
    case: &EngineeringCase,
) -> Result<CaseClosure, SingleReviewerError> {
    CaseClosure::terminal(
        case.case_id.clone(),
        payload.proposed_closure,
        payload.unresolved_gaps.clone(),
    )
    .map_err(|_| SingleReviewerError::InvalidClosure)
}

fn review_identity(
    payload: &ReviewerCaseReviewPayload,
    context: &ReviewerAdmissionContext<'_>,
    proposed_closure: &CaseClosure,
) -> Result<StableId, SingleReviewerError> {
    let identity = json!({
        "case_id": context.case.case_id,
        "case_conclusion": payload.case_conclusion,
        "case_position": payload.case_position,
        "claims": payload.claims,
        "finding_ids": context.case.finding_ids,
        "model_resolved_probes": payload.model_resolved_probes,
        "packet_digest": context.packet.packet_digest,
        "preferred_recommendation_id": payload.preferred_recommendation_id,
        "proposed_closure": proposed_closure,
        "recommendations": payload.recommendations,
        "reviewer_run_id": context.reviewer_run_id,
        "route_target": context.route_target,
        "template_version": context.case.template_version,
        "unresolved_gaps": payload.unresolved_gaps,
    });
    StableId::from_identity(
        "reviewer-case-review",
        REVIEWER_CASE_REVIEW_SCHEMA_VERSION,
        &identity,
    )
    .map_err(|error| SingleReviewerError::InvalidIdentity(error.to_string()))
}

fn validate_probe_ownership(
    payload: &ReviewerCaseReviewPayload,
    context: &ReviewerAdmissionContext<'_>,
) -> Result<(), SingleReviewerError> {
    let definitions = validated_probe_definitions(context)?;
    let resolved = validate_deterministic_results(context, &definitions)?;
    let resolved = validate_model_results(payload, context, &definitions, resolved)?;
    if resolved.len() == definitions.len() {
        Ok(())
    } else {
        Err(SingleReviewerError::UnresolvedMandatoryProbe)
    }
}

fn validated_probe_definitions<'a>(
    context: &'a ReviewerAdmissionContext<'_>,
) -> Result<BTreeMap<&'a StableId, &'a ProbeDefinition>, SingleReviewerError> {
    let definitions = context
        .probe_definitions
        .iter()
        .map(|definition| (&definition.probe_id, definition))
        .collect::<BTreeMap<_, _>>();
    let invalid = definitions.len() != context.probe_definitions.len()
        || context
            .probe_definitions
            .iter()
            .any(|definition| definition.validate(context.case, context.packet).is_err());
    if invalid {
        Err(SingleReviewerError::InvalidProbeSet)
    } else {
        Ok(definitions)
    }
}

fn validate_deterministic_results(
    context: &ReviewerAdmissionContext<'_>,
    definitions: &BTreeMap<&StableId, &ProbeDefinition>,
) -> Result<BTreeSet<StableId>, SingleReviewerError> {
    let mut resolved = BTreeSet::new();
    for result in context.deterministic_probe_results {
        let definition = definitions
            .get(&result.probe_id)
            .ok_or(SingleReviewerError::InvalidProbeSet)?;
        let invalid = !resolved.insert(result.probe_id.clone())
            || result.resolver.kind == ResolverKind::ModelInterpretation
            || result.validate(definition, context.packet).is_err();
        if invalid {
            return Err(SingleReviewerError::InvalidProbeSet);
        }
    }
    Ok(resolved)
}

fn validate_model_results(
    payload: &ReviewerCaseReviewPayload,
    context: &ReviewerAdmissionContext<'_>,
    definitions: &BTreeMap<&StableId, &ProbeDefinition>,
    mut resolved: BTreeSet<StableId>,
) -> Result<BTreeSet<StableId>, SingleReviewerError> {
    for result in &payload.model_resolved_probes {
        let definition = definitions.get(&result.probe_id).ok_or_else(|| {
            SingleReviewerError::InvalidModelProbe(format!(
                "{} is not a declared case probe",
                result.probe_id
            ))
        })?;
        if !resolved.insert(result.probe_id.clone()) {
            return Err(SingleReviewerError::InvalidModelProbe(format!(
                "{} repeats a host-resolved or model-resolved probe",
                result.probe_id
            )));
        }
        let invalid = result.resolver.kind != ResolverKind::ModelInterpretation
            || result.reviewer_run_id.as_ref() != Some(context.reviewer_run_id)
            || result.validate(definition, context.packet).is_err();
        if invalid {
            return Err(SingleReviewerError::InvalidModelProbe(format!(
                "{} has invalid resolver ownership or evidence citations",
                result.probe_id
            )));
        }
    }
    Ok(resolved)
}

fn validate_position_and_closure(
    position: CasePosition,
    closure: CaseClosureReason,
) -> Result<(), SingleReviewerError> {
    let compatible = match position {
        CasePosition::Substantiated => matches!(
            closure,
            CaseClosureReason::Resolved | CaseClosureReason::ResolvedWithUncertainty
        ),
        CasePosition::NotSubstantiated => closure == CaseClosureReason::NotSubstantiated,
        CasePosition::EvidenceBlocked => closure == CaseClosureReason::BlockedByMissingEvidence,
        CasePosition::Contested => closure == CaseClosureReason::Contested,
        CasePosition::NotApplicable => closure == CaseClosureReason::NotApplicable,
    };
    compatible
        .then_some(())
        .ok_or(SingleReviewerError::InvalidClosure)
}

fn validate_case_conclusion(
    conclusion: CaseConclusion,
    position: CasePosition,
    closure: CaseClosureReason,
    case: &EngineeringCase,
) -> Result<(), SingleReviewerError> {
    if !case.allowed_conclusions.contains(&conclusion) {
        return Err(SingleReviewerError::InvalidClosure);
    }
    let compatible = match conclusion {
        CaseConclusion::LifecycleMismatchSubstantiated => {
            position == CasePosition::Substantiated
                && matches!(
                    closure,
                    CaseClosureReason::Resolved | CaseClosureReason::ResolvedWithUncertainty
                )
        }
        CaseConclusion::RepetitionJustified
        | CaseConclusion::ImpactImmaterial
        | CaseConclusion::WiderOwnershipInfeasible => {
            position == CasePosition::NotSubstantiated
                && closure == CaseClosureReason::NotSubstantiated
        }
        CaseConclusion::InsufficientEvidence => {
            position == CasePosition::EvidenceBlocked
                && closure == CaseClosureReason::BlockedByMissingEvidence
        }
        CaseConclusion::SharedRootCause
        | CaseConclusion::PartialOverlap
        | CaseConclusion::ConsolidatedAction
        | CaseConclusion::SequencedActions
        | CaseConclusion::PreservedIndependentActions
        | CaseConclusion::NarrowedAction
        | CaseConclusion::DeferredAction => {
            position == CasePosition::Substantiated
                && matches!(
                    closure,
                    CaseClosureReason::Resolved | CaseClosureReason::ResolvedWithUncertainty
                )
        }
        CaseConclusion::IndependentFindings | CaseConclusion::CorrelationOnly => {
            position == CasePosition::NotSubstantiated
                && closure == CaseClosureReason::NotSubstantiated
        }
        CaseConclusion::NoRepositorySpecificAction => {
            position == CasePosition::EvidenceBlocked
                && closure == CaseClosureReason::BlockedByMissingEvidence
        }
    };
    compatible
        .then_some(())
        .ok_or(SingleReviewerError::InvalidClosure)
}

fn validate_derived_case_conclusion(
    conclusion: CaseConclusion,
    context: &ReviewerAdmissionContext<'_>,
    answers_by_probe: &BTreeMap<StableId, ProbeAnswer>,
) -> Result<(), SingleReviewerError> {
    let answers_by_slot = context
        .probe_definitions
        .iter()
        .map(|definition| {
            answers_by_probe
                .get(&definition.probe_id)
                .copied()
                .map(|answer| (definition.slot.clone(), answer))
                .ok_or_else(|| {
                    SingleReviewerError::InvalidCaseConclusion(format!(
                        "probe slot `{}` has no admitted answer",
                        definition.slot
                    ))
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let derived = crate::derive_case_conclusion(context.case.case_kind, &answers_by_slot)
        .map_err(SingleReviewerError::InvalidCaseConclusion)?;
    if conclusion == derived {
        Ok(())
    } else {
        Err(SingleReviewerError::InvalidCaseConclusion(format!(
            "stored conclusion {conclusion:?} does not match probe-derived conclusion {derived:?}"
        )))
    }
}

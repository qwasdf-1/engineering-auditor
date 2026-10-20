use std::collections::BTreeSet;

use super::{
    CaseConclusion, CaseTemplateBundle, CaseTemplateDefinition, InferenceGuardKind, TemplateStatus,
    CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION, CASE_TEMPLATE_SCHEMA_VERSION, CASE_TEMPLATE_VERSION,
};
use crate::{CaseKind, DecisionKind, SelectionReason};

pub(super) fn validate_bundle(bundle: &CaseTemplateBundle) -> Result<(), String> {
    validate_bundle_header(bundle)?;
    validate_entry_order(bundle)?;
    for entry in &bundle.entries {
        validate_entry(entry)?;
    }
    Ok(())
}

fn validate_bundle_header(bundle: &CaseTemplateBundle) -> Result<(), String> {
    if bundle.schema_version != CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION || bundle.bundle_version == 0 {
        return Err("unsupported case template bundle version".to_string());
    }
    if bundle.entries.len() != 3 {
        return Err("case template bundle must own all recognized case kinds".to_string());
    }
    Ok(())
}

fn validate_entry_order(bundle: &CaseTemplateBundle) -> Result<(), String> {
    for pair in bundle.entries.windows(2) {
        if (pair[0].kind, pair[0].version) > (pair[1].kind, pair[1].version) {
            return Err("case template entries must be ordered".to_string());
        }
    }
    Ok(())
}

fn validate_entry(entry: &super::CaseTemplateEntry) -> Result<(), String> {
    if entry.version == 0 {
        return Err("case template entry requires a version".to_string());
    }
    match (entry.status, &entry.template) {
        (TemplateStatus::Production | TemplateStatus::Shadow, Some(template)) => {
            if entry.kind != template.kind || entry.version != template.version {
                return Err("template entry identity mismatch".to_string());
            }
            template.validate()
        }
        (TemplateStatus::NotEnabled, None) => Ok(()),
        _ => Err("template status and definition are inconsistent".to_string()),
    }
}

pub(super) fn validate_template(template: &CaseTemplateDefinition) -> Result<(), String> {
    if template.schema_version != CASE_TEMPLATE_SCHEMA_VERSION
        || template.version != CASE_TEMPLATE_VERSION
    {
        return Err("unsupported case template identity".to_string());
    }
    validate_trigger(template)?;
    validate_subject_policy(template)?;
    validate_evidence_contract(template)?;
    validate_probe_policy(template)?;
    validate_case_policies(template)
}

fn validate_trigger(template: &CaseTemplateDefinition) -> Result<(), String> {
    let valid = match template.kind {
        CaseKind::LifecycleMismatch => {
            template.trigger.decision_kind == DecisionKind::EvaluateLifecycleOwnership
                && template.trigger.selection_reasons_any
                    == [SelectionReason::RepeatedWorkLifecycleIntersection]
        }
        CaseKind::SharedRootCause => {
            template.trigger.decision_kind == DecisionKind::EvaluateSharedRootCause
                && template.trigger.selection_reasons_any
                    == [
                        SelectionReason::SameSubjectConcentration,
                        SelectionReason::CrossLayerIntersection,
                    ]
        }
        CaseKind::RecommendationConsolidation => {
            template.trigger.decision_kind == DecisionKind::ReconcileRecommendations
                && template.trigger.selection_reasons_any
                    == [
                        SelectionReason::RecommendationOverlap,
                        SelectionReason::RecommendationConflict,
                    ]
        }
    };
    valid
        .then_some(())
        .ok_or_else(|| "template trigger is not canonical".to_string())
}

fn validate_subject_policy(template: &CaseTemplateDefinition) -> Result<(), String> {
    if template.subject_policy.minimum_subjects != 1
        || !template.subject_policy.require_connected
        || !template.subject_policy.allow_multiple_findings
    {
        return Err("template subject policy is not canonical".to_string());
    }
    Ok(())
}

fn validate_evidence_contract(template: &CaseTemplateDefinition) -> Result<(), String> {
    let requirements = template
        .evidence_contract
        .required
        .iter()
        .chain(&template.evidence_contract.optional)
        .chain(&template.evidence_contract.blocking)
        .collect::<Vec<_>>();
    let mut ids = BTreeSet::new();
    if requirements.is_empty()
        || requirements.iter().any(|requirement| {
            requirement.id.is_empty()
                || requirement.description.trim().is_empty()
                || !ids.insert(requirement.id.as_str())
        })
    {
        return Err("evidence requirements must be unique and nonempty".to_string());
    }
    Ok(())
}

fn validate_probe_policy(template: &CaseTemplateDefinition) -> Result<(), String> {
    let expected_slots: &[&str] = match template.kind {
        CaseKind::LifecycleMismatch => &[
            "repetition",
            "input_equivalence",
            "intervening_mutation",
            "material_cost",
            "consumer_distinction",
            "wider_owner",
            "invalidation_feasibility",
            "retention_risk",
        ],
        CaseKind::SharedRootCause => &[
            "same_responsibility",
            "shared_change_impact",
            "mere_correlation",
            "recommendation_conflict",
            "coupling_increase",
            "distinct_semantics",
            "independent_actionability",
        ],
        CaseKind::RecommendationConsolidation => &[
            "simultaneous_compatibility",
            "abstraction_alignment",
            "supersession",
            "measured_property_regression",
            "required_ordering",
            "proportionality",
            "deferral_appropriateness",
        ],
    };
    if template
        .mandatory_probes
        .iter()
        .map(|probe| probe.slot.as_str())
        .collect::<Vec<_>>()
        != expected_slots
    {
        return Err("mandatory probe slots are not canonical".to_string());
    }
    if template.mandatory_probes.iter().any(|probe| {
        probe.question.trim().is_empty()
            || probe.positive_boundary.trim().is_empty()
            || probe.negative_boundary.trim().is_empty()
            || probe.required_evidence_classes.is_empty()
            || probe.required_evidence_classes.iter().any(|class| {
                !crate::case_review::probe::evidence_class_compatible(probe.kind, *class)
            })
            || probe.blocked_claim_slots.is_empty()
    }) {
        return Err("mandatory probe policy is incomplete".to_string());
    }
    let optional = &template.optional_probe_policy;
    if optional.allowed_kinds.is_empty()
        || optional.maximum_count == 0
        || !optional.require_named_decision
        || !optional.require_case_boundary
    {
        return Err("optional probe policy is incomplete".to_string());
    }
    Ok(())
}

fn validate_case_policies(template: &CaseTemplateDefinition) -> Result<(), String> {
    let expected_guards = match template.kind {
        CaseKind::LifecycleMismatch => BTreeSet::from([
            InferenceGuardKind::RelevantMutation,
            InferenceGuardKind::DistinctConfiguration,
            InferenceGuardKind::TestOnlyRepetition,
            InferenceGuardKind::FreshnessConstraint,
            InferenceGuardKind::RetentionRisk,
            InferenceGuardKind::QuantitativeBenefit,
        ]),
        CaseKind::SharedRootCause => BTreeSet::from([
            InferenceGuardKind::CorrelationNotCausation,
            InferenceGuardKind::CouplingIncrease,
            InferenceGuardKind::DistinctSemantics,
            InferenceGuardKind::IndependentActionability,
        ]),
        CaseKind::RecommendationConsolidation => BTreeSet::from([
            InferenceGuardKind::ActionCompatibility,
            InferenceGuardKind::AbstractionMismatch,
            InferenceGuardKind::PropertyRegression,
            InferenceGuardKind::DisproportionateChange,
        ]),
    };
    let actual_guards = template
        .inference_guards
        .iter()
        .map(|guard| guard.kind)
        .collect::<BTreeSet<_>>();
    if actual_guards != expected_guards
        || template.inference_guards.iter().any(|guard| {
            guard.probe_slots.is_empty()
                || (guard.blocked_claim_slots.is_empty() && guard.blocked_conclusions.is_empty())
        })
    {
        return Err("template inference guards are incomplete".to_string());
    }
    let expected_conclusions: &[CaseConclusion] = match template.kind {
        CaseKind::LifecycleMismatch => &[
            CaseConclusion::LifecycleMismatchSubstantiated,
            CaseConclusion::RepetitionJustified,
            CaseConclusion::ImpactImmaterial,
            CaseConclusion::WiderOwnershipInfeasible,
            CaseConclusion::InsufficientEvidence,
        ],
        CaseKind::SharedRootCause => &[
            CaseConclusion::SharedRootCause,
            CaseConclusion::PartialOverlap,
            CaseConclusion::IndependentFindings,
            CaseConclusion::CorrelationOnly,
            CaseConclusion::InsufficientEvidence,
        ],
        CaseKind::RecommendationConsolidation => &[
            CaseConclusion::ConsolidatedAction,
            CaseConclusion::SequencedActions,
            CaseConclusion::PreservedIndependentActions,
            CaseConclusion::NarrowedAction,
            CaseConclusion::DeferredAction,
            CaseConclusion::NoRepositorySpecificAction,
        ],
    };
    let expected_insufficient = match template.kind {
        CaseKind::RecommendationConsolidation => CaseConclusion::NoRepositorySpecificAction,
        _ => CaseConclusion::InsufficientEvidence,
    };
    if template.closure_policy.allowed_conclusions != expected_conclusions
        || template.closure_policy.insufficient_evidence_conclusion != expected_insufficient
        || template.claim_slots.is_empty()
        || template.recommendation_families.is_empty()
        || template.route_policy.ensemble_required_when.is_empty()
        || template.challenge_policy.mandatory_when.is_empty()
    {
        return Err("template conclusion or routing policy is incomplete".to_string());
    }
    Ok(())
}

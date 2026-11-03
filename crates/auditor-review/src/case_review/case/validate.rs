use std::collections::BTreeSet;

use super::{
    CaseTemplateBundle, CaseTemplateDefinition, TemplateStatus,
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
    validate_evidence_contract(template)
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

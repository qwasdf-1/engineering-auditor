use crate::{CaseTemplateBundle, EngineeringCase, ReviewInput, ENGINEERING_CASE_SCHEMA_VERSION};

use super::EvidenceError;

pub(super) fn validate_case(
    case: &EngineeringCase,
    input: &ReviewInput,
) -> Result<(), EvidenceError> {
    if !binding_matches(case, input) {
        return Err(EvidenceError::CaseBindingMismatch);
    }
    let bundle = CaseTemplateBundle::from_value(input.case_template_bundle().semantic_snapshot())
        .map_err(EvidenceError::InvalidPacket)?;
    let template = bundle
        .entry(case.case_kind)
        .and_then(|entry| entry.template.as_ref())
        .filter(|template| template.version == case.template_version)
        .ok_or_else(|| {
            EvidenceError::InvalidPacket(
                "engineering case has no enabled bound template".to_string(),
            )
        })?;
    if template_matches(case, template) {
        Ok(())
    } else {
        Err(EvidenceError::InvalidPacket(
            "engineering case does not match its bound template".to_string(),
        ))
    }
}

fn binding_matches(case: &EngineeringCase, input: &ReviewInput) -> bool {
    case.schema_version == ENGINEERING_CASE_SCHEMA_VERSION
        && case.input_binding_id == input.binding().stable_id()
        && case.template_bundle_digest == input.binding().case_template_bundle_digest
        && !case.finding_ids.is_empty()
        && case
            .finding_ids
            .iter()
            .all(|finding_id| input.finding(finding_id).is_some())
}

fn template_matches(case: &EngineeringCase, template: &crate::CaseTemplateDefinition) -> bool {
    let probes_match = case.mandatory_probes.len() == template.mandatory_probes.len()
        && case
            .mandatory_probes
            .iter()
            .zip(&template.mandatory_probes)
            .all(|(probe, expected)| probe.slot == expected.slot && probe.kind == expected.kind);
    probes_match
        && case.evidence_contract == template.evidence_contract
        && case.allowed_conclusions == template.closure_policy.allowed_conclusions
        && case.claim_slots == template.claim_slots
        && case.inference_guards == template.inference_guards
        && case.recommendation_families == template.recommendation_families
        && case.route_policy == template.route_policy
        && case.challenge_policy == template.challenge_policy
        && case.closure_policy == template.closure_policy
}

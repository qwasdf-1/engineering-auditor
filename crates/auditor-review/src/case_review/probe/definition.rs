use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::result::{ProbeAnswer, ResolverKind};
use super::validate::{
    validate_definition, validate_optional_request, OptionalProbeValidationScope,
};
use super::ProbeError;
use crate::{
    CaseKind, CaseTemplateDefinition, ClaimSlot, ContentDigest, EngineeringCase, EvidenceClass,
    EvidenceItemKind, EvidenceLookupRequest, EvidenceLookupRequestKind, EvidencePacket, ProbeKind,
    RecommendationFamily, StableId,
};

pub const PROBE_DEFINITION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOrigin {
    Mandatory,
    Optional,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProbeDecisionTarget {
    Claim(ClaimSlot),
    Recommendation(RecommendationFamily),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalProbeRequest {
    pub slot: String,
    pub kind: ProbeKind,
    pub question: String,
    pub required_evidence_classes: Vec<EvidenceClass>,
    pub positive_boundary: String,
    pub negative_boundary: String,
    pub blocked_claim_slots: Vec<ClaimSlot>,
    pub target_ids: Vec<String>,
    pub decision_target: ProbeDecisionTarget,
    pub decision_relevance: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeDefinition {
    pub schema_version: u32,
    pub probe_id: StableId,
    pub case_id: StableId,
    pub case_kind: CaseKind,
    pub template_version: u32,
    pub template_bundle_digest: ContentDigest,
    pub origin: ProbeOrigin,
    pub slot: String,
    pub kind: ProbeKind,
    pub question: String,
    pub allowed_results: Vec<ProbeAnswer>,
    pub resolver_precedence: Vec<ResolverKind>,
    pub required_evidence_classes: Vec<EvidenceClass>,
    pub positive_boundary: String,
    pub negative_boundary: String,
    pub blocked_claim_slots: Vec<ClaimSlot>,
    pub target_ids: Vec<String>,
    pub decision_target: Option<ProbeDecisionTarget>,
    pub decision_relevance: String,
}

impl ProbeDefinition {
    pub fn validate(
        &self,
        case: &EngineeringCase,
        packet: &EvidencePacket,
    ) -> Result<(), ProbeError> {
        validate_definition(self, case, packet)
    }
}

pub struct ProbePlanner;

impl ProbePlanner {
    pub fn plan(
        &self,
        case: &EngineeringCase,
        template: &CaseTemplateDefinition,
        packet: &EvidencePacket,
        optional: &[OptionalProbeRequest],
    ) -> Result<Vec<ProbeDefinition>, ProbeError> {
        validate_plan_inputs(case, template, packet)?;
        if optional.len() > template.optional_probe_policy.maximum_count {
            return Err(ProbeError::InvalidOptionalProbe(
                "optional probe count exceeds the template ceiling".to_string(),
            ));
        }
        let mut definitions = template
            .mandatory_probes
            .iter()
            .map(|probe| {
                let probe_ref = case
                    .mandatory_probes
                    .iter()
                    .find(|candidate| candidate.slot == probe.slot)
                    .ok_or_else(|| {
                        ProbeError::InvalidDefinition(format!(
                            "case is missing mandatory slot {}",
                            probe.slot
                        ))
                    })?;
                Ok(ProbeDefinition {
                    schema_version: PROBE_DEFINITION_SCHEMA_VERSION,
                    probe_id: probe_ref.probe_id.clone(),
                    case_id: case.case_id.clone(),
                    case_kind: case.case_kind,
                    template_version: case.template_version,
                    template_bundle_digest: case.template_bundle_digest.clone(),
                    origin: ProbeOrigin::Mandatory,
                    slot: probe.slot.clone(),
                    kind: probe.kind,
                    question: probe.question.clone(),
                    allowed_results: ProbeAnswer::ALL.to_vec(),
                    resolver_precedence: ResolverKind::PRECEDENCE.to_vec(),
                    required_evidence_classes: probe.required_evidence_classes.clone(),
                    positive_boundary: probe.positive_boundary.clone(),
                    negative_boundary: probe.negative_boundary.clone(),
                    blocked_claim_slots: probe.blocked_claim_slots.clone(),
                    target_ids: mandatory_targets(case),
                    decision_target: None,
                    decision_relevance: "template_mandatory".to_string(),
                })
            })
            .collect::<Result<Vec<_>, ProbeError>>()?;
        let mandatory_slots = definitions
            .iter()
            .map(|definition| definition.slot.clone())
            .collect::<BTreeSet<_>>();
        let mut optional = optional.to_vec();
        optional.sort_by(|left, right| left.slot.cmp(&right.slot));
        let mut optional_slots = BTreeSet::new();
        for mut request in optional {
            request.required_evidence_classes.sort();
            request.required_evidence_classes.dedup();
            request.blocked_claim_slots.sort();
            request.blocked_claim_slots.dedup();
            request.target_ids.sort();
            request.target_ids.dedup();
            validate_optional_request(
                &request,
                OptionalProbeValidationScope {
                    case,
                    template,
                    packet,
                    mandatory_slots: &mandatory_slots,
                },
                &mut optional_slots,
            )?;
            definitions.push(ProbeDefinition {
                schema_version: PROBE_DEFINITION_SCHEMA_VERSION,
                probe_id: optional_probe_id(&case.case_id, &request.slot)?,
                case_id: case.case_id.clone(),
                case_kind: case.case_kind,
                template_version: case.template_version,
                template_bundle_digest: case.template_bundle_digest.clone(),
                origin: ProbeOrigin::Optional,
                slot: request.slot,
                kind: request.kind,
                question: request.question,
                allowed_results: ProbeAnswer::ALL.to_vec(),
                resolver_precedence: ResolverKind::PRECEDENCE.to_vec(),
                required_evidence_classes: request.required_evidence_classes,
                positive_boundary: request.positive_boundary,
                negative_boundary: request.negative_boundary,
                blocked_claim_slots: request.blocked_claim_slots,
                target_ids: request.target_ids,
                decision_target: Some(request.decision_target),
                decision_relevance: request.decision_relevance,
            });
        }
        for definition in &definitions {
            definition.validate(case, packet)?;
        }
        Ok(definitions)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeLookupPlan {
    pub requests: Vec<EvidenceLookupRequest>,
    pub unscheduled_probe_ids: Vec<StableId>,
}

pub struct ProbeLookupPlanner;

impl ProbeLookupPlanner {
    pub fn plan(
        &self,
        case: &EngineeringCase,
        definitions: &[ProbeDefinition],
        packet: &EvidencePacket,
    ) -> Result<ProbeLookupPlan, ProbeError> {
        for definition in definitions {
            definition.validate(case, packet)?;
        }
        if packet
            .items
            .iter()
            .any(|item| item.kind == EvidenceItemKind::SourceSpan)
        {
            return Ok(ProbeLookupPlan {
                requests: Vec::new(),
                unscheduled_probe_ids: Vec::new(),
            });
        }
        let maximum = packet
            .budget
            .maximum_lookup_requests
            .min(packet.budget.initial_source_excerpts);
        let mut requests = Vec::new();
        let mut unscheduled_probe_ids = Vec::new();
        for definition in definitions.iter().filter(|definition| {
            definition
                .required_evidence_classes
                .contains(&EvidenceClass::SourceSpan)
        }) {
            let Some((kind, target_id)) = lookup_target(definition, packet) else {
                unscheduled_probe_ids.push(definition.probe_id.clone());
                continue;
            };
            if requests.len() == maximum {
                unscheduled_probe_ids.push(definition.probe_id.clone());
                continue;
            }
            requests.push(EvidenceLookupRequest {
                case_id: case.case_id.clone(),
                probe_id: definition.probe_id.clone(),
                kind,
                target_id,
                decision_relevance: format!(
                    "probe {}: {}",
                    definition.slot, definition.decision_relevance
                ),
                hops: 0,
            });
        }
        Ok(ProbeLookupPlan {
            requests,
            unscheduled_probe_ids,
        })
    }
}

fn lookup_target(
    definition: &ProbeDefinition,
    packet: &EvidencePacket,
) -> Option<(EvidenceLookupRequestKind, String)> {
    definition
        .target_ids
        .iter()
        .find(|target| packet.allowed_neighborhood.map_symbol_ids.contains(*target))
        .or_else(|| packet.allowed_neighborhood.map_symbol_ids.first())
        .cloned()
        .map(|target| (EvidenceLookupRequestKind::SymbolNeighborhood, target))
        .or_else(|| {
            definition
                .target_ids
                .iter()
                .find(|target| target.starts_with("finding:"))
                .cloned()
                .map(|target| (EvidenceLookupRequestKind::FindingSource, target))
        })
}

fn validate_plan_inputs(
    case: &EngineeringCase,
    template: &CaseTemplateDefinition,
    packet: &EvidencePacket,
) -> Result<(), ProbeError> {
    packet
        .validate()
        .map_err(|error| ProbeError::InvalidDefinition(error.to_string()))?;
    if packet.case_id != case.case_id
        || packet.input_binding_id != case.input_binding_id
        || template.kind != case.case_kind
        || template.version != case.template_version
        || template.evidence_contract != case.evidence_contract
    {
        return Err(ProbeError::InvalidDefinition(
            "case, template, and packet identities do not match".to_string(),
        ));
    }
    Ok(())
}

fn mandatory_targets(case: &EngineeringCase) -> Vec<String> {
    let mut targets = case
        .subject_ids
        .iter()
        .cloned()
        .chain(case.finding_ids.iter().map(|id| format!("finding:{id}")))
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets
}

pub(super) fn optional_probe_id(case_id: &StableId, slot: &str) -> Result<StableId, ProbeError> {
    StableId::from_identity(
        "case-probe",
        PROBE_DEFINITION_SCHEMA_VERSION,
        &json!({"case_id": case_id, "slot": slot}),
    )
    .map_err(|error| ProbeError::InvalidOptionalProbe(error.to_string()))
}

use std::collections::BTreeMap;

use auditor_core::sha256_digest_bytes;
use serde::Serialize;
use serde_json::{Map, Value};

use super::reviewer_contract::{
    challenger_output_admission_instruction, common_output_admission_instruction,
    lookup_instruction, probe_evidence_instruction, provider_claim_kinds,
    provider_recommendation_family, reviewer_output_admission_instruction,
};

use crate::case_review::probe::{admissible_evidence_ids, evidence_classes_for_item};
use crate::{
    model_value_from_json, render_model_document, CaseEvidenceRequest, CaseKind, ClaimSlot,
    DecisionKind, EngineeringCase, EngineeringClaimKind, EvidenceClass, EvidenceClosure,
    EvidenceItem, EvidenceItemKind, EvidenceLookupRequestKind, EvidenceOmission,
    EvidenceOmissionReason, EvidencePacket, ModelDocument, ModelField, ModelProjection,
    ModelSection, ModelValue, OperatorCaseConstraints, ProbeAnswer, ProbeDefinition, ProbeKind,
    ProbeResult, RecommendationFamily, ReviewPromptBundleV2, StableId, TrustLabel,
};

const MODEL_PROJECTION_EXCLUDED_FIELDS: [&str; 8] = [
    "evidence.items[].content.evidence_keys",
    "evidence.items[].content.id",
    "evidence.items[].content.identity",
    "evidence.items[].content.kind",
    "evidence.items[].content.provenance.analysis_method",
    "evidence.items[].content.provenance.extractor_version",
    "evidence.items[].content.provenance.language",
    "evidence.items[].content.provenance.origin",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseProviderContext {
    pub case_id: StableId,
    pub decision: CaseDecisionContext,
    pub model_probe_contracts: Vec<ProviderProbeDefinition>,
    pub host_resolved_probes: Vec<ProviderProbeResult>,
    pub evidence: ProviderEvidencePacket,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseDecisionContext {
    pub case_kind: CaseKind,
    pub decision_kind: DecisionKind,
    pub conclusion_derivation_policy: &'static str,
    pub subject_ids: Vec<String>,
    pub recommendation_scope_ids: Vec<String>,
    pub claim_slots: Vec<ClaimSlot>,
    pub claim_kind_contracts: BTreeMap<EngineeringClaimKind, &'static str>,
    pub recommendation_families: Vec<ProviderRecommendationFamily>,
    pub operator_constraints: OperatorCaseConstraints,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRecommendationFamily {
    pub family: RecommendationFamily,
    pub use_when: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEvidencePacket {
    pub items: Vec<ProviderEvidenceItem>,
    pub lookup_authority: ProviderLookupAuthority,
    pub closure: EvidenceClosure,
    pub omissions: Vec<EvidenceOmission>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLookupAuthority {
    pub finding_source_target_ids: Vec<String>,
    pub symbol_neighborhood_target_ids: Vec<String>,
    pub maximum_graph_hops: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEvidenceItem {
    pub id: String,
    pub equivalent_ids: Vec<String>,
    pub kind: EvidenceItemKind,
    pub trust: TrustLabel,
    pub evidence_classes: Vec<crate::EvidenceClass>,
    pub content: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProbeResult {
    pub probe_id: StableId,
    pub answer: ProbeAnswer,
    pub evidence_ids: Vec<String>,
    pub explanation: String,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProbeDefinition {
    pub probe_id: StableId,
    pub slot: String,
    pub kind: ProbeKind,
    pub question: String,
    pub positive_boundary: String,
    pub negative_boundary: String,
    pub blocked_claim_slots: Vec<ClaimSlot>,
    pub admissible_evidence_ids: Vec<String>,
    pub decision_target: Option<crate::ProbeDecisionTarget>,
    pub decision_relevance: String,
}

impl CaseProviderContext {
    pub fn compile(
        case: &EngineeringCase,
        packet: &EvidencePacket,
        deterministic_probe_results: &[ProbeResult],
        unresolved_probes: &[ProbeDefinition],
    ) -> Result<Self, String> {
        if packet.case_id != case.case_id
            || deterministic_probe_results
                .iter()
                .any(|result| result.case_id != case.case_id)
            || unresolved_probes
                .iter()
                .any(|probe| probe.case_id != case.case_id)
        {
            return Err("case provider projection has mismatched case identity".to_string());
        }
        let mut host_resolved_probes = deterministic_probe_results
            .iter()
            .map(|result| ProviderProbeResult {
                probe_id: result.probe_id.clone(),
                answer: result.answer,
                evidence_ids: result.evidence_ids.clone(),
                explanation: result.explanation.clone(),
                limitations: result.limitations.clone(),
            })
            .collect::<Vec<_>>();
        host_resolved_probes.sort_by(|left, right| left.probe_id.cmp(&right.probe_id));
        let mut model_probe_contracts = unresolved_probes
            .iter()
            .map(|probe| ProviderProbeDefinition {
                probe_id: probe.probe_id.clone(),
                slot: probe.slot.clone(),
                kind: probe.kind,
                question: probe.question.clone(),
                positive_boundary: probe.positive_boundary.clone(),
                negative_boundary: probe.negative_boundary.clone(),
                blocked_claim_slots: probe
                    .blocked_claim_slots
                    .iter()
                    .copied()
                    .filter(|slot| *slot != ClaimSlot::LifecycleConclusion)
                    .collect(),
                admissible_evidence_ids: preferred_admissible_evidence_ids(probe, packet),
                decision_target: probe.decision_target,
                decision_relevance: probe.decision_relevance.clone(),
            })
            .collect::<Vec<_>>();
        model_probe_contracts.sort_by(|left, right| left.probe_id.cmp(&right.probe_id));
        let mut recommendation_scope_ids = case.subject_ids.clone();
        recommendation_scope_ids.extend(case.finding_ids.iter().cloned());
        recommendation_scope_ids.sort();
        recommendation_scope_ids.dedup();
        let evidence_items = projected_provider_evidence_items(
            packet,
            &model_probe_contracts,
            &host_resolved_probes,
        );
        Ok(Self {
            case_id: case.case_id.clone(),
            decision: CaseDecisionContext {
                case_kind: case.case_kind,
                decision_kind: case.decision_kind,
                conclusion_derivation_policy: crate::case_review::conclusion_derivation_policy(
                    case.case_kind,
                ),
                subject_ids: case.subject_ids.clone(),
                recommendation_scope_ids,
                claim_slots: case
                    .claim_slots
                    .iter()
                    .copied()
                    .filter(|slot| *slot != ClaimSlot::LifecycleConclusion)
                    .collect(),
                claim_kind_contracts: provider_claim_kinds(),
                recommendation_families: case
                    .recommendation_families
                    .iter()
                    .copied()
                    .map(provider_recommendation_family)
                    .collect(),
                operator_constraints: case.operator_constraints.clone(),
            },
            model_probe_contracts,
            host_resolved_probes,
            evidence: ProviderEvidencePacket {
                items: evidence_items,
                lookup_authority: ProviderLookupAuthority {
                    finding_source_target_ids: available_finding_source_target_ids(packet),
                    symbol_neighborhood_target_ids: packet
                        .allowed_neighborhood
                        .map_symbol_ids
                        .iter()
                        .filter(|id| symbol_source_items(&packet.items, id).is_empty())
                        .cloned()
                        .collect(),
                    maximum_graph_hops: packet.allowed_neighborhood.maximum_graph_hops,
                },
                closure: packet.closure.clone(),
                omissions: packet
                    .omissions
                    .iter()
                    .filter(|omission| omission.reason != EvidenceOmissionReason::Unavailable)
                    .cloned()
                    .collect(),
            },
        })
    }

    pub fn reviewer_instruction(bundle: &ReviewPromptBundleV2) -> String {
        [
            bundle.core_instruction.as_str(),
            bundle.reviewer_instruction.as_str(),
            bundle.bounded_response_instruction.as_str(),
            lookup_instruction(),
            probe_evidence_instruction(),
            common_output_admission_instruction(),
            reviewer_output_admission_instruction(),
        ]
        .join("\n\n")
    }

    pub fn render_markdown(&self, title: &str) -> Result<ModelProjection, String> {
        let source = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        let source_digest = sha256_digest_bytes(&source);
        let decision = serde_json::to_value(&self.decision).map_err(|error| error.to_string())?;
        let model_probes =
            serde_json::to_value(&self.model_probe_contracts).map_err(|error| error.to_string())?;
        let host_probes =
            serde_json::to_value(&self.host_resolved_probes).map_err(|error| error.to_string())?;
        let lookup = serde_json::to_value(&self.evidence.lookup_authority)
            .map_err(|error| error.to_string())?;
        let closure =
            serde_json::to_value(&self.evidence.closure).map_err(|error| error.to_string())?;
        let omissions =
            serde_json::to_value(&self.evidence.omissions).map_err(|error| error.to_string())?;
        let evidence_items = self
            .evidence
            .items
            .iter()
            .map(provider_evidence_item_value)
            .collect::<Result<Vec<_>, _>>()?;
        let mut evidence_fields = vec![
            ModelField::new("Items", ModelValue::Items(evidence_items)),
            ModelField::new("Closure", model_value_from_json(&closure)),
            ModelField::new("Omissions", model_value_from_json(&omissions)),
        ];
        if !self
            .evidence
            .lookup_authority
            .finding_source_target_ids
            .is_empty()
            || !self
                .evidence
                .lookup_authority
                .symbol_neighborhood_target_ids
                .is_empty()
        {
            evidence_fields.insert(
                1,
                ModelField::new("Lookup authority", model_value_from_json(&lookup)),
            );
        }
        let document = ModelDocument::new(
            title,
            vec![
                ModelSection::new(
                    "Case",
                    vec![ModelField::new(
                        "Case ID",
                        ModelValue::text(self.case_id.to_string()),
                    )],
                ),
                ModelSection::new(
                    "Decision contract",
                    vec![ModelField::new("Fields", model_value_from_json(&decision))],
                ),
                ModelSection::new(
                    "Case probe contracts",
                    vec![ModelField::new(
                        "Model probe contracts",
                        model_value_from_json(&model_probes),
                    )],
                ),
                ModelSection::new(
                    "Host-resolved probes",
                    vec![ModelField::new(
                        "Results",
                        model_value_from_json(&host_probes),
                    )],
                ),
                ModelSection::new("Evidence", evidence_fields),
            ],
        );
        render_model_document(&document, source_digest, MODEL_PROJECTION_EXCLUDED_FIELDS)
            .map_err(|error| error.to_string())
    }

    pub fn challenger_instruction(bundle: &ReviewPromptBundleV2) -> String {
        [
            bundle.core_instruction.as_str(),
            bundle.challenger_instruction.as_str(),
            bundle.bounded_response_instruction.as_str(),
            lookup_instruction(),
            common_output_admission_instruction(),
            challenger_output_admission_instruction(),
        ]
        .join("\n\n")
    }
}

pub(crate) fn render_evidence_items_markdown(
    title: &str,
    items: &[crate::EvidenceItem],
) -> Result<ModelProjection, String> {
    let source = serde_json::to_vec(items).map_err(|error| error.to_string())?;
    let projected = items
        .iter()
        .map(|item| {
            provider_evidence_item_value(&ProviderEvidenceItem {
                id: item.id.clone(),
                equivalent_ids: Vec::new(),
                kind: item.kind,
                trust: item.trust,
                evidence_classes: evidence_classes_for_item(item),
                content: item.content.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    render_model_document(
        &ModelDocument::new(
            title,
            vec![ModelSection::new(
                "Returned evidence",
                vec![ModelField::new("Items", ModelValue::Items(projected))],
            )],
        ),
        sha256_digest_bytes(&source),
        MODEL_PROJECTION_EXCLUDED_FIELDS,
    )
    .map_err(|error| error.to_string())
}

fn preferred_admissible_evidence_ids(
    probe: &ProbeDefinition,
    packet: &EvidencePacket,
) -> Vec<String> {
    if probe.kind == ProbeKind::Cardinality {
        let mut ids = packet
            .items
            .iter()
            .filter(|item| {
                model_relevant_evidence_item(item)
                    && matches!(
                        item.kind,
                        EvidenceItemKind::SourceSpan
                            | EvidenceItemKind::Test
                            | EvidenceItemKind::Finding
                    )
            })
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        return ids;
    }
    for class in evidence_class_priority(probe.kind) {
        if !probe.required_evidence_classes.contains(class) {
            continue;
        }
        let mut candidates = packet
            .items
            .iter()
            .filter(|item| {
                model_relevant_evidence_item(item)
                    && evidence_classes_for_item(item).contains(class)
            })
            .collect::<Vec<_>>();
        if *class == EvidenceClass::EvidenceGraph {
            let decision_nodes = candidates
                .iter()
                .copied()
                .filter(|item| {
                    matches!(
                        item.kind,
                        EvidenceItemKind::Measurement | EvidenceItemKind::Symbol
                    )
                })
                .collect::<Vec<_>>();
            if !decision_nodes.is_empty() {
                candidates = decision_nodes;
            }
        }
        if !candidates.is_empty() {
            return candidates.iter().map(|item| item.id.clone()).collect();
        }
    }
    model_relevant_admissible_evidence_ids(probe, packet)
}

fn model_relevant_admissible_evidence_ids(
    probe: &ProbeDefinition,
    packet: &EvidencePacket,
) -> Vec<String> {
    admissible_evidence_ids(probe, packet)
        .into_iter()
        .filter(|id| {
            packet
                .items
                .iter()
                .find(|item| item.id == *id)
                .is_some_and(model_relevant_evidence_item)
        })
        .collect()
}

fn evidence_class_priority(kind: ProbeKind) -> &'static [EvidenceClass] {
    use EvidenceClass::{
        Capability, ConsumerRequirement, EvidenceGraph, Finding, Mutation, OperatorConstraint,
        RepositoryMap, RetentionConstraint, RuntimeMeasurement, SourceRole, SourceSpan, Test,
    };
    const PRIORITIES: &[(ProbeKind, &[EvidenceClass])] = &[
        (
            ProbeKind::Existence,
            &[Finding, SourceSpan, RepositoryMap, EvidenceGraph],
        ),
        (ProbeKind::Identity, &[SourceSpan, Finding, EvidenceGraph]),
        (
            ProbeKind::SemanticEquivalence,
            &[SourceSpan, Finding, Mutation, ConsumerRequirement],
        ),
        (
            ProbeKind::Ordering,
            &[SourceSpan, RepositoryMap, EvidenceGraph],
        ),
        (ProbeKind::Mutation, &[Mutation, SourceSpan, EvidenceGraph]),
        (
            ProbeKind::Cost,
            &[RuntimeMeasurement, OperatorConstraint, Finding, SourceSpan],
        ),
        (
            ProbeKind::Cardinality,
            &[Finding, SourceSpan, SourceRole, Test, EvidenceGraph],
        ),
        (
            ProbeKind::Lifecycle,
            &[SourceSpan, RepositoryMap, EvidenceGraph],
        ),
        (
            ProbeKind::ConsumerRequirement,
            &[ConsumerRequirement, SourceSpan, EvidenceGraph],
        ),
        (
            ProbeKind::Constraint,
            &[RetentionConstraint, OperatorConstraint, SourceSpan],
        ),
        (
            ProbeKind::Counterexample,
            &[
                Mutation,
                ConsumerRequirement,
                SourceSpan,
                Finding,
                Test,
                EvidenceGraph,
            ],
        ),
        (
            ProbeKind::AlternativeFeasibility,
            &[
                OperatorConstraint,
                SourceSpan,
                RepositoryMap,
                Mutation,
                ConsumerRequirement,
            ],
        ),
        (
            ProbeKind::VerificationAvailability,
            &[
                Test,
                Capability,
                RuntimeMeasurement,
                OperatorConstraint,
                RepositoryMap,
            ],
        ),
    ];
    PRIORITIES
        .iter()
        .find_map(|(candidate, classes)| (*candidate == kind).then_some(*classes))
        .unwrap_or(&[])
}

fn projected_provider_evidence_items(
    packet: &EvidencePacket,
    model_probes: &[ProviderProbeDefinition],
    host_probes: &[ProviderProbeResult],
) -> Vec<ProviderEvidenceItem> {
    let required_ids = model_probes
        .iter()
        .flat_map(|probe| &probe.admissible_evidence_ids)
        .chain(host_probes.iter().flat_map(|probe| &probe.evidence_ids))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut projected = Vec::<ProviderEvidenceItem>::new();
    for item in packet.items.iter().filter(|item| {
        model_relevant_evidence_item(item)
            && (required_ids.contains(&item.id)
                || matches!(
                    item.kind,
                    EvidenceItemKind::Finding
                        | EvidenceItemKind::SourceSpan
                        | EvidenceItemKind::BriefConstraint
                        | EvidenceItemKind::Test
                ))
    }) {
        let evidence_classes = evidence_classes_for_item(item);
        if item.kind == EvidenceItemKind::SourceSpan {
            if let Some(existing) = projected.iter_mut().find(|existing| {
                existing.kind == item.kind
                    && existing.trust == item.trust
                    && existing.evidence_classes == evidence_classes
                    && existing.content == item.content
            }) {
                existing.equivalent_ids.push(item.id.clone());
                continue;
            }
        }
        projected.push(ProviderEvidenceItem {
            id: item.id.clone(),
            equivalent_ids: Vec::new(),
            kind: item.kind,
            trust: item.trust,
            evidence_classes,
            content: item.content.clone(),
        });
    }
    for item in &mut projected {
        item.equivalent_ids.sort();
        item.equivalent_ids.dedup();
    }
    projected
}

fn model_relevant_evidence_item(item: &EvidenceItem) -> bool {
    model_relevant_evidence_identity(item.kind, &item.provenance)
}

fn model_relevant_evidence_identity(kind: EvidenceItemKind, source: &str) -> bool {
    kind != EvidenceItemKind::Fact || source != "audit-report#selected-finding-source-roles"
}

fn provider_evidence_item_value(item: &ProviderEvidenceItem) -> Result<ModelValue, String> {
    let kind = serde_json::to_value(item.kind).map_err(|error| error.to_string())?;
    let trust = serde_json::to_value(item.trust).map_err(|error| error.to_string())?;
    let classes =
        serde_json::to_value(&item.evidence_classes).map_err(|error| error.to_string())?;
    Ok(ModelValue::fields(vec![
        ModelField::new("ID", ModelValue::text(&item.id)),
        ModelField::new("Equivalent IDs", ModelValue::list(&item.equivalent_ids)),
        ModelField::new("Kind", model_value_from_json(&kind)),
        ModelField::new("Trust", model_value_from_json(&trust)),
        ModelField::new("Evidence classes", model_value_from_json(&classes)),
        ModelField::new("Content", projected_evidence_content(item)),
    ]))
}

fn projected_evidence_content(item: &ProviderEvidenceItem) -> ModelValue {
    if item.kind == EvidenceItemKind::Finding {
        return project_finding_content(&item.content);
    }
    if item.kind == EvidenceItemKind::SourceSpan {
        return project_source_span_content(&item.content);
    }
    if item
        .evidence_classes
        .contains(&EvidenceClass::EvidenceGraph)
    {
        return model_value_from_json(&project_evidence_graph_content(&item.content));
    }
    model_value_from_json(&item.content)
}

fn project_evidence_graph_content(content: &Value) -> Value {
    const HOST_METADATA: [&str; 6] = [
        "analysis_method",
        "evidence_keys",
        "extractor_version",
        "origin",
        "source_role",
        "span_precision",
    ];
    match content {
        Value::Object(source) => Value::Object(
            source
                .iter()
                .filter(|(key, _)| !HOST_METADATA.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), project_evidence_graph_content(value)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.iter().map(project_evidence_graph_content).collect())
        }
        scalar => scalar.clone(),
    }
}

fn project_finding_content(content: &Value) -> ModelValue {
    const ALLOWED: [&str; 9] = [
        "detector",
        "category",
        "title",
        "message",
        "recommendation",
        "severity",
        "confidence",
        "locations",
        "evidence",
    ];
    let Some(source) = content.as_object() else {
        return model_value_from_json(content);
    };
    let mut projected = Map::new();
    for key in ALLOWED {
        if let Some(value) = source.get(key) {
            projected.insert(key.to_string(), value.clone());
        }
    }
    model_value_from_json(&Value::Object(projected))
}

fn project_source_span_content(content: &Value) -> ModelValue {
    let Some(source) = content.as_object() else {
        return model_value_from_json(content);
    };
    let mut fields = source
        .iter()
        .filter(|(key, _)| key.as_str() != "content")
        .map(|(key, value)| ModelField::new(key, model_value_from_json(value)))
        .collect::<Vec<_>>();
    if let Some(source_text) = source.get("content").and_then(Value::as_str) {
        let language = source
            .get("path")
            .and_then(Value::as_str)
            .and_then(|path| path.rsplit_once('.').map(|(_, extension)| extension))
            .unwrap_or("text");
        fields.push(ModelField::new(
            "content",
            ModelValue::repository_source(language, source_text),
        ));
    }
    ModelValue::fields(fields)
}

pub(crate) fn evidence_request_is_satisfied(
    packet: &EvidencePacket,
    request: &CaseEvidenceRequest,
) -> bool {
    satisfied_evidence_items(&packet.items, request).is_some()
}

pub(crate) fn satisfied_evidence_items(
    items: &[EvidenceItem],
    request: &CaseEvidenceRequest,
) -> Option<Vec<EvidenceItem>> {
    match request.request_kind {
        EvidenceLookupRequestKind::FindingSource => request
            .target_id
            .strip_prefix("finding:")
            .map(|id| finding_source_items(items, id)),
        EvidenceLookupRequestKind::SymbolNeighborhood => {
            Some(symbol_source_items(items, &request.target_id))
        }
    }
    .filter(|items| !items.is_empty())
}

fn available_finding_source_target_ids(packet: &EvidencePacket) -> Vec<String> {
    packet
        .allowed_neighborhood
        .finding_ids
        .iter()
        .filter(|id| finding_source_items(&packet.items, id).is_empty())
        .map(|id| format!("finding:{id}"))
        .collect()
}

fn finding_source_items(items: &[EvidenceItem], finding_id: &str) -> Vec<EvidenceItem> {
    let finding_item_id = format!("finding:{finding_id}");
    let Some(location) = items
        .iter()
        .find(|item| item.id == finding_item_id)
        .and_then(|item| item.content["locations"].as_array())
        .and_then(|locations| locations.first())
    else {
        return Vec::new();
    };
    let Some(path) = location["path"].as_str() else {
        return Vec::new();
    };
    let line = location["line"].as_u64().unwrap_or(1);
    source_items_covering(items, path, line, line)
}

fn symbol_source_items(items: &[EvidenceItem], symbol_id: &str) -> Vec<EvidenceItem> {
    let repository_symbol = items.iter().find(|item| {
        item.id == symbol_id
            && item.kind == EvidenceItemKind::Symbol
            && item.provenance.starts_with("repository-map#")
    });
    let covered_source = repository_symbol.and_then(|symbol| {
        let path = symbol.content["path"].as_str()?;
        let start_line = symbol.content["span"]["start_line"].as_u64()?;
        let end_line = symbol.content["span"]["end_line"].as_u64()?;
        Some(source_items_covering(items, path, start_line, end_line))
    });
    match covered_source {
        Some(items) => items,
        None => normalized_evidence_items(items.iter().filter(|item| {
            item.kind == EvidenceItemKind::SourceSpan
                && item.content["id"].as_str() == Some(symbol_id)
        })),
    }
}

fn source_items_covering(
    items: &[EvidenceItem],
    path: &str,
    start_line: u64,
    end_line: u64,
) -> Vec<EvidenceItem> {
    normalized_evidence_items(items.iter().filter(|item| {
        item.kind == EvidenceItemKind::SourceSpan
            && item.content["path"].as_str() == Some(path)
            && item.content["start_line"]
                .as_u64()
                .is_some_and(|source_start| source_start <= start_line)
            && item.content["end_line"]
                .as_u64()
                .is_some_and(|source_end| source_end >= end_line)
    }))
}

fn normalized_evidence_items<'a>(
    items: impl Iterator<Item = &'a EvidenceItem>,
) -> Vec<EvidenceItem> {
    let mut selected = items.cloned().collect::<Vec<_>>();
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected.dedup_by(|left, right| left.id == right.id);
    selected
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finding_projection_keeps_decision_fields_and_strips_host_metadata() {
        let projected = project_finding_content(&json!({
            "id": "EA-1",
            "kind": "finding",
            "identity": {"private": true},
            "detector": "lifecycle-loop-work",
            "message": "Repeated construction.",
            "recommendation": "Review lifecycle ownership.",
            "evidence_keys": ["internal"],
            "provenance": {
                "source_role": "primary",
                "analysis_method": "host-only"
            }
        }));
        let ModelValue::Fields(fields) = projected else {
            panic!("finding projection must be field-oriented");
        };
        let labels = fields
            .iter()
            .map(|field| field.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["detector", "message", "recommendation"]);
    }

    #[test]
    fn production_file_role_stays_host_only_while_test_role_is_model_visible() {
        let source = "audit-report#selected-finding-source-roles";
        assert!(!model_relevant_evidence_identity(
            EvidenceItemKind::Fact,
            source
        ));
        assert!(model_relevant_evidence_identity(
            EvidenceItemKind::Test,
            source
        ));
        assert_eq!(
            project_evidence_graph_content(&json!({
                "id": "file:1",
                "properties": {
                    "bytes": 10,
                    "source_role": "production",
                    "extractor_version": 1
                }
            })),
            json!({"id": "file:1", "properties": {"bytes": 10}})
        );
    }

    #[test]
    fn lifecycle_reownership_projection_excludes_redesigning_an_invalid_hoist() {
        let family = provider_recommendation_family(RecommendationFamily::LifecycleReownership);
        assert!(family.use_when.contains("no relevant intervening mutation"));
        assert!(family
            .use_when
            .contains("Do not use this family to propose a redesign"));
    }

    #[test]
    fn probe_contract_keeps_unconfirmed_polarity_unknown() {
        let instruction = probe_evidence_instruction();
        assert!(instruction.contains(
            "Failure to establish the positive boundary is never evidence for the negative boundary"
        ));
        assert!(instruction.contains("may-or-may-not, conditional, or unconfirmed, answer unknown"));
    }

    #[test]
    fn satisfied_lookup_returns_existing_full_evidence_items() {
        let finding = EvidenceItem::new(
            "finding:EA-1".to_string(),
            EvidenceItemKind::Finding,
            TrustLabel::DeterministicEvidence,
            "test-finding".to_string(),
            json!({"locations": [{"path": "src/lib.rs", "line": 2}]}),
        )
        .unwrap();
        let source = EvidenceItem::new(
            "source-span:test".to_string(),
            EvidenceItemKind::SourceSpan,
            TrustLabel::RepositorySource,
            "test-source".to_string(),
            json!({
                "content": "fn example() {}",
                "end_line": 3,
                "id": "finding-excerpt:src/lib.rs:1-3",
                "kind": "finding",
                "path": "src/lib.rs",
                "start_line": 1
            }),
        )
        .unwrap();
        assert_eq!(
            finding_source_items(&[finding, source.clone()], "EA-1"),
            vec![source]
        );
    }

    #[test]
    fn symbol_lookup_accepts_complete_source_covering_the_repository_map_span() {
        let symbol = EvidenceItem::new(
            "symbol:repository-map-id".to_string(),
            EvidenceItemKind::Symbol,
            TrustLabel::DeterministicEvidence,
            "repository-map#symbol:repository-map-id".to_string(),
            json!({
                "path": "src/lib.rs",
                "span": {"start_line": 4, "end_line": 8}
            }),
        )
        .unwrap();
        let source = EvidenceItem::new(
            "source-span:independent-id".to_string(),
            EvidenceItemKind::SourceSpan,
            TrustLabel::RepositorySource,
            "test-source".to_string(),
            json!({
                "content": "fn example() {}",
                "end_line": 10,
                "id": "finding-excerpt:src/lib.rs:1-10",
                "kind": "finding",
                "path": "src/lib.rs",
                "start_line": 1
            }),
        )
        .unwrap();

        assert_eq!(
            symbol_source_items(&[symbol, source.clone()], "symbol:repository-map-id"),
            vec![source]
        );
    }
}

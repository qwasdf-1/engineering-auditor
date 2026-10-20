use std::collections::BTreeSet;

use super::{
    definition::optional_probe_id, result::derive_result_id, OptionalProbeRequest, ProbeAnswer,
    ProbeDefinition, ProbeError, ProbeOrigin, ProbeResolution, ProbeResult, ResolverKind,
    PROBE_DEFINITION_SCHEMA_VERSION, PROBE_RESULT_SCHEMA_VERSION,
};
use crate::{
    EngineeringCase, EvidenceClass, EvidenceItem, EvidenceItemKind, EvidencePacket, ProbeKind,
};

const EVIDENCE_COMPATIBILITY: &[(ProbeKind, &[EvidenceClass])] = &[
    (
        ProbeKind::Existence,
        &[
            EvidenceClass::Finding,
            EvidenceClass::EvidenceGraph,
            EvidenceClass::RepositoryMap,
        ],
    ),
    (
        ProbeKind::Identity,
        &[
            EvidenceClass::Finding,
            EvidenceClass::EvidenceGraph,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::SemanticEquivalence,
        &[
            EvidenceClass::Finding,
            EvidenceClass::SourceSpan,
            EvidenceClass::Mutation,
            EvidenceClass::ConsumerRequirement,
        ],
    ),
    (
        ProbeKind::Ordering,
        &[
            EvidenceClass::EvidenceGraph,
            EvidenceClass::RepositoryMap,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::Mutation,
        &[
            EvidenceClass::EvidenceGraph,
            EvidenceClass::Mutation,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::Cost,
        &[
            EvidenceClass::Finding,
            EvidenceClass::RuntimeMeasurement,
            EvidenceClass::OperatorConstraint,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::Cardinality,
        &[
            EvidenceClass::Finding,
            EvidenceClass::EvidenceGraph,
            EvidenceClass::SourceRole,
            EvidenceClass::SourceSpan,
            EvidenceClass::Test,
        ],
    ),
    (
        ProbeKind::Lifecycle,
        &[
            EvidenceClass::RepositoryMap,
            EvidenceClass::EvidenceGraph,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::ConsumerRequirement,
        &[
            EvidenceClass::EvidenceGraph,
            EvidenceClass::ConsumerRequirement,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::Constraint,
        &[
            EvidenceClass::RetentionConstraint,
            EvidenceClass::OperatorConstraint,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::Counterexample,
        &[
            EvidenceClass::Finding,
            EvidenceClass::EvidenceGraph,
            EvidenceClass::SourceSpan,
            EvidenceClass::Mutation,
            EvidenceClass::ConsumerRequirement,
            EvidenceClass::Test,
        ],
    ),
    (
        ProbeKind::AlternativeFeasibility,
        &[
            EvidenceClass::RepositoryMap,
            EvidenceClass::Mutation,
            EvidenceClass::ConsumerRequirement,
            EvidenceClass::OperatorConstraint,
            EvidenceClass::SourceSpan,
        ],
    ),
    (
        ProbeKind::VerificationAvailability,
        &[
            EvidenceClass::RepositoryMap,
            EvidenceClass::Capability,
            EvidenceClass::RuntimeMeasurement,
            EvidenceClass::OperatorConstraint,
            EvidenceClass::Test,
        ],
    ),
];

const ITEM_KIND_CLASSES: &[(EvidenceItemKind, &[EvidenceClass])] = &[
    (EvidenceItemKind::Finding, &[EvidenceClass::Finding]),
    (EvidenceItemKind::Fact, &[EvidenceClass::EvidenceGraph]),
    (
        EvidenceItemKind::Measurement,
        &[EvidenceClass::EvidenceGraph],
    ),
    (EvidenceItemKind::SourceSpan, &[EvidenceClass::SourceSpan]),
    (EvidenceItemKind::Symbol, &[EvidenceClass::EvidenceGraph]),
    (EvidenceItemKind::Edge, &[EvidenceClass::EvidenceGraph]),
    (
        EvidenceItemKind::Capability,
        &[EvidenceClass::Capability, EvidenceClass::EvidenceGraph],
    ),
    (
        EvidenceItemKind::BriefConstraint,
        &[EvidenceClass::OperatorConstraint],
    ),
    (
        EvidenceItemKind::Test,
        &[EvidenceClass::Test, EvidenceClass::SourceRole],
    ),
];

pub(crate) fn evidence_class_compatible(kind: ProbeKind, class: EvidenceClass) -> bool {
    EVIDENCE_COMPATIBILITY
        .iter()
        .find(|(candidate, _)| *candidate == kind)
        .is_some_and(|(_, classes)| classes.contains(&class))
}

/// The definition and packet bind to this case's identity and template.
fn valid_header(
    definition: &ProbeDefinition,
    case: &EngineeringCase,
    packet: &EvidencePacket,
) -> bool {
    definition.schema_version == PROBE_DEFINITION_SCHEMA_VERSION
        && definition.case_id == case.case_id
        && definition.case_kind == case.case_kind
        && definition.template_version == case.template_version
        && definition.template_bundle_digest == case.template_bundle_digest
        && packet.case_id == case.case_id
        && packet.input_binding_id == case.input_binding_id
}

/// The question, boundaries, policy constants, and evidence classes are set
/// and compatible with the case.
fn valid_body(definition: &ProbeDefinition, case: &EngineeringCase) -> bool {
    valid_slot(&definition.slot)
        && !definition.question.trim().is_empty()
        && !definition.positive_boundary.trim().is_empty()
        && !definition.negative_boundary.trim().is_empty()
        && !definition.decision_relevance.trim().is_empty()
        && definition.allowed_results == ProbeAnswer::ALL
        && definition.resolver_precedence == ResolverKind::PRECEDENCE
        && !definition.required_evidence_classes.is_empty()
        && !definition.blocked_claim_slots.is_empty()
        && definition
            .required_evidence_classes
            .iter()
            .all(|class| evidence_class_compatible(definition.kind, *class))
        && definition
            .blocked_claim_slots
            .iter()
            .all(|slot| case.claim_slots.contains(slot))
}

/// A mandatory probe is one the case declares; an optional probe targets an
/// allowed decision under its derived id.
fn valid_origin(definition: &ProbeDefinition, case: &EngineeringCase) -> bool {
    match definition.origin {
        ProbeOrigin::Mandatory => {
            definition.decision_target.is_none()
                && case.mandatory_probes.iter().any(|probe| {
                    probe.probe_id == definition.probe_id
                        && probe.slot == definition.slot
                        && probe.kind == definition.kind
                })
        }
        ProbeOrigin::Optional => {
            definition
                .decision_target
                .is_some_and(|target| decision_target_allowed(target, case))
                && optional_probe_id(&case.case_id, &definition.slot)
                    .is_ok_and(|id| id == definition.probe_id)
        }
    }
}

pub(super) fn validate_definition(
    definition: &ProbeDefinition,
    case: &EngineeringCase,
    packet: &EvidencePacket,
) -> Result<(), ProbeError> {
    if !valid_header(definition, case, packet)
        || !valid_body(definition, case)
        || !valid_origin(definition, case)
        || !targets_allowed(definition, case, packet)
    {
        return Err(ProbeError::InvalidDefinition(
            "identity, policy, evidence compatibility, or case targets are invalid".to_string(),
        ));
    }
    Ok(())
}

pub(super) struct OptionalProbeValidationScope<'a> {
    pub case: &'a EngineeringCase,
    pub template: &'a crate::CaseTemplateDefinition,
    pub packet: &'a EvidencePacket,
    pub mandatory_slots: &'a BTreeSet<String>,
}

pub(super) fn validate_optional_request(
    request: &OptionalProbeRequest,
    scope: OptionalProbeValidationScope<'_>,
    optional_slots: &mut BTreeSet<String>,
) -> Result<(), ProbeError> {
    let OptionalProbeValidationScope {
        case,
        template,
        packet,
        mandatory_slots,
    } = scope;
    let allowed_kind = template
        .optional_probe_policy
        .allowed_kinds
        .contains(&request.kind);
    let valid = valid_slot(&request.slot)
        && !mandatory_slots.contains(request.slot.as_str())
        && optional_slots.insert(request.slot.clone())
        && allowed_kind
        && !request.question.trim().is_empty()
        && !request.decision_relevance.trim().is_empty()
        && !request.required_evidence_classes.is_empty()
        && request
            .required_evidence_classes
            .iter()
            .all(|class| evidence_class_compatible(request.kind, *class))
        && !request.blocked_claim_slots.is_empty()
        && request
            .blocked_claim_slots
            .iter()
            .all(|slot| case.claim_slots.contains(slot))
        && decision_target_allowed(request.decision_target, case)
        && !request.target_ids.is_empty()
        && request
            .target_ids
            .iter()
            .all(|target| allowed_target(target, case, packet));
    if !valid {
        return Err(ProbeError::InvalidOptionalProbe(
            "request violates kind, slot, evidence, decision, or case-boundary policy".to_string(),
        ));
    }
    Ok(())
}

fn decision_target_allowed(target: super::ProbeDecisionTarget, case: &EngineeringCase) -> bool {
    match target {
        super::ProbeDecisionTarget::Claim(slot) => case.claim_slots.contains(&slot),
        super::ProbeDecisionTarget::Recommendation(family) => {
            case.recommendation_families.contains(&family)
        }
    }
}

pub(super) fn validate_resolution(
    resolution: &ProbeResolution,
    definition: &ProbeDefinition,
    packet: &EvidencePacket,
    kind: ResolverKind,
) -> Result<(), ProbeError> {
    let header_error = if !definition.allowed_results.contains(&resolution.answer) {
        Some("answer is outside the probe's allowed result vocabulary")
    } else if resolution.explanation.trim().is_empty() {
        Some("resolver explanation is empty")
    } else if (kind == ResolverKind::ModelInterpretation) != resolution.reviewer_run_id.is_some() {
        Some("reviewer-run provenance does not match resolver ownership")
    } else {
        None
    };
    if let Some(message) = header_error {
        return Err(ProbeError::InvalidResult(message.to_string()));
    }
    if let Some(id) = resolution
        .evidence_ids
        .iter()
        .find(|id| !valid_evidence_id(id, definition, packet))
    {
        return Err(ProbeError::InvalidResult(format!(
            "evidence citation `{id}` is absent from the packet or incompatible with the probe"
        )));
    }
    let cited_classes = resolution
        .evidence_ids
        .iter()
        .filter_map(|id| packet.items.iter().find(|item| item.id == *id))
        .flat_map(evidence_classes_for_item)
        .collect::<BTreeSet<_>>();
    if !answer_has_required_evidence_strength(&definition.slot, resolution.answer, &cited_classes) {
        return Err(ProbeError::InvalidResult(
            "material_cost yes or no requires a cited runtime measurement or operator-bound constraint; without one, answer unknown"
                .to_string(),
        ));
    }
    Ok(())
}

fn answer_has_required_evidence_strength(
    slot: &str,
    answer: ProbeAnswer,
    cited_classes: &BTreeSet<EvidenceClass>,
) -> bool {
    slot != "material_cost"
        || !matches!(answer, ProbeAnswer::Yes | ProbeAnswer::No)
        || cited_classes.contains(&EvidenceClass::RuntimeMeasurement)
        || cited_classes.contains(&EvidenceClass::OperatorConstraint)
}

pub(super) fn validate_result(
    result: &ProbeResult,
    definition: &ProbeDefinition,
    packet: &EvidencePacket,
) -> Result<(), ProbeError> {
    let resolution = ProbeResolution {
        answer: result.answer,
        evidence_ids: result.evidence_ids.clone(),
        explanation: result.explanation.clone(),
        limitations: result.limitations.clone(),
        reviewer_run_id: result.reviewer_run_id.clone(),
    };
    validate_resolution(&resolution, definition, packet, result.resolver.kind)?;
    if result.schema_version != PROBE_RESULT_SCHEMA_VERSION
        || result.probe_id != definition.probe_id
        || result.case_id != definition.case_id
        || result.resolver.id.trim().is_empty()
        || result.resolver.version == 0
        || derive_result_id(definition, &result.resolver, &resolution)? != result.result_id
    {
        return Err(ProbeError::InvalidResult(
            "result identity or resolver record is invalid".to_string(),
        ));
    }
    Ok(())
}

fn targets_allowed(
    definition: &ProbeDefinition,
    case: &EngineeringCase,
    packet: &EvidencePacket,
) -> bool {
    !definition.target_ids.is_empty()
        && definition
            .target_ids
            .iter()
            .all(|target| allowed_target(target, case, packet))
}

fn allowed_target(target: &str, case: &EngineeringCase, packet: &EvidencePacket) -> bool {
    case.subject_ids.iter().any(|id| id == target)
        || case
            .finding_ids
            .iter()
            .any(|id| target == format!("finding:{id}"))
        || packet
            .allowed_neighborhood
            .graph_node_ids
            .iter()
            .any(|id| id == target)
        || packet
            .allowed_neighborhood
            .map_symbol_ids
            .iter()
            .any(|id| id == target)
}

fn valid_evidence_id(id: &str, definition: &ProbeDefinition, packet: &EvidencePacket) -> bool {
    packet
        .items
        .iter()
        .find(|item| item.id == id)
        .is_some_and(|item| item_compatible(item, definition))
}

pub(crate) fn admissible_evidence_ids(
    definition: &ProbeDefinition,
    packet: &EvidencePacket,
) -> Vec<String> {
    packet
        .items
        .iter()
        .filter(|item| item_compatible(item, definition))
        .map(|item| item.id.clone())
        .collect()
}

fn item_compatible(item: &EvidenceItem, definition: &ProbeDefinition) -> bool {
    evidence_classes_for_item(item).iter().any(|class| {
        definition.required_evidence_classes.contains(class)
            && evidence_class_compatible(definition.kind, *class)
    })
}

pub(crate) fn evidence_classes_for_item(item: &EvidenceItem) -> Vec<EvidenceClass> {
    provenance_class(item).map_or_else(
        || {
            ITEM_KIND_CLASSES
                .iter()
                .find(|(kind, _)| *kind == item.kind)
                .map_or_else(Vec::new, |(_, classes)| classes.to_vec())
        },
        |class| vec![class],
    )
}

fn provenance_class(item: &EvidenceItem) -> Option<EvidenceClass> {
    match item.kind {
        EvidenceItemKind::Fact if item.provenance.contains("source-role") => {
            Some(EvidenceClass::SourceRole)
        }
        EvidenceItemKind::Measurement if item.provenance.starts_with("input-binding#") => {
            Some(EvidenceClass::RuntimeMeasurement)
        }
        EvidenceItemKind::Symbol if item.provenance.starts_with("repository-map#") => {
            Some(EvidenceClass::RepositoryMap)
        }
        _ => None,
    }
}

fn valid_slot(slot: &str) -> bool {
    !slot.is_empty()
        && slot
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod answer_evidence_tests {
    use super::*;

    #[test]
    fn material_cost_requires_bound_evidence_for_definitive_answers() {
        let structural = BTreeSet::from([EvidenceClass::Finding]);
        let measured = BTreeSet::from([EvidenceClass::RuntimeMeasurement]);
        let operator_bound = BTreeSet::from([EvidenceClass::OperatorConstraint]);

        assert!(!answer_has_required_evidence_strength(
            "material_cost",
            ProbeAnswer::Yes,
            &structural,
        ));
        assert!(!answer_has_required_evidence_strength(
            "material_cost",
            ProbeAnswer::No,
            &structural,
        ));
        assert!(answer_has_required_evidence_strength(
            "material_cost",
            ProbeAnswer::Unknown,
            &structural,
        ));
        assert!(answer_has_required_evidence_strength(
            "material_cost",
            ProbeAnswer::Yes,
            &measured,
        ));
        assert!(answer_has_required_evidence_strength(
            "material_cost",
            ProbeAnswer::No,
            &operator_bound,
        ));
        assert!(answer_has_required_evidence_strength(
            "input_equivalence",
            ProbeAnswer::No,
            &structural,
        ));
    }
}

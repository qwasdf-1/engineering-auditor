use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::identity::generated_item_id;
use super::{
    AllowedEvidenceNeighborhood, ClosureState, EvidenceClosure, EvidenceError, EvidenceIndexes,
    EvidenceItem, EvidenceItemKind, EvidenceOmission, EvidenceOmissionReason, EvidencePacket,
    EvidencePacketBudget, EvidenceUnavailable,
};
use crate::{EngineeringCase, EvidenceClass, EvidenceRequirement, ReviewInput, TrustLabel};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RequirementTier {
    Required,
    Blocking,
    Optional,
    Presentation,
}

#[derive(Clone)]
pub(super) struct Requirement {
    pub(super) id: String,
    pub(super) class: EvidenceClass,
    pub(super) tier: RequirementTier,
}

#[derive(Clone)]
pub(super) struct CandidateItem {
    pub(super) item: EvidenceItem,
    pub(super) classes: BTreeSet<EvidenceClass>,
    pub(super) tier: RequirementTier,
}

pub struct EvidenceCompiler;

impl EvidenceCompiler {
    pub fn compile(
        &self,
        case: &EngineeringCase,
        input: &ReviewInput,
    ) -> Result<EvidencePacket, EvidenceError> {
        self.compile_with_budget_and_lookup_items(case, input, EvidencePacketBudget::default(), &[])
    }

    pub fn compile_with_budget(
        &self,
        case: &EngineeringCase,
        input: &ReviewInput,
        budget: EvidencePacketBudget,
    ) -> Result<EvidencePacket, EvidenceError> {
        self.compile_with_budget_and_lookup_items(case, input, budget, &[])
    }

    pub fn compile_with_lookup_items(
        &self,
        case: &EngineeringCase,
        input: &ReviewInput,
        lookup_items: &[EvidenceItem],
    ) -> Result<EvidencePacket, EvidenceError> {
        self.compile_with_budget_and_lookup_items(
            case,
            input,
            EvidencePacketBudget::default(),
            lookup_items,
        )
    }

    pub fn compile_with_budget_and_lookup_items(
        &self,
        case: &EngineeringCase,
        input: &ReviewInput,
        budget: EvidencePacketBudget,
        lookup_items: &[EvidenceItem],
    ) -> Result<EvidencePacket, EvidenceError> {
        super::case_validation::validate_case(case, input)?;
        let budget = budget.tightened_by(input.operator_brief())?;
        let neighborhood =
            super::neighborhood::case_neighborhood(case, input, budget.maximum_graph_hops)?;
        let requirements = requirements(case);
        let candidates = collect_candidates(case, input, &neighborhood, lookup_items)?;
        super::packet_compile::compile_packet(case, budget, neighborhood, requirements, candidates)
    }
}

fn requirements(case: &EngineeringCase) -> Vec<Requirement> {
    let mut requirements = Vec::new();
    extend_requirements(
        &mut requirements,
        &case.evidence_contract.required,
        RequirementTier::Required,
    );
    extend_requirements(
        &mut requirements,
        &case.evidence_contract.blocking,
        RequirementTier::Blocking,
    );
    extend_requirements(
        &mut requirements,
        &case.evidence_contract.optional,
        RequirementTier::Optional,
    );
    requirements
}

fn extend_requirements(
    target: &mut Vec<Requirement>,
    source: &[EvidenceRequirement],
    tier: RequirementTier,
) {
    target.extend(source.iter().map(|requirement| Requirement {
        id: requirement.id.clone(),
        class: requirement.evidence_class,
        tier,
    }));
}

fn collect_candidates(
    case: &EngineeringCase,
    input: &ReviewInput,
    neighborhood: &AllowedEvidenceNeighborhood,
    lookup_items: &[EvidenceItem],
) -> Result<Vec<CandidateItem>, EvidenceError> {
    let mut candidates = Vec::new();
    for finding_id in &case.finding_ids {
        let finding = input.finding(finding_id).ok_or_else(|| {
            EvidenceError::InvalidPacket(format!("selected finding is absent: {finding_id}"))
        })?;
        candidates.push(candidate(
            EvidenceItem::new(
                format!("finding:{finding_id}"),
                EvidenceItemKind::Finding,
                TrustLabel::DeterministicEvidence,
                format!("audit-report#finding:{finding_id}"),
                finding.clone(),
            )?,
            [EvidenceClass::Finding],
            RequirementTier::Required,
        ));
    }
    super::graph_candidates::collect_graph_candidates(input, neighborhood, &mut candidates)?;
    collect_map_candidates(input, neighborhood, &mut candidates)?;
    collect_source_role_candidate(case, input, &mut candidates)?;
    collect_capability_candidates(case, input, &mut candidates)?;
    collect_constraint_candidates(case, input, &mut candidates)?;
    collect_attachment_candidates(input, &mut candidates)?;
    for item in lookup_items {
        if item.kind != EvidenceItemKind::SourceSpan
            || !item.id.starts_with(EvidenceItemKind::SourceSpan.prefix())
        {
            return Err(EvidenceError::InvalidPacket(
                "lookup additions must be typed source-span evidence".to_string(),
            ));
        }
        candidates.push(candidate(
            item.clone(),
            [EvidenceClass::SourceSpan],
            RequirementTier::Optional,
        ));
    }
    candidates.sort_by(|left, right| {
        (left.tier, left.item.id.as_str()).cmp(&(right.tier, right.item.id.as_str()))
    });
    candidates.dedup_by(|left, right| left.item.id == right.item.id);
    Ok(candidates)
}

fn collect_map_candidates(
    input: &ReviewInput,
    neighborhood: &AllowedEvidenceNeighborhood,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    let allowed = neighborhood
        .map_symbol_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for symbol in &input.repository_map().symbols {
        if !allowed.contains(symbol.id.as_str()) {
            continue;
        }
        let mut content = serde_json::to_value(symbol)
            .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?;
        if let Some(path) = input
            .repository_map()
            .files
            .iter()
            .find(|file| file.id == symbol.file_id)
            .map(|file| file.path.clone())
        {
            content
                .as_object_mut()
                .expect("repository-map symbols serialize as objects")
                .insert("path".to_string(), Value::String(path));
        }
        candidates.push(candidate(
            EvidenceItem::new(
                symbol.id.clone(),
                EvidenceItemKind::Symbol,
                TrustLabel::DeterministicEvidence,
                format!("repository-map#{}", symbol.id),
                content,
            )?,
            [EvidenceClass::RepositoryMap],
            RequirementTier::Presentation,
        ));
    }
    Ok(())
}

fn collect_source_role_candidate(
    case: &EngineeringCase,
    input: &ReviewInput,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    let roles = case
        .finding_ids
        .iter()
        .filter_map(|id| input.finding(id))
        .filter_map(|finding| finding["provenance"]["source_role"].as_str())
        .collect::<BTreeSet<_>>();
    if roles.is_empty() {
        return Ok(());
    }
    let test_only = roles.contains("test") || roles.contains("fixture");
    let content = json!({"finding_ids": case.finding_ids, "source_roles": roles});
    let item_kind = if test_only {
        EvidenceItemKind::Test
    } else {
        EvidenceItemKind::Fact
    };
    let id = generated_item_id(item_kind.prefix().trim_end_matches(':'), &content)?;
    let mut classes = BTreeSet::from([EvidenceClass::SourceRole]);
    if test_only {
        classes.insert(EvidenceClass::Test);
    }
    candidates.push(CandidateItem {
        item: EvidenceItem::new(
            id,
            item_kind,
            TrustLabel::DeterministicEvidence,
            "audit-report#selected-finding-source-roles".to_string(),
            content,
        )?,
        classes,
        tier: RequirementTier::Required,
    });
    Ok(())
}

fn collect_capability_candidates(
    case: &EngineeringCase,
    input: &ReviewInput,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    let detectors = case
        .finding_ids
        .iter()
        .filter_map(|id| input.finding(id))
        .filter_map(|finding| finding["detector"].as_str())
        .collect::<BTreeSet<_>>();
    for capability in input.report()["capability_executions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|capability| {
            capability["detector_id"]
                .as_str()
                .is_some_and(|id| detectors.contains(id))
        })
    {
        let id = generated_item_id("capability", capability)?;
        candidates.push(candidate(
            EvidenceItem::new(
                id,
                EvidenceItemKind::Capability,
                TrustLabel::DeterministicEvidence,
                "audit-report#capability-executions".to_string(),
                capability.clone(),
            )?,
            [EvidenceClass::Capability],
            RequirementTier::Presentation,
        ));
    }
    Ok(())
}

fn collect_constraint_candidates(
    case: &EngineeringCase,
    input: &ReviewInput,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    let allowed = case
        .operator_constraints
        .constraint_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for constraint in input
        .operator_brief()
        .into_iter()
        .flat_map(|brief| &brief.constraints)
        .filter(|constraint| allowed.contains(constraint.id.as_str()))
    {
        let content = serde_json::to_value(constraint)
            .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?;
        candidates.push(candidate(
            EvidenceItem::new(
                generated_item_id("brief-constraint", &content)?,
                EvidenceItemKind::BriefConstraint,
                TrustLabel::OperatorIntent,
                format!("review-brief#constraint/{}", constraint.id),
                content,
            )?,
            [EvidenceClass::OperatorConstraint],
            RequirementTier::Optional,
        ));
    }
    Ok(())
}

fn collect_attachment_candidates(
    input: &ReviewInput,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    for attachment in input.evidence_attachments() {
        let content = json!({
            "attachment_digest": attachment.digest(),
            "attachment_id": attachment.id(),
            "attachment_kind": attachment.kind(),
            "semantic_snapshot": attachment.semantic_snapshot(),
        });
        candidates.push(candidate(
            EvidenceItem::new(
                generated_item_id("measurement", &content)?,
                EvidenceItemKind::Measurement,
                TrustLabel::DeterministicEvidence,
                format!("input-binding#{}", attachment.id()),
                content,
            )?,
            [EvidenceClass::RuntimeMeasurement],
            RequirementTier::Optional,
        ));
    }
    Ok(())
}

pub(super) fn candidate(
    item: EvidenceItem,
    classes: impl IntoIterator<Item = EvidenceClass>,
    tier: RequirementTier,
) -> CandidateItem {
    CandidateItem {
        item,
        classes: classes.into_iter().collect(),
        tier,
    }
}

pub(super) fn record_budget_omissions(
    omissions: &mut Vec<EvidenceOmission>,
    requirements: &[Requirement],
    candidate: &CandidateItem,
    reason: EvidenceOmissionReason,
    detail: &str,
) {
    let requirement_ids = requirements
        .iter()
        .filter(|requirement| {
            requirement.tier == candidate.tier && candidate.classes.contains(&requirement.class)
        })
        .map(|requirement| requirement.id.clone())
        .chain(
            (candidate.tier == RequirementTier::Presentation).then_some("presentation".to_string()),
        );
    omissions.extend(requirement_ids.map(|requirement_id| EvidenceOmission {
        requirement_id,
        item_id: Some(candidate.item.id.clone()),
        reason,
        detail: detail.to_string(),
    }));
}

#[derive(Default)]
struct ClosureAccumulator {
    indexes: EvidenceIndexes,
    unavailable: BTreeMap<String, EvidenceUnavailable>,
    missing_classes: BTreeSet<EvidenceClass>,
    required_budget_loss: bool,
    blocking_conflict: bool,
    reasons: Vec<String>,
}

pub(super) fn closure_state(
    case: &EngineeringCase,
    requirements: &[Requirement],
    admitted: &[CandidateItem],
    omissions: &[EvidenceOmission],
) -> (
    EvidenceIndexes,
    BTreeMap<String, EvidenceUnavailable>,
    EvidenceClosure,
) {
    let mut accumulator = ClosureAccumulator::default();
    for requirement in requirements {
        let ids = admitted
            .iter()
            .filter(|candidate| candidate.classes.contains(&requirement.class))
            .map(|candidate| candidate.item.id.clone())
            .collect::<Vec<_>>();
        let had_budget_omission = omissions.iter().any(|omission| {
            omission.requirement_id == requirement.id
                && matches!(
                    omission.reason,
                    EvidenceOmissionReason::TargetBudget | EvidenceOmissionReason::HardBudget
                )
        });
        index_requirement(&mut accumulator, requirement, ids, had_budget_omission);
    }
    let state = derive_closure_state(&mut accumulator, omissions);
    let affected_claim_slots = if !accumulator.missing_classes.is_empty() {
        case.claim_slots.clone()
    } else {
        Default::default()
    };
    (
        accumulator.indexes,
        accumulator.unavailable,
        EvidenceClosure {
            state,
            reasons: accumulator.reasons,
            missing_evidence_classes: accumulator.missing_classes.into_iter().collect(),
            affected_claim_slots,
        },
    )
}

fn index_requirement(
    accumulator: &mut ClosureAccumulator,
    requirement: &Requirement,
    ids: Vec<String>,
    had_budget_omission: bool,
) {
    match requirement.tier {
        RequirementTier::Required => {
            accumulator
                .indexes
                .required
                .insert(requirement.id.clone(), ids.clone());
            if ids.is_empty() {
                accumulator.missing_classes.insert(requirement.class);
                accumulator.required_budget_loss |= had_budget_omission;
            }
        }
        RequirementTier::Blocking => {
            accumulator
                .indexes
                .blocking
                .insert(requirement.id.clone(), ids.clone());
            accumulator
                .indexes
                .counterevidence
                .insert(requirement.id.clone(), ids.clone());
            if ids.is_empty() {
                record_unavailable(
                    accumulator,
                    requirement,
                    "no typed counterevidence is bound to this review input",
                );
            } else {
                accumulator.blocking_conflict = true;
                accumulator.reasons.push(format!(
                    "blocking counterevidence is present for {}",
                    requirement.id
                ));
            }
        }
        RequirementTier::Optional => {
            accumulator
                .indexes
                .optional
                .insert(requirement.id.clone(), ids.clone());
            if ids.is_empty() {
                record_unavailable(
                    accumulator,
                    requirement,
                    "no typed optional evidence is bound to this review input",
                );
            }
        }
        RequirementTier::Presentation => {}
    }
}

fn record_unavailable(
    accumulator: &mut ClosureAccumulator,
    requirement: &Requirement,
    reason: &str,
) {
    accumulator.unavailable.insert(
        requirement.id.clone(),
        EvidenceUnavailable {
            evidence_class: requirement.class,
            reason: reason.to_string(),
        },
    );
}

fn derive_closure_state(
    accumulator: &mut ClosureAccumulator,
    omissions: &[EvidenceOmission],
) -> ClosureState {
    if accumulator.required_budget_loss {
        accumulator
            .reasons
            .push("required evidence exceeded the hard packet ceiling".to_string());
        ClosureState::OverBudget
    } else if !accumulator.missing_classes.is_empty() {
        accumulator
            .reasons
            .push("required evidence classes are missing".to_string());
        ClosureState::Blocked
    } else if accumulator.blocking_conflict || has_budget_omission(omissions) {
        accumulator
            .reasons
            .push("optional or blocking evidence remains unresolved".to_string());
        ClosureState::PartiallyClosed
    } else {
        accumulator
            .reasons
            .push("all declared evidence requirements are resolved".to_string());
        ClosureState::Closed
    }
}

fn has_budget_omission(omissions: &[EvidenceOmission]) -> bool {
    omissions.iter().any(|omission| {
        matches!(
            omission.reason,
            EvidenceOmissionReason::TargetBudget | EvidenceOmissionReason::HardBudget
        )
    })
}

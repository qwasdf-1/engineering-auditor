use std::collections::{BTreeMap, BTreeSet};

use super::dependency::validate_dependency_graph;
use super::{EngineeringClaim, ENGINEERING_CLAIM_SCHEMA_VERSION};
use crate::{ClaimSlot, ClaimStatus, ProbeAnswer, StableId};

pub(crate) fn validate_claims(
    case_id: &StableId,
    allowed_slots: &[ClaimSlot],
    probe_answers: &BTreeMap<StableId, ProbeAnswer>,
    claims: &[EngineeringClaim],
) -> Result<(), String> {
    let context = ClaimValidationContext {
        case_id,
        allowed_slots,
        probe_answers,
    };
    let mut ids = BTreeSet::new();
    let mut errors = Vec::new();
    let mut references_valid = BTreeMap::new();
    for claim in claims {
        let rebuilt = match rebuild_claim(case_id, claim) {
            Ok(rebuilt) => rebuilt,
            Err(error) => {
                errors.push(format!("claim slot {:?} is malformed: {error}", claim.slot));
                references_valid.insert(&claim.claim_id, false);
                continue;
            }
        };
        let duplicate = !ids.insert(claim.claim_id.clone());
        let (mut claim_errors, missing_probe_ids) =
            validate_claim_references(claim, &rebuilt, duplicate, &context);
        errors.append(&mut claim_errors);
        references_valid.insert(&claim.claim_id, missing_probe_ids.is_empty());
    }
    let dependency_graph_valid = validate_dependencies(claims, &mut errors);
    let claims_by_id = claims
        .iter()
        .map(|claim| (&claim.claim_id, claim))
        .collect::<BTreeMap<_, _>>();
    let derived_statuses = if dependency_graph_valid {
        derive_claim_statuses(probe_answers, &claims_by_id)
    } else {
        Default::default()
    };
    errors.extend(claims.iter().filter_map(|claim| {
        references_valid
            .get(&claim.claim_id)
            .copied()
            .unwrap_or(false)
            .then(|| validate_status(claim, probe_answers, &derived_statuses).err())
            .flatten()
    }));
    errors.sort();
    errors.dedup();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

struct ClaimValidationContext<'a> {
    case_id: &'a StableId,
    allowed_slots: &'a [ClaimSlot],
    probe_answers: &'a BTreeMap<StableId, ProbeAnswer>,
}

fn rebuild_claim(case_id: &StableId, claim: &EngineeringClaim) -> Result<EngineeringClaim, String> {
    super::EngineeringClaim::from_draft(
        case_id.clone(),
        super::ClaimDraft {
            slot: claim.slot,
            kind: claim.kind,
            proposition: claim.proposition.clone(),
            status: claim.status,
            supporting_probe_ids: claim.supporting_probe_ids.clone(),
            counter_probe_ids: claim.counter_probe_ids.clone(),
            dependency_claim_ids: claim.dependency_claim_ids.clone(),
            assumptions: claim.assumptions.clone(),
            confidence: claim.confidence,
        },
    )
}

fn validate_claim_references(
    claim: &EngineeringClaim,
    rebuilt: &EngineeringClaim,
    duplicate: bool,
    context: &ClaimValidationContext<'_>,
) -> (Vec<String>, BTreeSet<StableId>) {
    let mut errors = Vec::new();
    let invalid_identity = claim.schema_version != ENGINEERING_CLAIM_SCHEMA_VERSION
        || &claim.case_id != context.case_id
        || claim.claim_id != rebuilt.claim_id
        || duplicate;
    if invalid_identity {
        errors.push(format!(
            "claim slot {:?} has an invalid or duplicate identity",
            claim.slot
        ));
    }
    if !context.allowed_slots.contains(&claim.slot) {
        errors.push(format!(
            "claim slot {:?} is not allowed by the case",
            claim.slot
        ));
    }
    let missing_probe_ids = claim
        .supporting_probe_ids
        .iter()
        .chain(&claim.counter_probe_ids)
        .filter(|id| !context.probe_answers.contains_key(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !missing_probe_ids.is_empty() {
        errors.push(format!(
            "claim slot {:?} references unknown probes: {:?}",
            claim.slot, missing_probe_ids
        ));
    }
    (errors, missing_probe_ids)
}

fn validate_dependencies(claims: &[EngineeringClaim], errors: &mut Vec<String>) -> bool {
    match validate_dependency_graph(claims) {
        Ok(()) => true,
        Err(error) => {
            errors.push(error);
            false
        }
    }
}

struct ClaimAnswers {
    supporting: BTreeSet<ProbeAnswer>,
    counter: BTreeSet<ProbeAnswer>,
}

impl ClaimAnswers {
    fn from_claim(
        claim: &EngineeringClaim,
        probe_answers: &BTreeMap<StableId, ProbeAnswer>,
    ) -> Self {
        Self {
            supporting: claim
                .supporting_probe_ids
                .iter()
                .filter_map(|id| probe_answers.get(id))
                .copied()
                .collect(),
            counter: claim
                .counter_probe_ids
                .iter()
                .filter_map(|id| probe_answers.get(id))
                .copied()
                .collect(),
        }
    }
}

fn validate_status(
    claim: &EngineeringClaim,
    probe_answers: &BTreeMap<StableId, ProbeAnswer>,
    derived_statuses: &BTreeMap<StableId, Option<ClaimStatus>>,
) -> Result<(), String> {
    let answers = ClaimAnswers::from_claim(claim, probe_answers);
    let dependency_statuses = claim
        .dependency_claim_ids
        .iter()
        .filter_map(|id| derived_statuses.get(id).copied().flatten())
        .collect::<BTreeSet<_>>();
    let supported_status = derived_statuses.get(&claim.claim_id).copied().flatten();
    if supported_status == Some(claim.status) {
        return Ok(());
    }
    let supported_statuses = supported_status.into_iter().collect::<BTreeSet<_>>();
    Err(format!(
        "claim slot {:?} status {:?} is not supported: supporting answers {:?}, counter answers {:?}, dependency statuses {:?}; supported statuses are {:?}",
        claim.slot,
        claim.status,
        answers.supporting,
        answers.counter,
        dependency_statuses,
        supported_statuses
    ))
}

fn derive_claim_statuses(
    probe_answers: &BTreeMap<StableId, ProbeAnswer>,
    claims: &BTreeMap<&StableId, &EngineeringClaim>,
) -> BTreeMap<StableId, Option<ClaimStatus>> {
    let mut derived = BTreeMap::new();
    for claim_id in claims.keys() {
        derive_claim_status(claim_id, probe_answers, claims, &mut derived);
    }
    derived
}

fn derive_claim_status(
    claim_id: &StableId,
    probe_answers: &BTreeMap<StableId, ProbeAnswer>,
    claims: &BTreeMap<&StableId, &EngineeringClaim>,
    derived: &mut BTreeMap<StableId, Option<ClaimStatus>>,
) -> Option<ClaimStatus> {
    if let Some(status) = derived.get(claim_id) {
        return *status;
    }
    let claim = claims.get(claim_id)?;
    let answers = ClaimAnswers::from_claim(claim, probe_answers);
    let mut dependency_statuses = BTreeSet::new();
    for dependency_id in &claim.dependency_claim_ids {
        dependency_statuses.insert(derive_claim_status(
            dependency_id,
            probe_answers,
            claims,
            derived,
        )?);
    }
    let status = supported_status(&answers.supporting, &answers.counter, &dependency_statuses);
    derived.insert(claim_id.clone(), status);
    status
}

fn supported_status(
    supporting: &BTreeSet<ProbeAnswer>,
    counter: &BTreeSet<ProbeAnswer>,
    dependency_statuses: &BTreeSet<ClaimStatus>,
) -> Option<ClaimStatus> {
    if is_contested(supporting, counter, dependency_statuses) {
        return Some(ClaimStatus::Contested);
    }
    if is_not_substantiated(supporting, dependency_statuses) {
        return Some(ClaimStatus::NotSubstantiated);
    }
    if is_blocked(supporting, counter, dependency_statuses) {
        return Some(ClaimStatus::Blocked);
    }
    if is_substantiated(supporting, dependency_statuses) {
        return Some(ClaimStatus::Substantiated);
    }
    is_partially_substantiated(supporting, dependency_statuses)
        .then_some(ClaimStatus::PartiallySubstantiated)
}

fn is_contested(
    supporting: &BTreeSet<ProbeAnswer>,
    counter: &BTreeSet<ProbeAnswer>,
    dependencies: &BTreeSet<ClaimStatus>,
) -> bool {
    supporting.contains(&ProbeAnswer::ConflictingEvidence)
        || counter.contains(&ProbeAnswer::Yes)
        || dependencies.contains(&ClaimStatus::Contested)
}

fn is_not_substantiated(
    supporting: &BTreeSet<ProbeAnswer>,
    dependencies: &BTreeSet<ClaimStatus>,
) -> bool {
    supporting.contains(&ProbeAnswer::NotApplicable)
        || dependencies.contains(&ClaimStatus::NotSubstantiated)
}

fn is_blocked(
    supporting: &BTreeSet<ProbeAnswer>,
    counter: &BTreeSet<ProbeAnswer>,
    dependencies: &BTreeSet<ClaimStatus>,
) -> bool {
    supporting.contains(&ProbeAnswer::Unknown)
        || counter.contains(&ProbeAnswer::Unknown)
        || dependencies.contains(&ClaimStatus::Blocked)
}

fn is_substantiated(
    supporting: &BTreeSet<ProbeAnswer>,
    dependencies: &BTreeSet<ClaimStatus>,
) -> bool {
    (!supporting.is_empty() || !dependencies.is_empty())
        && supporting.iter().all(definitive)
        && dependencies
            .iter()
            .all(|status| *status == ClaimStatus::Substantiated)
}

fn is_partially_substantiated(
    supporting: &BTreeSet<ProbeAnswer>,
    dependencies: &BTreeSet<ClaimStatus>,
) -> bool {
    (supporting.iter().any(definitive) || !dependencies.is_empty())
        && dependencies.iter().all(|status| {
            matches!(
                status,
                ClaimStatus::Substantiated | ClaimStatus::PartiallySubstantiated
            )
        })
}

fn definitive(answer: &ProbeAnswer) -> bool {
    matches!(answer, ProbeAnswer::Yes | ProbeAnswer::No)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{ClaimConfidence, ClaimDraft, EngineeringClaim, EngineeringClaimKind};

    #[test]
    fn substantiated_claim_accepts_positive_and_negative_boundary_support() {
        let case_id = StableId::from_identity("engineering-case", 1, &json!({"case": "mixed"}))
            .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "positive"}))
            .expect("yes probe id");
        let no = StableId::from_identity("case-probe", 1, &json!({"probe": "negative"}))
            .expect("no probe id");
        let claim = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::RootCause,
                proposition: "Repetition exists and no intervening mutation invalidates reuse."
                    .to_string(),
                status: ClaimStatus::Substantiated,
                supporting_probe_ids: vec![yes.clone(), no.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::High,
            },
        )
        .expect("claim");
        let answers = BTreeMap::from([(yes, ProbeAnswer::Yes), (no, ProbeAnswer::No)]);
        assert!(validate_claims(
            &case_id,
            &[ClaimSlot::LifecycleConclusion],
            &answers,
            &[claim],
        )
        .is_ok());
    }

    #[test]
    fn blocked_actionability_dependency_blocks_a_root_cause_conclusion() {
        let case_id =
            StableId::from_identity("engineering-case", 1, &json!({"case": "dependency"}))
                .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "yes"}))
            .expect("yes probe id");
        let unknown = StableId::from_identity("case-probe", 1, &json!({"probe": "unknown"}))
            .expect("unknown probe id");
        let premise = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::WiderOwner,
                kind: EngineeringClaimKind::RootCause,
                proposition: "The wider owner remains unresolved.".to_string(),
                status: ClaimStatus::Blocked,
                supporting_probe_ids: vec![unknown.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect("premise");
        let conclusion = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::Actionability,
                proposition: "The lifecycle conclusion inherits the unresolved owner.".to_string(),
                status: ClaimStatus::Blocked,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: vec![premise.claim_id.clone()],
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect("conclusion");
        let answers = BTreeMap::from([(yes, ProbeAnswer::Yes), (unknown, ProbeAnswer::Unknown)]);
        assert!(validate_claims(
            &case_id,
            &[ClaimSlot::WiderOwner, ClaimSlot::LifecycleConclusion],
            &answers,
            &[premise, conclusion],
        )
        .is_ok());
    }

    #[test]
    fn dependency_only_claim_derives_status_without_repeating_probe_ids() {
        let case_id =
            StableId::from_identity("engineering-case", 1, &json!({"case": "dependency-only"}))
                .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "yes"}))
            .expect("yes probe id");
        let premise = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::WiderOwner,
                kind: EngineeringClaimKind::Actionability,
                proposition: "A wider owner is established.".to_string(),
                status: ClaimStatus::Substantiated,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::High,
            },
        )
        .expect("premise");
        let conclusion = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::RootCause,
                proposition: "The lifecycle diagnosis follows from the admitted premise."
                    .to_string(),
                status: ClaimStatus::Substantiated,
                supporting_probe_ids: Vec::new(),
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: vec![premise.claim_id.clone()],
                assumptions: Vec::new(),
                confidence: ClaimConfidence::High,
            },
        )
        .expect("dependency-derived claim");
        assert!(validate_claims(
            &case_id,
            &[ClaimSlot::WiderOwner, ClaimSlot::LifecycleConclusion],
            &BTreeMap::from([(yes, ProbeAnswer::Yes)]),
            &[premise, conclusion],
        )
        .is_ok());
    }

    #[test]
    fn claim_without_probe_support_or_dependency_is_rejected() {
        let case_id =
            StableId::from_identity("engineering-case", 1, &json!({"case": "unsupported"}))
                .expect("case id");
        let error = EngineeringClaim::from_draft(
            case_id,
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::RootCause,
                proposition: "An unsupported conclusion.".to_string(),
                status: ClaimStatus::Substantiated,
                supporting_probe_ids: Vec::new(),
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect_err("a root claim still needs direct support");
        assert!(error.contains("supporting probe or dependency"));
    }

    #[test]
    fn invalid_status_diagnostic_lists_the_supported_statuses() {
        let case_id =
            StableId::from_identity("engineering-case", 1, &json!({"case": "diagnostic"}))
                .expect("case id");
        let unknown = StableId::from_identity("case-probe", 1, &json!({"probe": "unknown"}))
            .expect("unknown probe id");
        let claim = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::ConsumerRequirement,
                kind: EngineeringClaimKind::Actionability,
                proposition: "The consumer requirement remains unresolved.".to_string(),
                status: ClaimStatus::NotSubstantiated,
                supporting_probe_ids: vec![unknown.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect("claim");
        let error = validate_claims(
            &case_id,
            &[ClaimSlot::ConsumerRequirement],
            &BTreeMap::from([(unknown, ProbeAnswer::Unknown)]),
            &[claim],
        )
        .expect_err("unknown support must reject not-substantiated");
        assert!(error.contains("supported statuses are {Blocked}"));
    }

    #[test]
    fn coupled_status_diagnostic_uses_dependency_fixed_point() {
        let case_id =
            StableId::from_identity("engineering-case", 1, &json!({"case": "fixed-point"}))
                .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "yes"}))
            .expect("yes probe id");
        let premise = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::MaterialImpact,
                kind: EngineeringClaimKind::Actionability,
                proposition: "The supported impact was labeled partial.".to_string(),
                status: ClaimStatus::PartiallySubstantiated,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Medium,
            },
        )
        .expect("premise");
        let conclusion = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::RootCause,
                proposition: "The conclusion already has its fixed-point status.".to_string(),
                status: ClaimStatus::Substantiated,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: Vec::new(),
                dependency_claim_ids: vec![premise.claim_id.clone()],
                assumptions: Vec::new(),
                confidence: ClaimConfidence::High,
            },
        )
        .expect("conclusion");
        let error = validate_claims(
            &case_id,
            &[ClaimSlot::MaterialImpact, ClaimSlot::LifecycleConclusion],
            &BTreeMap::from([(yes, ProbeAnswer::Yes)]),
            &[premise, conclusion],
        )
        .expect_err("only the premise status should require correction");
        assert!(error.contains("claim slot MaterialImpact"));
        assert!(!error.contains("claim slot LifecycleConclusion"));
    }

    #[test]
    fn unknown_counterprobe_blocks_a_derived_conclusion() {
        let case_id = StableId::from_identity("engineering-case", 1, &json!({"case": "counter"}))
            .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "yes"}))
            .expect("yes probe id");
        let unknown = StableId::from_identity("case-probe", 1, &json!({"probe": "unknown"}))
            .expect("unknown probe id");
        let claim = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::Actionability,
                proposition: "An unresolved counterprobe blocks the conclusion.".to_string(),
                status: ClaimStatus::Blocked,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: vec![unknown.clone()],
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect("claim");
        let answers = BTreeMap::from([(yes, ProbeAnswer::Yes), (unknown, ProbeAnswer::Unknown)]);
        assert!(validate_claims(
            &case_id,
            &[ClaimSlot::LifecycleConclusion],
            &answers,
            &[claim],
        )
        .is_ok());
    }

    #[test]
    fn unknown_counterprobe_rejects_a_partial_conclusion() {
        let case_id = StableId::from_identity(
            "engineering-case",
            1,
            &json!({"case": "counter-precedence"}),
        )
        .expect("case id");
        let yes = StableId::from_identity("case-probe", 1, &json!({"probe": "yes"}))
            .expect("yes probe id");
        let unknown = StableId::from_identity("case-probe", 1, &json!({"probe": "unknown"}))
            .expect("unknown probe id");
        let claim = EngineeringClaim::from_draft(
            case_id.clone(),
            ClaimDraft {
                slot: ClaimSlot::LifecycleConclusion,
                kind: EngineeringClaimKind::RootCause,
                proposition: "An unresolved counterprobe blocks the conclusion.".to_string(),
                status: ClaimStatus::PartiallySubstantiated,
                supporting_probe_ids: vec![yes.clone()],
                counter_probe_ids: vec![unknown.clone()],
                dependency_claim_ids: Vec::new(),
                assumptions: Vec::new(),
                confidence: ClaimConfidence::Low,
            },
        )
        .expect("claim");
        let error = validate_claims(
            &case_id,
            &[ClaimSlot::LifecycleConclusion],
            &BTreeMap::from([(yes, ProbeAnswer::Yes), (unknown, ProbeAnswer::Unknown)]),
            &[claim],
        )
        .expect_err("blocked status must take precedence over partial support");
        assert!(error.contains("supported statuses are {Blocked}"));
    }
}

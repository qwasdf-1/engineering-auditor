use std::collections::{BTreeMap, BTreeSet};

use super::dependency::validate_dependency_graph;
use super::{EngineeringClaim, ENGINEERING_CLAIM_SCHEMA_VERSION};
use crate::{ClaimSlot, ClaimStatus, StableId};

/// Checks a case's claims: each identity rebuilds from its content, each
/// slot is one the case allows, every cited evidence ID is one the host
/// issued, dependencies form an acyclic graph, and each status is one its
/// support can carry.
pub fn validate_claims(
    case_id: &StableId,
    allowed_slots: &[ClaimSlot],
    known_evidence_ids: &BTreeSet<String>,
    claims: &[EngineeringClaim],
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut errors = Vec::new();
    for claim in claims {
        let duplicate = !ids.insert(claim.claim_id.clone());
        errors.extend(identity_error(case_id, claim, duplicate));
        if !allowed_slots.contains(&claim.slot) {
            errors.push(format!(
                "claim {} slot {:?} is not allowed by the case",
                claim.claim_id, claim.slot
            ));
        }
        errors.extend(citation_error(claim, known_evidence_ids));
    }
    if let Err(error) = validate_dependency_graph(claims) {
        errors.push(error);
    } else {
        let by_id = claims
            .iter()
            .map(|claim| (&claim.claim_id, claim))
            .collect::<BTreeMap<_, _>>();
        errors.extend(
            claims
                .iter()
                .filter_map(|claim| status_error(claim, &by_id)),
        );
    }
    errors.sort();
    errors.dedup();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn identity_error(case_id: &StableId, claim: &EngineeringClaim, duplicate: bool) -> Option<String> {
    let rebuilt = EngineeringClaim::from_draft(case_id.clone(), claim.content.clone());
    let valid = claim.schema_version == ENGINEERING_CLAIM_SCHEMA_VERSION
        && &claim.case_id == case_id
        && rebuilt.is_ok_and(|rebuilt| rebuilt.claim_id == claim.claim_id)
        && !duplicate;
    (!valid).then(|| {
        format!(
            "claim {} (slot {:?}) has an invalid or duplicate identity",
            claim.claim_id, claim.slot
        )
    })
}

fn citation_error(claim: &EngineeringClaim, known: &BTreeSet<String>) -> Option<String> {
    let unknown = claim
        .supporting_evidence_ids
        .iter()
        .chain(&claim.counter_evidence_ids)
        .filter(|id| !known.contains(*id))
        .collect::<BTreeSet<_>>();
    (!unknown.is_empty()).then(|| {
        format!(
            "claim {} (slot {:?}) cites unknown evidence: {:?}",
            claim.claim_id, claim.slot, unknown
        )
    })
}

/// A substantiated claim needs support and no counter-evidence, with every
/// dependency substantiated; a partial one needs some support; a contested
/// one needs counter-evidence or a contested dependency. Blocked and not
/// substantiated are always available to the reviewer.
fn status_error(
    claim: &EngineeringClaim,
    by_id: &BTreeMap<&StableId, &EngineeringClaim>,
) -> Option<String> {
    let dependencies = claim
        .dependency_claim_ids
        .iter()
        .filter_map(|id| by_id.get(id).map(|dependency| dependency.status))
        .collect::<BTreeSet<_>>();
    let supported = !claim.supporting_evidence_ids.is_empty() || !dependencies.is_empty();
    let countered =
        !claim.counter_evidence_ids.is_empty() || dependencies.contains(&ClaimStatus::Contested);
    let partial_dependencies = || {
        dependencies.iter().all(|status| {
            matches!(
                status,
                ClaimStatus::Substantiated | ClaimStatus::PartiallySubstantiated
            )
        })
    };
    let why = match claim.status {
        ClaimStatus::Substantiated | ClaimStatus::PartiallySubstantiated if !supported => {
            "it cites no supporting evidence and depends on no claim; cite the evidence it rests on, or mark it not_substantiated"
        }
        ClaimStatus::Substantiated if !claim.counter_evidence_ids.is_empty() => {
            "it cites counter-evidence; a claim with evidence on both sides is partially_substantiated or contested"
        }
        ClaimStatus::Substantiated
            if dependencies
                .iter()
                .any(|status| *status != ClaimStatus::Substantiated) =>
        {
            "it depends on a claim that is not substantiated; it is at most as strong as that claim, so mark it partially_substantiated"
        }
        ClaimStatus::PartiallySubstantiated if !partial_dependencies() => {
            "it depends on a claim that is contested, blocked, or not substantiated"
        }
        ClaimStatus::Contested if !countered => {
            "it cites no counter-evidence and depends on no contested claim"
        }
        _ => return None,
    };
    Some(format!(
        "claim {} (slot {:?}) status {:?} is not supported: {why}",
        claim.claim_id, claim.slot, claim.status
    ))
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;

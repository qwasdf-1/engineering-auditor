use crate::EngineeringCase;

use super::compile::{
    closure_state, record_budget_omissions, CandidateItem, Requirement, RequirementTier,
};
use super::packet::EvidencePacketSemantics;
use super::{
    AllowedEvidenceNeighborhood, EvidenceError, EvidenceOmission, EvidenceOmissionReason,
    EvidencePacket, EvidencePacketBudget, EVIDENCE_PACKET_SCHEMA_VERSION,
};

struct Admission {
    items: Vec<CandidateItem>,
    omissions: Vec<EvidenceOmission>,
    bytes: usize,
}

pub(super) fn compile_packet(
    case: &EngineeringCase,
    budget: EvidencePacketBudget,
    neighborhood: AllowedEvidenceNeighborhood,
    requirements: Vec<Requirement>,
    candidates: Vec<CandidateItem>,
) -> Result<EvidencePacket, EvidenceError> {
    let mut admission = admit_candidates(budget, &requirements, candidates)?;
    loop {
        match build_packet(case, budget, &neighborhood, &requirements, &admission) {
            Ok(packet) => return Ok(packet),
            Err(EvidenceError::InvalidPacket(detail)) if hard_ceiling_error(&detail) => {
                remove_for_hard_ceiling(&requirements, &mut admission)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn admit_candidates(
    budget: EvidencePacketBudget,
    requirements: &[Requirement],
    candidates: Vec<CandidateItem>,
) -> Result<Admission, EvidenceError> {
    let mut admission = Admission {
        items: Vec::new(),
        omissions: missing_requirement_omissions(requirements, &candidates),
        bytes: 0,
    };
    for candidate in candidates {
        admit_candidate(budget, requirements, &mut admission, candidate)?;
    }
    Ok(admission)
}

fn missing_requirement_omissions(
    requirements: &[Requirement],
    candidates: &[CandidateItem],
) -> Vec<EvidenceOmission> {
    requirements
        .iter()
        .filter(|requirement| {
            candidates
                .iter()
                .all(|candidate| !candidate.classes.contains(&requirement.class))
        })
        .map(|requirement| EvidenceOmission {
            requirement_id: requirement.id.clone(),
            item_id: None,
            reason: EvidenceOmissionReason::Unavailable,
            detail: "no typed evidence of this class is bound to the review input".to_string(),
        })
        .collect()
}

fn admit_candidate(
    budget: EvidencePacketBudget,
    requirements: &[Requirement],
    admission: &mut Admission,
    candidate: CandidateItem,
) -> Result<(), EvidenceError> {
    let item_bytes = serde_json::to_vec(&candidate.item)
        .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?
        .len();
    let required = candidate.tier == RequirementTier::Required;
    let ceiling = if required {
        budget.hard_packet_bytes
    } else {
        budget.target_packet_bytes
    };
    if admission.bytes + item_bytes <= ceiling {
        admission.bytes += item_bytes;
        admission.items.push(candidate);
    } else {
        let reason = if required {
            EvidenceOmissionReason::HardBudget
        } else {
            EvidenceOmissionReason::TargetBudget
        };
        record_budget_omissions(
            &mut admission.omissions,
            requirements,
            &candidate,
            reason,
            "typed evidence item did not fit its admission ceiling",
        );
    }
    Ok(())
}

fn build_packet(
    case: &EngineeringCase,
    budget: EvidencePacketBudget,
    neighborhood: &AllowedEvidenceNeighborhood,
    requirements: &[Requirement],
    admission: &Admission,
) -> Result<EvidencePacket, EvidenceError> {
    let (mut indexes, unavailable, closure) =
        closure_state(case, requirements, &admission.items, &admission.omissions);
    indexes.unavailable = unavailable;
    EvidencePacket::build(EvidencePacketSemantics {
        schema_version: EVIDENCE_PACKET_SCHEMA_VERSION,
        case_id: case.case_id.clone(),
        input_binding_id: case.input_binding_id.clone(),
        items: admission
            .items
            .iter()
            .map(|entry| entry.item.clone())
            .collect(),
        indexes,
        allowed_neighborhood: neighborhood.clone(),
        budget,
        closure,
        omissions: admission.omissions.clone(),
    })
}

fn hard_ceiling_error(detail: &str) -> bool {
    detail == "schema version or hard packet ceiling is invalid"
}

fn remove_for_hard_ceiling(
    requirements: &[Requirement],
    admission: &mut Admission,
) -> Result<(), EvidenceError> {
    let index = admission
        .items
        .iter()
        .enumerate()
        .max_by_key(|(_, entry)| entry.tier)
        .map(|(index, _)| index)
        .ok_or_else(|| {
            EvidenceError::InvalidBudget("hard ceiling cannot contain packet metadata".to_string())
        })?;
    let removed = admission.items.remove(index);
    record_budget_omissions(
        &mut admission.omissions,
        requirements,
        &removed,
        EvidenceOmissionReason::HardBudget,
        "item removed to keep the complete packet under its hard ceiling",
    );
    Ok(())
}

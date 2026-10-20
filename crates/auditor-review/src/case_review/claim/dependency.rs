use std::collections::{BTreeMap, BTreeSet};

use super::EngineeringClaim;
use crate::StableId;

pub(super) fn validate_dependency_graph(claims: &[EngineeringClaim]) -> Result<(), String> {
    let by_id = claims
        .iter()
        .map(|claim| (&claim.claim_id, claim))
        .collect::<BTreeMap<_, _>>();
    for claim in claims {
        for dependency_id in &claim.dependency_claim_ids {
            by_id
                .get(dependency_id)
                .ok_or_else(|| "claim has a dangling dependency".to_string())?;
        }
        visit(
            &claim.claim_id,
            &by_id,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?;
    }
    Ok(())
}

fn visit(
    id: &StableId,
    claims: &BTreeMap<&StableId, &EngineeringClaim>,
    visiting: &mut BTreeSet<StableId>,
    visited: &mut BTreeSet<StableId>,
) -> Result<(), String> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.clone()) {
        return Err("claim dependency graph is cyclic".to_string());
    }
    if let Some(claim) = claims.get(id) {
        for dependency in &claim.dependency_claim_ids {
            visit(dependency, claims, visiting, visited)?;
        }
    }
    visiting.remove(id);
    visited.insert(id.clone());
    Ok(())
}

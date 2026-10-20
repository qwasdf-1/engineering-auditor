use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ClaimSlot, ClaimStatus, ContentDigest, EngineeringClaim, EnsembleReviewerSlot, StableId,
};

use super::{group_reviews_by_key, member_values, present_reviewers};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimClusterState {
    Common,
    Compatible,
    ScopeVariant,
    MutuallyExclusive,
    MinorityOnly,
    Unresolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimCluster {
    pub slot: ClaimSlot,
    pub state: ClaimClusterState,
    pub members: BTreeMap<EnsembleReviewerSlot, Vec<StableId>>,
}

pub fn compare_claims(
    reviews: &[(EnsembleReviewerSlot, &[EngineeringClaim])],
) -> Vec<ClaimCluster> {
    let mut clusters = Vec::new();
    for (slot, claims) in group_reviews_by_key(reviews, |claim| claim.slot) {
        let present = present_reviewers(&claims);
        let statuses = member_values(&claims, |claim| claim.status);
        let (structural, dependency_scopes) = claim_shapes(&claims);
        let state = if present == 1 {
            ClaimClusterState::MinorityOnly
        } else if statuses.contains(&ClaimStatus::Contested)
            || statuses.contains(&ClaimStatus::Blocked)
        {
            ClaimClusterState::Unresolved
        } else if statuses.contains(&ClaimStatus::Substantiated)
            && statuses.contains(&ClaimStatus::NotSubstantiated)
        {
            ClaimClusterState::MutuallyExclusive
        } else if present == EnsembleReviewerSlot::ALL.len() && structural.len() == 1 {
            ClaimClusterState::Common
        } else if dependency_scopes.len() == 1 {
            ClaimClusterState::Compatible
        } else {
            ClaimClusterState::ScopeVariant
        };
        clusters.push(ClaimCluster {
            slot,
            state,
            members: claims
                .into_iter()
                .map(|(reviewer, values)| {
                    (
                        reviewer,
                        values
                            .into_iter()
                            .map(|claim| claim.claim_id.clone())
                            .collect(),
                    )
                })
                .collect(),
        });
    }
    clusters
}

fn claim_shapes(
    claims: &[(EnsembleReviewerSlot, Vec<&EngineeringClaim>)],
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut structural = BTreeSet::new();
    let mut dependencies = BTreeSet::new();
    for claim in claims.iter().flat_map(|(_, claims)| claims) {
        structural.insert(
            ContentDigest::from_canonical_value(&serde_json::json!({
                "assumptions": claim.assumptions,
                "counter_probe_ids": claim.counter_probe_ids,
                "dependency_count": claim.dependency_claim_ids.len(),
                "kind": claim.kind,
                "status": claim.status,
                "supporting_probe_ids": claim.supporting_probe_ids,
            }))
            .to_string(),
        );
        dependencies.insert(
            ContentDigest::from_canonical_value(&serde_json::json!({
                "counter_probe_ids": claim.counter_probe_ids,
                "dependency_count": claim.dependency_claim_ids.len(),
                "kind": claim.kind,
                "supporting_probe_ids": claim.supporting_probe_ids,
            }))
            .to_string(),
        );
    }
    (structural, dependencies)
}

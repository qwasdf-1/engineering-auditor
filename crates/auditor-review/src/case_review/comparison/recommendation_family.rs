use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ContentDigest, ContextualRecommendation, EnsembleReviewerSlot, RecommendationFamily, StableId,
};

use super::{group_reviews_by_key, member_values, present_reviewers};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationFamilyState {
    Common,
    CompatibleVariant,
    Contested,
    MinorityOnly,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationFamilyComparison {
    pub family: RecommendationFamily,
    pub state: RecommendationFamilyState,
    pub members: BTreeMap<EnsembleReviewerSlot, Vec<StableId>>,
    pub scope_variants: Vec<Vec<String>>,
    pub decision_dependencies: Vec<String>,
}

pub fn compare_recommendations(
    reviews: &[(EnsembleReviewerSlot, &[ContextualRecommendation])],
) -> Vec<RecommendationFamilyComparison> {
    let mut comparisons = Vec::new();
    for (family, members) in group_reviews_by_key(reviews, |item| item.family) {
        let present = present_reviewers(&members);
        let scopes = member_values(&members, |item| item.scope_ids.clone());
        let structural = member_values(&members, |item| {
            ContentDigest::from_canonical_value(&serde_json::json!({
                "alternatives": item.alternatives,
                "costs": item.costs,
                "detector_advice_relationship": item.detector_advice_relationship,
                "expected_benefits": item.expected_benefits,
                "preconditions": item.preconditions,
                "risks": item.risks,
                "scope_ids": item.scope_ids,
                "verification_steps": item.verification_steps,
            }))
            .to_string()
        });
        let state = if present == 1 {
            RecommendationFamilyState::MinorityOnly
        } else if present < EnsembleReviewerSlot::ALL.len() {
            RecommendationFamilyState::Contested
        } else if structural.len() == 1 {
            RecommendationFamilyState::Common
        } else {
            RecommendationFamilyState::CompatibleVariant
        };
        let decision_dependencies = (scopes.len() > 1)
            .then(|| "Select the ownership/scope boundary before choosing a variant.".to_string())
            .into_iter()
            .collect();
        comparisons.push(RecommendationFamilyComparison {
            family,
            state,
            members: members
                .into_iter()
                .map(|(reviewer, values)| {
                    (
                        reviewer,
                        values
                            .into_iter()
                            .map(|item| item.recommendation_id.clone())
                            .collect(),
                    )
                })
                .collect(),
            scope_variants: scopes.into_iter().collect(),
            decision_dependencies,
        });
    }
    comparisons
}

pub fn validate_comparison_recommendation_families(
    comparison: &[RecommendationFamilyComparison],
    candidates: &[ContextualRecommendation],
) -> Result<(), String> {
    let allowed = comparison
        .iter()
        .map(|family| family.family)
        .collect::<BTreeSet<_>>();
    if candidates
        .iter()
        .any(|item| !allowed.contains(&item.family))
    {
        return Err("challenger or response introduced a new recommendation family".to_string());
    }
    Ok(())
}

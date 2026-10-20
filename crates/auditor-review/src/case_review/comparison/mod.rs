mod claim_cluster;
mod outcome;
mod probe_matrix;
mod recommendation_family;

pub use claim_cluster::{compare_claims, ClaimCluster, ClaimClusterState};
pub use outcome::{
    compare_ensemble_run, ChallengeTarget, ComparisonTargetType, EnsembleComparison,
    MaterialDisagreement, ReviewComparisonOutcome, ENSEMBLE_COMPARISON_SCHEMA_VERSION,
};
pub use probe_matrix::{compare_probe_results, ProbeComparison, ProbeComparisonState};
pub use recommendation_family::{
    compare_recommendations, validate_comparison_recommendation_families,
    RecommendationFamilyComparison, RecommendationFamilyState,
};

/// Reviewers in a group that contributed at least one item.
fn present_reviewers<T>(members: &[(EnsembleReviewerSlot, Vec<&T>)]) -> usize {
    members
        .iter()
        .filter(|(_, values)| !values.is_empty())
        .count()
}

/// The distinct `value` of every item in a group.
fn member_values<T, V: Ord>(
    members: &[(EnsembleReviewerSlot, Vec<&T>)],
    value: impl Fn(&T) -> V,
) -> BTreeSet<V> {
    members
        .iter()
        .flat_map(|(_, values)| values.iter().map(|item| value(item)))
        .collect()
}

fn group_reviews_by_key<'a, T, K>(
    reviews: &[(EnsembleReviewerSlot, &'a [T])],
    key: impl Fn(&T) -> K + Copy,
) -> BTreeMap<K, Vec<(EnsembleReviewerSlot, Vec<&'a T>)>>
where
    K: Copy + Ord,
{
    let keys = reviews
        .iter()
        .flat_map(|(_, items)| items.iter().map(key))
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .map(|selected| {
            let members = reviews
                .iter()
                .map(|(reviewer, items)| {
                    (
                        *reviewer,
                        items.iter().filter(|item| key(item) == selected).collect(),
                    )
                })
                .collect();
            (selected, members)
        })
        .collect()
}
use std::collections::{BTreeMap, BTreeSet};

use crate::EnsembleReviewerSlot;

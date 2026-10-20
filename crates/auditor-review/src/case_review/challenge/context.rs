use std::collections::BTreeMap;

use crate::{ReviewerCaseReview, StableId};

use super::ChallengeTargetType;

pub(crate) fn challenge_targets(
    review: &ReviewerCaseReview,
) -> BTreeMap<StableId, ChallengeTargetType> {
    review
        .claims
        .iter()
        .map(|claim| (claim.claim_id.clone(), ChallengeTargetType::Claim))
        .chain(review.recommendations.iter().map(|recommendation| {
            (
                recommendation.recommendation_id.clone(),
                ChallengeTargetType::Recommendation,
            )
        }))
        .collect()
}

pub(crate) fn reviewer_cited_probe_ids(
    review: &ReviewerCaseReview,
) -> impl Iterator<Item = String> + '_ {
    review
        .claims
        .iter()
        .flat_map(|claim| {
            claim
                .supporting_probe_ids
                .iter()
                .chain(&claim.counter_probe_ids)
        })
        .chain(review.recommendations.iter().flat_map(|recommendation| {
            recommendation
                .expected_benefits
                .iter()
                .flat_map(|benefit| &benefit.evidence_probe_ids)
                .chain(
                    recommendation
                        .verification_steps
                        .iter()
                        .flat_map(|step| &step.evidence_probe_ids),
                )
        }))
        .map(ToString::to_string)
}

pub(crate) fn unknown_identifier_reason<T: Ord + ToString>(
    identifiers: &[T],
    allowed: &std::collections::BTreeSet<T>,
    prefix: &str,
    suffix: &str,
) -> Option<String> {
    let unknown = identifiers
        .iter()
        .filter(|identifier| !allowed.contains(*identifier))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    (!unknown.is_empty()).then(|| format!("{prefix}{unknown:?}{suffix}"))
}

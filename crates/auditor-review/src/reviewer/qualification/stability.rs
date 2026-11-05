//! Agreement between runs on one input. Questions are deterministic, so
//! two runs ask the same ones; for each pair of runs, every hotspot
//! question's top-ranked subjects and the plan's top steps (as the
//! findings they resolve, since step IDs are content-derived) must overlap
//! by at least the policy's share. A question one run answered and the
//! other did not is a disagreement too.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::super::driver::EngineeringReview;
use super::super::questions::{QuestionAnswer, QuestionKind};
use super::LISTED_FAILURES;

/// How much two runs must agree. Provisional until Phase B measures real
/// runs.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StabilityPolicy {
    /// How many top hotspots, and top plan steps, are compared.
    pub top: usize,
    /// The share of them two runs must have in common.
    pub min_overlap: f64,
}

impl Default for StabilityPolicy {
    fn default() -> Self {
        Self {
            top: 3,
            min_overlap: 0.5,
        }
    }
}

/// What one ranking comparison found in common.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RankingOverlap {
    pub question_id: String,
    pub shared: usize,
    pub compared: usize,
}

impl RankingOverlap {
    fn holds(&self, policy: StabilityPolicy) -> bool {
        self.shared as f64 >= policy.min_overlap * self.compared as f64
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PairStability {
    /// Indexes of the two runs.
    pub runs: [usize; 2],
    pub hotspots: Vec<RankingOverlap>,
    /// Findings resolved by each plan's top steps, in common over all.
    pub plan: Option<RankingOverlap>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StabilityResult {
    pub policy: StabilityPolicy,
    pub pairs: Vec<PairStability>,
    pub passed: bool,
    pub failures: Vec<String>,
}

pub(super) fn compare(reviews: &[&EngineeringReview], policy: StabilityPolicy) -> StabilityResult {
    let mut pairs = Vec::new();
    let mut failures = Vec::new();
    if reviews.len() < 2 {
        failures.push("stability needs two runs on one input".to_string());
    }
    for first in 0..reviews.len() {
        for second in first + 1..reviews.len() {
            let pair = compare_pair([first, second], reviews[first], reviews[second], policy);
            failures.extend(pair_failures(&pair, reviews, policy));
            pairs.push(pair);
        }
    }
    failures.truncate(LISTED_FAILURES);
    StabilityResult {
        policy,
        passed: failures.is_empty(),
        pairs,
        failures,
    }
}

fn compare_pair(
    runs: [usize; 2],
    first: &EngineeringReview,
    second: &EngineeringReview,
    policy: StabilityPolicy,
) -> PairStability {
    let mut hotspots = Vec::new();
    let mut plan = None;
    for (kind, left, right) in answered_in_both(first, second) {
        let question_id = left.question_id.as_str().to_string();
        match kind {
            QuestionKind::HotspotAssessment => {
                let compared = policy.top.min(left.ranking.len()).min(right.ranking.len());
                let right = right.ranking[..compared].iter().collect::<BTreeSet<_>>();
                hotspots.push(RankingOverlap {
                    question_id,
                    shared: left.ranking[..compared]
                        .iter()
                        .filter(|id| right.contains(id))
                        .count(),
                    compared,
                });
            }
            QuestionKind::RemediationPlan => {
                let (left, right) = (
                    top_findings(left, policy.top),
                    top_findings(right, policy.top),
                );
                plan = Some(RankingOverlap {
                    question_id,
                    shared: left.intersection(&right).count(),
                    compared: left.union(&right).count(),
                });
            }
            QuestionKind::SharedRootCause | QuestionKind::ArchitectureReview => {}
        }
    }
    PairStability {
        runs,
        hotspots,
        plan,
    }
}

fn pair_failures(
    pair: &PairStability,
    reviews: &[&EngineeringReview],
    policy: StabilityPolicy,
) -> Vec<String> {
    let [first, second] = pair.runs;
    let label = format!("runs {first} and {second}");
    let mut failures = one_sided(reviews[first], reviews[second])
        .into_iter()
        .map(|question| format!("{label}: only one answered {question}"))
        .collect::<Vec<_>>();
    if pair.hotspots.is_empty() && pair.plan.is_none() {
        failures.push(format!("{label}: no hotspot or plan answer to compare"));
    }
    failures.extend(
        pair.hotspots
            .iter()
            .chain(&pair.plan)
            .filter(|overlap| !overlap.holds(policy))
            .map(|overlap| {
                format!(
                    "{label}: {} shares {} of {}",
                    overlap.question_id, overlap.shared, overlap.compared
                )
            }),
    );
    failures
}

fn answered_in_both<'r>(
    first: &'r EngineeringReview,
    second: &'r EngineeringReview,
) -> Vec<(QuestionKind, &'r QuestionAnswer, &'r QuestionAnswer)> {
    first
        .questions
        .iter()
        .filter_map(|record| {
            let left = record.answer.as_ref()?;
            let right = second
                .questions
                .iter()
                .find(|other| other.question_id == record.question_id)?
                .answer
                .as_ref()?;
            Some((record.kind, left, right))
        })
        .collect()
}

/// Questions both runs asked and exactly one answered.
fn one_sided(first: &EngineeringReview, second: &EngineeringReview) -> Vec<String> {
    first
        .questions
        .iter()
        .filter_map(|record| {
            let other = second
                .questions
                .iter()
                .find(|other| other.question_id == record.question_id)?;
            (record.answer.is_some() != other.answer.is_some())
                .then(|| record.question_id.as_str().to_string())
        })
        .collect()
}

/// The findings the plan's first `top` steps, in ranking order, resolve.
fn top_findings(plan: &QuestionAnswer, top: usize) -> BTreeSet<&str> {
    plan.ranking
        .iter()
        .take(top)
        .filter_map(|id| {
            plan.recommendations
                .iter()
                .find(|step| step.recommendation_id.as_str() == id)
        })
        .flat_map(|step| step.affected_finding_ids.iter().map(String::as_str))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlap_holds_at_the_policy_share() {
        let policy = StabilityPolicy::default();
        let overlap = |shared, compared| RankingOverlap {
            question_id: String::new(),
            shared,
            compared,
        };
        assert!(overlap(2, 3).holds(policy));
        assert!(!overlap(1, 3).holds(policy));
        assert!(overlap(0, 0).holds(policy));
    }
}

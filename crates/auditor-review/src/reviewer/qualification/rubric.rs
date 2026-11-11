//! The usefulness rubric (`contracts/review-usefulness-v1.schema.json`):
//! one item per answered claim and recommendation, scored correct,
//! specific, actionable, and by its novelty against the report: it restates
//! a finding, adds detail that follows from the report and the code, or says
//! what the report cannot (a diagnosis, a better fix, which findings are
//! benign). An item is
//! useful when it is correct, specific, and actionable and does not merely
//! restate a finding. As with the
//! precision labels, a model's score is a pre-label until a second,
//! different model agrees (corroborated) or a person scores it (final);
//! scorers disagreeing is a dispute only a person settles. No scoring
//! model may be one the review ran on, and the sheet does not name them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::super::driver::{wire_name as wire, EngineeringReview};
use super::super::questions::QuestionKind;

pub const USEFULNESS_SHEET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Claim,
    Recommendation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsefulnessStatus {
    Pending,
    PreLabel,
    Corroborated,
    Final,
}

/// What an item adds to the report it reviews.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Novelty {
    /// A finding, or a finding's own recommendation, says it already.
    Restates,
    /// Detail that follows from the report and a look at the code: where
    /// and how to apply the report's own fix, or its facts assembled.
    AddsDetail,
    /// What the report cannot say: why a finding exists or how much it
    /// matters, a fix better than the report's, or which findings are benign.
    BeyondReport,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsefulnessScores {
    pub correct: bool,
    pub specific: bool,
    pub actionable: bool,
    pub novelty: Novelty,
}

impl UsefulnessScores {
    fn useful(self) -> bool {
        self.correct && self.specific && self.actionable && self.novelty != Novelty::Restates
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreCheck {
    /// `human:<id>` or `model:<id>`.
    pub scorer: String,
    pub scores: UsefulnessScores,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsefulnessItem {
    pub item_id: String,
    pub question_id: String,
    pub question_kind: QuestionKind,
    pub item: ItemKind,
    /// What the scorer judges: the claim or the recommendation as stated.
    pub text: String,
    pub evidence_ids: Vec<String>,
    /// The patch checker's verdict, for a recommendation with a patch.
    #[serde(default)]
    pub patch_verdict: Option<String>,
    pub status: UsefulnessStatus,
    #[serde(default)]
    pub scores: Option<UsefulnessScores>,
    #[serde(default)]
    pub scorer: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub checks: Vec<ScoreCheck>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsefulnessSheet {
    pub schema_version: u32,
    /// The scored review's [`EngineeringReview::digest`].
    pub review_digest: String,
    pub source_digest: String,
    pub items: Vec<UsefulnessItem>,
}

/// How the sheets' items stand, and how many counted items are useful.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsefulnessSummary {
    pub sheets: usize,
    pub items: usize,
    pub pending: usize,
    pub pre_label: usize,
    /// Pre-labels a second scorer disagreed with, waiting for a person.
    pub disputed: usize,
    pub corroborated: usize,
    pub final_scores: usize,
    /// Corroborated and final items: the ones the score counts.
    pub counted: usize,
    pub useful: usize,
    pub correct: usize,
    pub specific: usize,
    pub actionable: usize,
    pub adds_detail: usize,
    pub beyond_report: usize,
    /// Useful items that go beyond the report.
    pub useful_beyond_report: usize,
}

/// An unscored sheet for one review.
pub fn usefulness_sheet(review: &EngineeringReview) -> UsefulnessSheet {
    let mut items = Vec::new();
    for record in &review.questions {
        let Some(answer) = &record.answer else {
            continue;
        };
        let item = |item_id: &str, item, text: String, evidence_ids: Vec<String>| UsefulnessItem {
            item_id: item_id.to_string(),
            question_id: record.question_id.as_str().to_string(),
            question_kind: record.kind,
            item,
            text,
            evidence_ids,
            patch_verdict: None,
            status: UsefulnessStatus::Pending,
            scores: None,
            scorer: None,
            reason: None,
            checks: Vec::new(),
        };
        for claim in &answer.claims {
            items.push(item(
                claim.claim_id.as_str(),
                ItemKind::Claim,
                format!(
                    "{} ({}): {}",
                    wire(&claim.slot),
                    wire(&claim.status),
                    claim.proposition
                ),
                claim.supporting_evidence_ids.clone(),
            ));
        }
        for step in &answer.recommendations {
            let mut entry = item(
                step.recommendation_id.as_str(),
                ItemKind::Recommendation,
                format!("{}: {}", wire(&step.family), step.summary),
                // What it acts on, which for a module cycle or a co-change
                // pair is no finding, and the findings it resolves.
                step.scope_ids
                    .iter()
                    .chain(&step.affected_finding_ids)
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            );
            entry.patch_verdict = step.patch.as_ref().map(|_| {
                review
                    .patch_checks
                    .iter()
                    .rev()
                    .find(|check| check.recommendation_id.as_ref() == Some(&step.recommendation_id))
                    .and_then(|check| check.check["verdict"].as_str())
                    .unwrap_or("unchecked")
                    .to_string()
            });
            items.push(entry);
        }
    }
    UsefulnessSheet {
        schema_version: USEFULNESS_SHEET_SCHEMA_VERSION,
        review_digest: review.digest(),
        source_digest: review.source_digest.clone(),
        items,
    }
}

/// Checks each sheet against the review it scores and counts its items.
/// A sheet for no given review, with other items than that review's, or
/// with an item scored against the rules is an error.
pub(super) fn summarize(
    sheets: &[UsefulnessSheet],
    reviews: &[&EngineeringReview],
) -> Result<UsefulnessSummary, String> {
    let mut summary = UsefulnessSummary::default();
    for sheet in sheets {
        let review = reviews
            .iter()
            .find(|review| review.digest() == sheet.review_digest)
            .ok_or("a usefulness sheet scores a review that was not given")?;
        let blank = usefulness_sheet(review);
        let listed = |items: &[UsefulnessItem]| {
            items
                .iter()
                .map(|item| (item.item_id.clone(), item.text.clone()))
                .collect::<Vec<_>>()
        };
        if sheet.schema_version != USEFULNESS_SHEET_SCHEMA_VERSION
            || listed(&sheet.items) != listed(&blank.items)
        {
            return Err(format!(
                "the usefulness sheet for {} does not list that review's items as stated",
                sheet.review_digest
            ));
        }
        let reviewers = reviewer_models(review);
        summary.sheets += 1;
        for item in &sheet.items {
            count_item(&mut summary, item, &reviewers)
                .map_err(|error| format!("usefulness item {}: {error}", item.item_id))?;
        }
    }
    Ok(summary)
}

fn count_item(
    summary: &mut UsefulnessSummary,
    item: &UsefulnessItem,
    reviewers: &BTreeSet<String>,
) -> Result<(), String> {
    summary.items += 1;
    let scorers = item
        .scorer
        .iter()
        .chain(item.checks.iter().map(|check| &check.scorer));
    for scorer in scorers {
        let model = scorer.strip_prefix("model:");
        if !(scorer.starts_with("human:") || model.is_some()) || scorer.ends_with(':') {
            return Err(format!(
                "scorer `{scorer}` is neither human:<id> nor model:<id>"
            ));
        }
        if model.is_some_and(|model| reviewers.contains(model)) {
            return Err(format!("{scorer} ran the review, so it cannot score it"));
        }
    }
    let disagree =
        |scores: UsefulnessScores| item.checks.iter().any(|check| check.scores != scores);
    let model_scorer = item
        .scorer
        .as_deref()
        .filter(|scorer| scorer.starts_with("model:"));
    let scores = match (item.status, item.scores, model_scorer) {
        (UsefulnessStatus::Pending, None, None) if item.scorer.is_none() && item.checks.is_empty() => {
            summary.pending += 1;
            return Ok(());
        }
        (UsefulnessStatus::PreLabel, Some(scores), Some(_)) => {
            summary.pre_label += 1;
            summary.disputed += usize::from(disagree(scores));
            return Ok(());
        }
        (UsefulnessStatus::Corroborated, Some(scores), Some(scorer))
            if !disagree(scores)
                && item.checks.iter().any(|check| {
                    check.scorer.starts_with("model:") && check.scorer != scorer
                }) =>
        {
            summary.corroborated += 1;
            scores
        }
        (UsefulnessStatus::Final, Some(scores), None)
            if item.scorer.as_deref().is_some_and(|scorer| scorer.starts_with("human:")) =>
        {
            summary.final_scores += 1;
            scores
        }
        (status, ..) => {
            return Err(format!(
                "status {status:?} needs: pending no score; pre_label a model's score; corroborated a model's score and an agreeing check by another model, with no disagreeing check; final a person's score"
            ))
        }
    };
    summary.counted += 1;
    summary.useful += usize::from(scores.useful());
    summary.correct += usize::from(scores.correct);
    summary.specific += usize::from(scores.specific);
    summary.actionable += usize::from(scores.actionable);
    summary.adds_detail += usize::from(scores.novelty == Novelty::AddsDetail);
    let beyond = scores.novelty == Novelty::BeyondReport;
    summary.beyond_report += usize::from(beyond);
    summary.useful_beyond_report += usize::from(beyond && scores.useful());
    Ok(())
}

/// The models the review ran on, as `<model>` and `<provider>/<model>`.
fn reviewer_models(review: &EngineeringReview) -> BTreeSet<String> {
    review
        .calls
        .iter()
        .flat_map(|call| {
            [
                call.target.model.clone(),
                format!("{}/{}", call.target.provider_id, call.target.model),
            ]
        })
        .collect()
}

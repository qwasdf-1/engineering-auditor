//! Qualification: holds recorded reviews to the automatic gates, offline.
//! It reads the review records and the report, map, and history they
//! read, never a provider or the repository tree, so it runs after the
//! checkout is gone. Each run must be bound to the inputs given, answer
//! every question, cite only evidence it was shown and the inputs hold,
//! not misstate a cited finding, have every patched recommendation
//! checked, and stay within its budgets; runs on one input must agree on
//! the top hotspots and plan steps. Usefulness is scored separately, on
//! [`UsefulnessSheet`]s, and summarized beside the gates.

mod citations;
mod resolve;
mod rubric;
mod spend;
mod stability;

use serde::{Deserialize, Serialize};

use super::driver::{canonical_digest, EngineeringReview, QuestionStatus};
use super::questions::Question;
use super::view::RepositoryView;
use super::{OrientationPack, QuestionTemplateBundle};
use crate::{ReviewBrief, ReviewPromptBundleV2, ReviewRoutesV2, ENGINEERING_REVIEW_SCHEMA_VERSION};

pub use rubric::{
    usefulness_sheet, ItemKind, Novelty, ScoreCheck, UsefulnessItem, UsefulnessScores,
    UsefulnessSheet, UsefulnessStatus, UsefulnessSummary, USEFULNESS_SHEET_SCHEMA_VERSION,
};
pub use stability::{PairStability, RankingOverlap, StabilityPolicy, StabilityResult};

pub const REVIEW_QUALIFICATION_SCHEMA_VERSION: u32 = 1;
/// Failures one gate lists before counting the rest.
const LISTED_FAILURES: usize = 20;

/// What the runs are checked against: the inputs and configuration a
/// review of this repository should have used.
pub struct QualificationInputs<'a> {
    pub view: &'a RepositoryView<'a>,
    pub brief: Option<&'a ReviewBrief>,
    pub templates: &'a QuestionTemplateBundle,
    pub prompts: &'a ReviewPromptBundleV2,
    pub routes: &'a ReviewRoutesV2,
}

/// One recorded review and the name it is reported under (its path).
pub struct RecordedReview {
    pub label: String,
    pub review: EngineeringReview,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// The record names these inputs, configuration, pack, and questions.
    Binding,
    /// Every question was answered.
    Coverage,
    /// Every answer still passes its question's check, and every cited or
    /// issued evidence ID resolves in the inputs.
    Citations,
    /// No claim cites evidence both ways or misstates a cited finding's
    /// measured value.
    Contradictions,
    /// Every recommendation with a patch was verified or refuted.
    PatchChecks,
    /// The recorded spend adds up and stays within every budget.
    Budget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateResult {
    pub gate: Gate,
    pub passed: bool,
    pub failures: Vec<String>,
}

impl GateResult {
    fn new(gate: Gate, mut failures: Vec<String>) -> Self {
        if failures.len() > LISTED_FAILURES {
            let more = failures.len() - LISTED_FAILURES;
            failures.truncate(LISTED_FAILURES);
            failures.push(format!("{more} more"));
        }
        Self {
            gate,
            passed: failures.is_empty(),
            failures,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunQualification {
    pub label: String,
    pub review_digest: String,
    pub questions: usize,
    pub answered: usize,
    pub gates: Vec<GateResult>,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewQualification {
    pub schema_version: u32,
    /// The report, map, and history digest the runs were checked against.
    pub source_digest: String,
    pub runs: Vec<RunQualification>,
    pub stability: StabilityResult,
    /// Present when usefulness sheets were given.
    pub usefulness: Option<UsefulnessSummary>,
    /// Every run passed every gate, and the runs agree.
    pub passed: bool,
}

/// Rebuilt once for all runs: what the review showed before any tool.
struct Expected {
    pack: OrientationPack,
    questions: Vec<Question>,
}

/// Checks each run, compares the runs, and summarizes the sheets. An
/// error means the inputs themselves are unusable (the pack or a question
/// cannot be rebuilt, or a sheet is malformed or for another review);
/// everything about the runs is a gate result.
pub fn qualify_reviews(
    inputs: &QualificationInputs<'_>,
    runs: &[RecordedReview],
    policy: StabilityPolicy,
    sheets: &[UsefulnessSheet],
) -> Result<ReviewQualification, String> {
    let expected = Expected {
        pack: inputs.view.orientation_pack(inputs.brief)?,
        questions: inputs.view.questions(inputs.templates)?,
    };
    let runs_checked = runs
        .iter()
        .map(|run| qualify_run(inputs, &expected, run))
        .collect::<Vec<_>>();
    let reviews = runs.iter().map(|run| &run.review).collect::<Vec<_>>();
    let stability = stability::compare(&reviews, policy);
    let usefulness = if sheets.is_empty() {
        None
    } else {
        Some(rubric::summarize(sheets, &reviews)?)
    };
    let passed =
        !runs_checked.is_empty() && runs_checked.iter().all(|run| run.passed) && stability.passed;
    Ok(ReviewQualification {
        schema_version: REVIEW_QUALIFICATION_SCHEMA_VERSION,
        source_digest: inputs.view.source_digest.clone(),
        runs: runs_checked,
        stability,
        usefulness,
        passed,
    })
}

fn qualify_run(
    inputs: &QualificationInputs<'_>,
    expected: &Expected,
    run: &RecordedReview,
) -> RunQualification {
    let review = &run.review;
    let gates = vec![
        GateResult::new(Gate::Binding, binding_failures(inputs, expected, review)),
        GateResult::new(Gate::Coverage, coverage_failures(review)),
        GateResult::new(
            Gate::Citations,
            citations::citation_failures(inputs.view, &expected.pack, &expected.questions, review),
        ),
        GateResult::new(
            Gate::Contradictions,
            citations::contradiction_failures(inputs.view, review),
        ),
        GateResult::new(Gate::PatchChecks, spend::patch_check_failures(review)),
        GateResult::new(
            Gate::Budget,
            spend::budget_failures(inputs.routes, inputs.brief, review),
        ),
    ];
    RunQualification {
        label: run.label.clone(),
        review_digest: review.digest(),
        questions: review.questions.len(),
        answered: review.answered().count(),
        passed: gates.iter().all(|gate| gate.passed),
        gates,
    }
}

/// The record must name the inputs and configuration given, and show the
/// same pack and questions they rebuild to.
fn binding_failures(
    inputs: &QualificationInputs<'_>,
    expected: &Expected,
    review: &EngineeringReview,
) -> Vec<String> {
    let brief_digest = inputs.brief.map(canonical_digest);
    let checks = [
        (
            review.schema_version == ENGINEERING_REVIEW_SCHEMA_VERSION,
            "the record's schema version is not the current one",
        ),
        (
            review.source_digest == inputs.view.source_digest,
            "the review read another report, map, or history digest",
        ),
        (
            review.template_bundle_digest == inputs.templates.digest().as_str(),
            "the review used other question templates",
        ),
        (
            review.prompt_bundle_id == inputs.prompts.bundle_id,
            "the review used another prompt bundle",
        ),
        (
            review.routes_digest == canonical_digest(inputs.routes),
            "the review ran with other routes",
        ),
        (
            review.brief_digest == brief_digest,
            "the review ran with another brief, or with none",
        ),
        (
            review.orientation.markdown_digest == expected.pack.audit.markdown_digest,
            "the recorded orientation pack is not the one these inputs build",
        ),
    ];
    let mut failures = checks
        .into_iter()
        .filter(|(holds, _)| !holds)
        .map(|(_, failure)| failure.to_string())
        .collect::<Vec<_>>();
    let recorded = review
        .questions
        .iter()
        .map(|record| {
            (
                &record.question_id,
                &record.markdown_digest,
                &record.depends_on,
            )
        })
        .collect::<Vec<_>>();
    let rebuilt = expected
        .questions
        .iter()
        .map(|question| {
            (
                &question.question_id,
                &question.audit.markdown_digest,
                &question.depends_on,
            )
        })
        .collect::<Vec<_>>();
    if recorded != rebuilt {
        failures.push(format!(
            "the review asked {} questions that differ from the {} these inputs build",
            recorded.len(),
            rebuilt.len()
        ));
    }
    failures
}

fn coverage_failures(review: &EngineeringReview) -> Vec<String> {
    review
        .questions
        .iter()
        .filter(|record| record.status != QuestionStatus::Answered || record.answer.is_none())
        .map(|record| {
            format!(
                "{} was not answered: {}",
                record.question_id.as_str(),
                record.reason.as_deref().unwrap_or("no reason recorded")
            )
        })
        .collect()
}

//! What a review records: each question with its answer or why it has
//! none, every provider call and retrieval, the patch checks, and what the
//! review spent.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ContentDigest, ProviderFailureClass, QuestionAnswer, QuestionKind, RetrievalRecord,
    RouteTarget, StableId, Usage,
};

pub const ENGINEERING_REVIEW_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringReview {
    pub schema_version: u32,
    /// The report, map, and history digest the review read.
    pub source_digest: String,
    pub template_bundle_digest: String,
    pub prompt_bundle_id: StableId,
    /// The routes and the brief the review ran with, by canonical digest,
    /// so qualification can hold the record to the same configuration.
    pub routes_digest: String,
    pub brief_digest: Option<String>,
    /// Patch checks the review was allowed.
    pub max_patch_checks: usize,
    pub orientation: ProjectionSummary,
    pub questions: Vec<QuestionRecord>,
    pub patch_checks: Vec<PatchCheckRecord>,
    pub retrieval: Vec<RetrievalRecord>,
    pub calls: Vec<ReviewCall>,
    pub spend: Spend,
    /// Why the review stopped before its last question, if it did.
    pub stopped: Option<String>,
}

impl EngineeringReview {
    pub fn answered(&self) -> impl Iterator<Item = &QuestionAnswer> {
        self.questions
            .iter()
            .filter_map(|question| question.answer.as_ref())
    }

    /// The record's canonical digest, which a usefulness sheet binds.
    pub fn digest(&self) -> String {
        canonical_digest(self)
    }
}

/// A configuration value's digest over its canonical JSON.
pub fn canonical_digest(value: &impl Serialize) -> String {
    ContentDigest::from_canonical_value(&serde_json::to_value(value).expect("serializable"))
        .as_str()
        .to_string()
}

/// A rendered model-visible document, by digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionSummary {
    pub markdown_digest: String,
    pub markdown_bytes: usize,
    pub evidence_ids: usize,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionStatus {
    Answered,
    Unanswered,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRecord {
    pub question_id: StableId,
    pub kind: QuestionKind,
    pub template_version: u32,
    pub subject_ids: Vec<String>,
    pub finding_count: usize,
    pub depends_on: Vec<StableId>,
    pub markdown_digest: String,
    pub status: QuestionStatus,
    /// Why an unanswered question has no answer.
    pub reason: Option<String>,
    /// The route target that gave the accepted answer.
    pub target: Option<RouteTarget>,
    pub requests: usize,
    /// Each rejected answer's reasons, in order.
    pub rejected_answers: Vec<String>,
    pub answer: Option<QuestionAnswer>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckRequester {
    /// The reviewer called `check_patch` while answering.
    Reviewer,
    /// The host checked an accepted recommendation's patch.
    Host,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchCheckRecord {
    pub question_id: StableId,
    pub requested_by: CheckRequester,
    pub recommendation_id: Option<StableId>,
    pub cited_findings: Vec<String>,
    /// The checker's verdict record (`review check-patch` output).
    pub check: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCall {
    pub question_id: StableId,
    pub request_id: String,
    pub target: RouteTarget,
    /// Retries of this request after a failure that produced no model
    /// output.
    pub retry: u8,
    pub input_bytes: usize,
    pub usage: Usage,
    pub finish_reason: Option<String>,
    pub failure: Option<ProviderFailureClass>,
    pub failure_detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spend {
    pub requests: usize,
    pub input_bytes: usize,
    pub input_tokens: usize,
    pub cached_input_tokens: usize,
    pub output_tokens: usize,
    pub cost_microusd: u64,
    pub patch_checks: usize,
}

impl Spend {
    pub(super) fn add_request(&mut self, input_bytes: usize, cost_microusd: u64) {
        self.requests += 1;
        self.input_bytes += input_bytes;
        self.cost_microusd += cost_microusd;
    }

    pub(super) fn add_usage(&mut self, usage: Usage) {
        self.input_tokens += usage.input_tokens;
        self.cached_input_tokens += usage.cached_input_tokens;
        self.output_tokens += usage.output_tokens;
    }
}

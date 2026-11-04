//! Questions: what the reviewer is asked after the orientation pack. A
//! checked-in template gives each kind's wording, limits, and the claim
//! slots and recommendation families its answer may use; a trigger over the
//! report, map, and history picks the subjects; and the answer check holds
//! an answer to its kind's shape.

mod answer;
mod triggers;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::document::{BoundedDocument, Item, Row, Section};
use super::view::RepositoryView;
use crate::{ClaimSlot, ContentDigest, ModelProjectionAudit, RecommendationFamily, StableId};

pub use answer::QuestionAnswer;

pub const QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION: u32 = 1;
/// Bumped when a question's identity fields change.
pub const QUESTION_SCHEMA_VERSION: u32 = 1;
/// Questions share the case prefix, so their answers are claim and
/// recommendation records like any case's.
const QUESTION_ID_PREFIX: &str = "engineering-case";
/// Bytes one question's Markdown may take; a question whose subjects do
/// not fit is an error, never silently cut.
const QUESTION_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    HotspotAssessment,
    SharedRootCause,
    ArchitectureReview,
    RemediationPlan,
}

impl QuestionKind {
    /// The order a review asks them in: the plan last, so it can build on
    /// the other answers.
    pub const ALL: [Self; 4] = [
        Self::HotspotAssessment,
        Self::SharedRootCause,
        Self::ArchitectureReview,
        Self::RemediationPlan,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::HotspotAssessment => "Hotspot assessment",
            Self::SharedRootCause => "Shared root cause",
            Self::ArchitectureReview => "Architecture review",
            Self::RemediationPlan => "Remediation plan",
        }
    }

    /// What the answer's `ranking` must hold.
    fn ranking_rule(self) -> &'static str {
        match self {
            Self::HotspotAssessment => "every subject ID once, most risky first",
            Self::RemediationPlan => "every recommendation ID once, in the order to carry them out",
            Self::SharedRootCause | Self::ArchitectureReview => "empty",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionTemplate {
    pub kind: QuestionKind,
    pub version: u32,
    /// How many questions of this kind one review asks.
    pub max_questions: usize,
    /// How many subjects one question names.
    pub max_subjects: usize,
    pub question: String,
    pub claim_slots: Vec<ClaimSlot>,
    pub recommendation_families: Vec<RecommendationFamily>,
}

impl QuestionTemplate {
    fn is_valid(&self) -> bool {
        let plan = self.kind == QuestionKind::RemediationPlan;
        self.version > 0
            && self.max_questions > 0
            && (self.max_subjects == 0) == plan
            && (!plan || self.max_questions == 1)
            && !self.question.trim().is_empty()
            && !self.claim_slots.is_empty()
            && !self.recommendation_families.is_empty()
            && distinct(&self.claim_slots)
            && distinct(&self.recommendation_families)
    }
}

fn distinct<T: Ord>(values: &[T]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionTemplateBundle {
    pub schema_version: u32,
    pub bundle_version: u32,
    pub templates: Vec<QuestionTemplate>,
}

impl QuestionTemplateBundle {
    pub fn checked_in() -> Result<Self, String> {
        Self::from_json(include_str!(
            "../../../../../config/auditor/review-question-templates-v1.json"
        ))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let bundle: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid question template bundle: {error}"))?;
        bundle.validate()?;
        Ok(bundle)
    }

    /// One template per kind, in [`QuestionKind::ALL`] order, each with
    /// wording, slots, and families, and subjects for every kind but the
    /// plan, which reads the other answers instead.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION {
            return Err(format!(
                "question template bundle schema_version {} is not {QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let kinds = self
            .templates
            .iter()
            .map(|template| template.kind)
            .collect::<Vec<_>>();
        if kinds != QuestionKind::ALL {
            return Err(format!(
                "question templates must be {:?} in that order, found {kinds:?}",
                QuestionKind::ALL
            ));
        }
        match self.templates.iter().find(|template| !template.is_valid()) {
            Some(template) => Err(format!(
                "question template {:?} needs a version, one plan or at least one question, subjects unless it is the plan, wording, and distinct slots and families",
                template.kind
            )),
            None => Ok(()),
        }
    }

    pub fn digest(&self) -> ContentDigest {
        ContentDigest::from_canonical_value(
            &serde_json::to_value(self).expect("bundle is serializable"),
        )
    }

    pub fn template(&self, kind: QuestionKind) -> &QuestionTemplate {
        self.templates
            .iter()
            .find(|template| template.kind == kind)
            .expect("a validated bundle has every kind")
    }
}

/// What a question is about: a file, a symbol, a module cycle, or a
/// finding, with the evidence IDs its row shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuestionSubject {
    pub id: String,
    pub summary: String,
    pub evidence_ids: Vec<String>,
}

impl QuestionSubject {
    fn from_row(row: Row) -> Self {
        Self {
            id: row.ids[0].clone(),
            summary: row.text,
            evidence_ids: row.ids,
        }
    }

    fn row(&self) -> Row {
        Row::new(self.summary.clone(), self.evidence_ids.clone())
    }
}

/// One question, ready to ask: its subjects, the findings its
/// recommendations may resolve, and the Markdown the reviewer reads.
#[derive(Clone, Debug)]
pub struct Question {
    pub question_id: StableId,
    pub kind: QuestionKind,
    pub template_version: u32,
    pub subjects: Vec<QuestionSubject>,
    /// Finding evidence IDs in scope; the plan's scope is every finding.
    pub finding_ids: Vec<String>,
    /// The questions whose answers the plan builds on.
    pub depends_on: Vec<StableId>,
    pub claim_slots: Vec<ClaimSlot>,
    pub recommendation_families: Vec<RecommendationFamily>,
    pub markdown: String,
    pub audit: ModelProjectionAudit,
    /// Every evidence ID the question shows.
    pub evidence_ids: BTreeSet<String>,
}

/// A trigger's choice: the subjects, any finding rows shown with them, and
/// the findings in scope.
#[derive(Default)]
struct Draft {
    subjects: Vec<QuestionSubject>,
    /// Subjects the trigger found beyond the template's limit.
    omitted_subjects: usize,
    finding_rows: Vec<Row>,
    /// Findings left out of the rows, and so out of scope.
    omitted_findings: usize,
    finding_ids: Vec<String>,
}

impl RepositoryView<'_> {
    /// The questions this repository triggers, in [`QuestionKind::ALL`]
    /// order. The plan is always asked, last, and depends on every other
    /// question.
    pub fn questions(&self, templates: &QuestionTemplateBundle) -> Result<Vec<Question>, String> {
        let bundle_digest = templates.digest();
        let mut questions = Vec::new();
        for kind in QuestionKind::ALL {
            let template = templates.template(kind);
            let drafts = match kind {
                QuestionKind::HotspotAssessment => triggers::hotspots(self, template.max_subjects),
                QuestionKind::SharedRootCause => triggers::clusters(self, template.max_questions),
                QuestionKind::ArchitectureReview => {
                    triggers::structure(self, template.max_subjects)
                }
                QuestionKind::RemediationPlan => vec![triggers::plan(self)],
            };
            let depends_on = if kind == QuestionKind::RemediationPlan {
                questions
                    .iter()
                    .map(|question: &Question| question.question_id.clone())
                    .collect()
            } else {
                Vec::new()
            };
            for draft in drafts.into_iter().take(template.max_questions) {
                questions.push(self.question(template, &bundle_digest, draft, &depends_on)?);
            }
        }
        Ok(questions)
    }

    fn question(
        &self,
        template: &QuestionTemplate,
        bundle_digest: &ContentDigest,
        draft: Draft,
        depends_on: &[StableId],
    ) -> Result<Question, String> {
        let subject_ids = draft
            .subjects
            .iter()
            .map(|subject| subject.id.as_str())
            .collect::<Vec<_>>();
        let question_id = StableId::from_identity(
            QUESTION_ID_PREFIX,
            QUESTION_SCHEMA_VERSION,
            &json!({
                "kind": template.kind,
                "template_version": template.version,
                "template_bundle_digest": bundle_digest.as_str(),
                "source_digest": self.source_digest,
                "subject_ids": subject_ids,
                "finding_ids": draft.finding_ids,
                "depends_on": depends_on.iter().map(StableId::as_str).collect::<Vec<_>>(),
            }),
        )
        .map_err(|error| error.to_string())?;
        let mut items = vec![Item::text("Ask", template.question.clone())];
        if !draft.subjects.is_empty() {
            items.push(Item::rows(
                more("Subjects", draft.omitted_subjects, "not asked about"),
                draft.subjects.iter().map(QuestionSubject::row),
                usize::MAX,
            ));
        }
        if !draft.finding_rows.is_empty() {
            items.push(Item::rows(
                more("Findings", draft.omitted_findings, "out of scope"),
                draft.finding_rows,
                usize::MAX,
            ));
        }
        if !depends_on.is_empty() {
            items.push(Item::notes(
                "Earlier questions",
                depends_on
                    .iter()
                    .map(|id| id.as_str().to_string())
                    .collect(),
            ));
        }
        items.push(Item::notes(
            "Answer with",
            vec![
                format!("question_id: {}", question_id.as_str()),
                format!("claim slots: {}", names(&template.claim_slots)),
                format!(
                    "recommendation families: {}",
                    names(&template.recommendation_families)
                ),
                format!("ranking: {}", template.kind.ranking_rule()),
            ],
        ));
        let rendered = BoundedDocument::new(
            format!("Question: {}", template.kind.title()),
            vec![Section::new("Question", items)],
        )
        .render(QUESTION_BYTES, &self.source_digest)?;
        if rendered.truncated {
            return Err(format!(
                "the {} question does not fit in {QUESTION_BYTES} bytes",
                template.kind.title()
            ));
        }
        Ok(Question {
            question_id,
            kind: template.kind,
            template_version: template.version,
            subjects: draft.subjects,
            finding_ids: draft.finding_ids,
            depends_on: depends_on.to_vec(),
            claim_slots: template.claim_slots.clone(),
            recommendation_families: template.recommendation_families.clone(),
            markdown: rendered.markdown,
            audit: rendered.audit,
            evidence_ids: rendered.evidence_ids,
        })
    }
}

/// A list label noting what the trigger left out.
fn more(label: &str, omitted: usize, why: &str) -> String {
    if omitted == 0 {
        label.to_string()
    } else {
        format!("{label} ({omitted} more {why})")
    }
}

/// Serialized enum names, comma-separated.
fn names<T: Serialize>(values: &[T]) -> String {
    values
        .iter()
        .map(|value| {
            serde_json::to_value(value)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

//! The engineering reviewer's view of a scanned repository: an orientation
//! pack given up front, and retrieval tools for the rest. Both read only the
//! report, the repository map, and hash-checked mapped source, and every row
//! they render cites an evidence ID the host issued.

mod document;
mod driver;
mod evidence;
mod modules;
mod orientation;
mod qualification;
mod questions;
mod rankings;
mod retrieval;
mod retrieval_render;
mod view;

pub(crate) use driver::REVIEWER_TURN;
pub use driver::{
    canonical_digest, run_engineering_review, CheckRequester, EngineeringReview, PatchCheckRecord,
    ProjectionSummary, QuestionRecord, QuestionStatus, RecommendationChecker, ReviewCall,
    ReviewSetup, Spend, DEFAULT_PATCH_CHECKS, ENGINEERING_REVIEW_SCHEMA_VERSION,
};
pub use orientation::{OrientationPack, MINIMUM_ORIENTATION_BYTES, TARGET_ORIENTATION_BYTES};
pub use qualification::{
    qualify_reviews, usefulness_sheet, Gate, GateResult, ItemKind, Novelty, PairStability,
    QualificationInputs, RankingOverlap, RecordedReview, ReviewQualification, RunQualification,
    ScoreCheck, StabilityPolicy, StabilityResult, UsefulnessItem, UsefulnessScores,
    UsefulnessSheet, UsefulnessStatus, UsefulnessSummary, REVIEW_QUALIFICATION_SCHEMA_VERSION,
    USEFULNESS_SHEET_SCHEMA_VERSION,
};
pub use questions::{
    Question, QuestionAnswer, QuestionKind, QuestionSubject, QuestionTemplate,
    QuestionTemplateBundle, QUESTION_SCHEMA_VERSION, QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION,
};
pub use retrieval::{
    retrieval_tool_specs, RetrievalBudget, RetrievalHost, RetrievalOutcome, RetrievalRecord,
    RETRIEVAL_TOOL_NAMES,
};
pub use view::RepositoryView;

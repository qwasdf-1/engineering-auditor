mod ensemble;
pub(crate) mod evidence_round;
mod reviewer;
mod reviewer_contract;
mod route;
mod single;
mod single_admission;
mod single_schema;
mod single_wire;
mod state;

pub use ensemble::{
    classify_ensemble_independence, EnsembleCaseFailure, EnsembleCaseRun,
    EnsembleChallengeExecution, EnsembleFailureStage, EnsembleIndependence, EnsembleReviewerPass,
    EnsembleReviewerSlot, ENSEMBLE_CASE_RUN_SCHEMA_VERSION,
};
pub use reviewer::CaseProviderContext;
pub(crate) use reviewer::{
    render_evidence_items_markdown, satisfied_evidence_items, ProviderProbeDefinition,
};
pub use route::{
    DecisionMagnitude, DeliberationMode, DeliberationRouter, DeterministicDisposition,
    ModeAvailability, ModeReason, ModeRoutingOutcome, ModeSelection, RoutePredicateEvidence,
    RoutingError, RoutingFacts, RoutingInput,
};
pub use single::{
    CasePosition, ReviewerAdmissionContext, ReviewerCaseReview, ReviewerCaseReviewPayload,
    SingleReviewerError, SingleReviewerSession, SingleReviewerTurn,
    REVIEWER_CASE_REVIEW_SCHEMA_VERSION,
};
pub use single_schema::reviewer_case_review_payload_schema;
pub(crate) use single_schema::{
    probe_answer_tool_schema, recommendation_wire_schema, validate_probe_answer_arguments,
    validate_recommendation_benefit_arguments,
};
pub use state::{
    CaseExecutionState, CaseStateLedger, CaseStateTransition, CaseTransitionError, SkippedStage,
    StageSkipReason,
};

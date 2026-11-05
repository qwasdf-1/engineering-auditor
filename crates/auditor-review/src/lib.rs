mod brief;
mod case_review;
mod history;
mod provider;
mod repository_map;
mod reviewer;
mod transcript_output;

pub use brief::{
    BriefBudgets, ConstraintKind, DocumentIntent, GoalPriority, ReviewBrief, ReviewConstraint,
    ReviewDocument, ReviewGoal, ReviewTestCommand, RiskLevel, RiskPosture,
    MAX_TEST_TIMEOUT_SECONDS, REVIEW_BRIEF_SCHEMA_VERSION,
};
pub use case_review::{
    model_value_from_json, render_model_document, render_structured_model_document,
    validate_claims, validate_recommendations, BenefitEvidenceMode, ClaimConfidence, ClaimDraft,
    ClaimSlot, ClaimStatus, ContentDigest, ContextualRecommendation, DetectorAdviceRelationship,
    EngineeringClaim, EngineeringClaimKind, ExpectedBenefit, IdentityError, Materiality,
    ModelDocument, ModelField, ModelProjection, ModelProjectionAudit, ModelSection, ModelValue,
    RecommendationAlternative, RecommendationBounds, RecommendationDraft, RecommendationFamily,
    StableId, VerificationStep, CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
    ENGINEERING_CLAIM_SCHEMA_VERSION, MODEL_MARKDOWN_RENDERER_VERSION,
};
pub use history::{
    load_history_digest, CoChange, FileHistory, HistoryDigest, HISTORY_DIGEST_SCHEMA_VERSION,
};
pub use provider::{
    discover_provider, render_native_conversation, render_transcript_markdown, translate_tool_spec,
    AssessmentMode, AssessmentModeRecorded, BuiltinAdapter, CallDispatch, CostClass,
    DiscoveryRecord, HostedConsent, HttpTransport, NativeConversationMessage, NativeToolCall,
    NativeToolTurn, PrivacyClass, ProviderCallRecord, ProviderConfig, ProviderFailure,
    ProviderFailureClass, ProviderKind, ProviderManifest, ProviderMaturity, ProviderProtocol,
    ProviderRequest, ProviderResponse, RepairContext, ReviewPromptBundleV2, ReviewRoutesV2,
    RouteBudget, RouteTarget, Tool, ToolChoice, ToolSpec, TranscriptEvent,
    TranscriptMarkdownOptions, TranscriptRecorder, Transport, TransportHeader, TransportRequest,
    TransportResponse, Usage, V2ProviderChain, PROVIDER_ADAPTER_VERSION,
    PROVIDER_TOOL_TRANSLATION_VERSION, REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION,
    REVIEW_ROUTES_V2_SCHEMA_VERSION, TOOL_NOOP,
};
pub use repository_map::{
    load_and_validate_repository_map, load_repository_map, LookupBudget, RepositoryMap,
    RepositoryMapError, SourceExcerpt, SourceLookup,
};
pub use reviewer::{
    canonical_digest, qualify_reviews, retrieval_tool_specs, run_engineering_review,
    usefulness_sheet, CheckRequester, EngineeringReview, Gate, GateResult, ItemKind,
    OrientationPack, PairStability, PatchCheckRecord, ProjectionSummary, QualificationInputs,
    Question, QuestionAnswer, QuestionKind, QuestionRecord, QuestionStatus, QuestionSubject,
    QuestionTemplate, QuestionTemplateBundle, RankingOverlap, RecommendationChecker,
    RecordedReview, RepositoryView, RetrievalBudget, RetrievalHost, RetrievalOutcome,
    RetrievalRecord, ReviewCall, ReviewQualification, ReviewSetup, RunQualification, ScoreCheck,
    Spend, StabilityPolicy, StabilityResult, UsefulnessItem, UsefulnessScores, UsefulnessSheet,
    UsefulnessStatus, UsefulnessSummary, DEFAULT_PATCH_CHECKS, ENGINEERING_REVIEW_SCHEMA_VERSION,
    MINIMUM_ORIENTATION_BYTES, QUESTION_SCHEMA_VERSION, QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION,
    RETRIEVAL_TOOL_NAMES, REVIEW_QUALIFICATION_SCHEMA_VERSION, TARGET_ORIENTATION_BYTES,
    USEFULNESS_SHEET_SCHEMA_VERSION,
};

mod brief;
mod case_review;
mod context;
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
    validate_claims, validate_recommendations, AgendaCoverage, AgendaPlanner, AgendaPolicy,
    AgendaPolicyDecision, AllowedEvidenceNeighborhood, BenefitEvidenceMode, CandidateSelection,
    CandidateSelectionError, CandidateSelector, CanonicalIdentity, CaseInstantiationError,
    CaseInstantiationResult, CaseKind, CasePlanningRecord, CasePlanningStatus, CaseTemplate,
    CaseTemplateBundle, CaseTemplateDefinition, CaseTemplateEntry, CaseTemplateRegistry,
    ClaimConfidence, ClaimDraft, ClaimSlot, ClaimStatus, ClosureState, ContentDigest,
    ContextualRecommendation, ContractKind, DecisionKind, DetectorAdviceRelationship,
    EngineeringCase, EngineeringClaim, EngineeringClaimKind, EvidenceAttachment,
    EvidenceAttachmentAdapter, EvidenceAttachmentKind, EvidenceClass, EvidenceClosure,
    EvidenceCompiler, EvidenceContractTemplate, EvidenceError, EvidenceIndexes, EvidenceItem,
    EvidenceItemKind, EvidenceLookup, EvidenceLookupRequest, EvidenceLookupRequestKind,
    EvidenceLookupResult, EvidenceOmission, EvidenceOmissionReason, EvidencePacket,
    EvidencePacketBudget, EvidenceRequirement, EvidenceUnavailable, ExpectedBenefit,
    FoundationAgendaPolicy, IdentityError, InputBinding, InputBindingDigests, InputContract,
    InputError, InputVerificationRequest, InputVerifier, Materiality, ModelDocument, ModelField,
    ModelProjection, ModelProjectionAudit, ModelSection, ModelValue, OperatorCaseConstraints,
    PlanningPriority, PolicySuppressionReason, RecommendationAlternative, RecommendationBounds,
    RecommendationDraft, RecommendationFamily, ReviewAgenda, ReviewCandidate, ReviewInput,
    SelectionReason, SelectorManifestEntry, SelectorRegistry, SemanticRecord, SemanticRecordLedger,
    SemanticRevisionRecord, SourceCandidateRef, StableId, SubjectPolicy, SuppressionReason,
    SuppressionRecord, SuppressionSignal, TaskCallProvenance, TaskLedgerError,
    TemplateNotEnabledReason, TemplateNotEnabledRecord, TemplateStatus, TriggerPolicy,
    VerificationStep, VerifiedContract, ABSOLUTE_SOURCE_EXCERPTS, AGENDA_PLANNER_VERSION,
    CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION, CASE_TEMPLATE_SCHEMA_VERSION, CASE_TEMPLATE_VERSION,
    CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION, ENGINEERING_CASE_SCHEMA_VERSION,
    ENGINEERING_CLAIM_SCHEMA_VERSION, EVIDENCE_PACKET_SCHEMA_VERSION, HARD_PACKET_BYTES,
    INITIAL_SOURCE_EXCERPTS, INPUT_BINDING_SCHEMA_VERSION, MAXIMUM_GRAPH_HOPS,
    MODEL_MARKDOWN_RENDERER_VERSION, REVIEW_AGENDA_SCHEMA_VERSION, REVIEW_CANDIDATE_SCHEMA_VERSION,
    SELECTOR_VERSION, TARGET_PACKET_BYTES,
};
pub use context::{
    compile_evidence_brief, compile_provider_context, compile_provider_context_with_lookup,
    CapabilityBrief, ContextBlock, ContextBlockKind, ContextBudget, DocumentationClaim,
    EgressManifest, EgressManifestEntry, EgressPolicy, EvidenceBrief, EvidenceGraphOutline,
    FindingBrief, ProviderContext, ProviderContextBudgets, QueryableSymbol, TrustLabel,
    MAX_QUERYABLE_SYMBOLS,
};
pub use history::{
    load_history_digest, CoChange, FileHistory, HistoryDigest, HISTORY_DIGEST_SCHEMA_VERSION,
};
pub use provider::{
    discover_provider, render_native_conversation, render_transcript_markdown, translate_tool_spec,
    AssessmentMode, AssessmentModeRecorded, BudgetExhaustion, BuiltinAdapter, CallDispatch,
    CaseBudgetLedger, CaseBudgetPolicy, CaseCallContext, CaseProviderCallRecord, CostClass,
    DiscoveryRecord, HostedConsent, HttpTransport, NativeConversationMessage, NativeToolCall,
    NativeToolTurn, PrivacyClass, ProviderCallRecord, ProviderConfig, ProviderFailure,
    ProviderFailureClass, ProviderKind, ProviderManifest, ProviderMaturity, ProviderProtocol,
    ProviderRequest, ProviderResponse, RepairContext, ReviewPromptBundleV2, ReviewRole,
    ReviewRoutesV2, RouteBudget, RouteTarget, Tool, ToolChoice, ToolSpec, TranscriptEvent,
    TranscriptMarkdownOptions, TranscriptRecorder, Transport, TransportHeader, TransportRequest,
    TransportResponse, Usage, V2ProviderChain, PROVIDER_ADAPTER_VERSION,
    PROVIDER_TOOL_TRANSLATION_VERSION, REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION,
    REVIEW_ROUTES_V2_SCHEMA_VERSION, TOOL_NOOP,
};
pub use repository_map::{
    load_and_validate_repository_map, LookupBudget, RepositoryMap, RepositoryMapError,
    SourceExcerpt, SourceLookup,
};
pub use reviewer::{
    retrieval_tool_specs, run_engineering_review, CheckRequester, EngineeringReview,
    OrientationPack, PatchCheckRecord, ProjectionSummary, Question, QuestionAnswer, QuestionKind,
    QuestionRecord, QuestionStatus, QuestionSubject, QuestionTemplate, QuestionTemplateBundle,
    RecommendationChecker, RepositoryView, RetrievalBudget, RetrievalHost, RetrievalOutcome,
    RetrievalRecord, ReviewCall, ReviewSetup, Spend, DEFAULT_PATCH_CHECKS,
    ENGINEERING_REVIEW_SCHEMA_VERSION, MINIMUM_ORIENTATION_BYTES, QUESTION_SCHEMA_VERSION,
    QUESTION_TEMPLATE_BUNDLE_SCHEMA_VERSION, RETRIEVAL_TOOL_NAMES, TARGET_ORIENTATION_BYTES,
};

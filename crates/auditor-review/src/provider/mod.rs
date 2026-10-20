mod adapters;
mod case_qualification;
mod case_tool;
mod case_tool_schema;
mod discovery;
mod dispatch;
mod native;
mod native_tool;
mod native_tool_adapter;
mod repair;
mod retry;
mod schema_validation;
mod transcript;
mod transcript_markdown;
mod transport;
mod types;
mod v2;
mod wire_response;

pub use adapters::BuiltinAdapter;
pub use case_qualification::{
    qualify_case_target, CaseQualificationRuntime, CASE_ROLE_TOOL_QUALIFICATION_STAGE,
    CASE_SEMANTIC_QUALIFICATION_STAGE,
};
pub use case_tool::{
    case_tool_schema_digest, case_tools_for_role, challenge_response_tools, CaseEvidenceRequest,
    CaseProviderTurn, CaseTool, CaseToolBoundary, CaseToolError, TOOL_ANSWER_CASE_PROBE,
    TOOL_COMPLETE_CASE_CHALLENGE, TOOL_COMPLETE_CASE_REVIEW, TOOL_COMPLETE_CHALLENGE_RESPONSE,
    TOOL_EMIT_QUALIFICATION_NONCE, TOOL_RECORD_CASE_CLAIM, TOOL_RECORD_CASE_OBJECTION,
    TOOL_RECORD_CASE_RECOMMENDATION, TOOL_RECORD_COMMON_CONCERN, TOOL_RECORD_OBJECTION_RESPONSE,
    TOOL_RECORD_REPLACEMENT_RECOMMENDATION, TOOL_REOPEN_CASE_CLAIM, TOOL_REOPEN_CASE_OBJECTION,
    TOOL_REOPEN_CASE_RECOMMENDATION, TOOL_REOPEN_COMMON_CONCERN, TOOL_REOPEN_OBJECTION_RESPONSE,
    TOOL_REOPEN_REPLACEMENT_RECOMMENDATION, TOOL_REQUEST_CASE_EVIDENCE, TOOL_REVISE_CASE_CLAIM,
    TOOL_REVISE_CASE_OBJECTION, TOOL_REVISE_CASE_PROBE_ANSWER, TOOL_REVISE_CASE_RECOMMENDATION,
    TOOL_REVISE_COMMON_CONCERN, TOOL_REVISE_OBJECTION_RESPONSE,
    TOOL_REVISE_REPLACEMENT_RECOMMENDATION, TOOL_WITHDRAW_CASE_CLAIM, TOOL_WITHDRAW_CASE_OBJECTION,
    TOOL_WITHDRAW_CASE_RECOMMENDATION, TOOL_WITHDRAW_COMMON_CONCERN,
    TOOL_WITHDRAW_OBJECTION_RESPONSE, TOOL_WITHDRAW_REPLACEMENT_RECOMMENDATION,
};
pub use discovery::discover_provider;
pub use dispatch::CallDispatch;
pub(crate) use native::parse_native_assistant_output;
pub use native::{
    render_native_conversation, NativeConversationMessage, NativeToolCall, NativeToolTurn,
};
pub use native_tool::{
    translate as translate_tool_spec, Tool, ToolChoice, ToolSpec,
    PROVIDER_TOOL_TRANSLATION_VERSION, TOOL_NOOP,
};
pub use native_tool_adapter::{AssessmentMode, AssessmentModeRecorded};
pub use repair::RepairContext;
pub(crate) use retry::{wait_before_no_model_output_retry, MAX_NO_MODEL_OUTPUT_RETRIES};
pub use transcript::{TranscriptEvent, TranscriptRecorder};
pub use transcript_markdown::{render_transcript_markdown, TranscriptMarkdownOptions};
pub use transport::{
    HttpTransport, Transport, TransportHeader, TransportRequest, TransportResponse,
};
pub use types::{
    CostClass, DiscoveryRecord, HostedConsent, PrivacyClass, ProviderCallRecord, ProviderConfig,
    ProviderFailure, ProviderFailureClass, ProviderKind, ProviderManifest, ProviderMaturity,
    ProviderProtocol, ProviderRequest, ProviderResponse, RouteBudget, RouteTarget, Usage,
    PROVIDER_ADAPTER_VERSION,
};
pub use v2::{
    qualification_binding, BudgetExhaustion, CaseBudgetLedger, CaseBudgetPolicy, CaseCallContext,
    CaseProviderCallRecord, CaseQualificationBinding, CaseQualificationRecord, EnsembleRouteChains,
    QualificationScope, ReviewPromptBundleV2, ReviewRole, ReviewRoutesV2,
    SemanticQualificationCase, V2ProviderChain, REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION,
    REVIEW_QUALIFICATION_V2_SCHEMA_VERSION, REVIEW_ROUTES_V2_SCHEMA_VERSION,
};

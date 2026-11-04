mod adapters;
mod discovery;
mod dispatch;
mod native;
mod native_tool;
mod native_tool_adapter;
mod repair;
pub(crate) mod retry;
mod schema_validation;
mod transcript;
mod transcript_markdown;
mod transport;
mod types;
mod v2;
mod wire_response;

pub use adapters::BuiltinAdapter;
pub use discovery::discover_provider;
pub use dispatch::CallDispatch;
pub use native::{
    render_native_conversation, NativeConversationMessage, NativeToolCall, NativeToolTurn,
};
pub use native_tool::{
    translate as translate_tool_spec, Tool, ToolChoice, ToolSpec,
    PROVIDER_TOOL_TRANSLATION_VERSION, TOOL_NOOP,
};
pub use native_tool_adapter::{AssessmentMode, AssessmentModeRecorded};
pub use repair::RepairContext;
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
    BudgetExhaustion, CaseBudgetLedger, CaseBudgetPolicy, CaseCallContext, CaseProviderCallRecord,
    ReviewPromptBundleV2, ReviewRole, ReviewRoutesV2, V2ProviderChain,
    REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION, REVIEW_ROUTES_V2_SCHEMA_VERSION,
};

use std::collections::{BTreeMap, BTreeSet};

use auditor_core::sha256_digest_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROVIDER_ADAPTER_VERSION: u32 = 45;

const HTTP_AUTHENTICATION_REQUIRED: u16 = 401;
const HTTP_AUTHORIZATION_DENIED: u16 = 403;
const HTTP_BAD_REQUEST: u16 = 400;
const HTTP_NOT_FOUND: u16 = 404;
const HTTP_METHOD_NOT_ALLOWED: u16 = 405;
const HTTP_REQUEST_TIMEOUT: u16 = 408;
const HTTP_CONTENT_TOO_LARGE: u16 = 413;
const HTTP_UNSUPPORTED_MEDIA_TYPE: u16 = 415;
const HTTP_UNPROCESSABLE_CONTENT: u16 = 422;
const HTTP_RATE_LIMITED: u16 = 429;
const HTTP_GATEWAY_TIMEOUT: u16 = 504;
const HTTP_SERVER_ERROR_START: u16 = 500;
const HTTP_SERVER_ERROR_END: u16 = 599;
const DEFAULT_ROUTE_REQUESTS: usize = 32;
const DEFAULT_ROUTE_INPUT_BYTES: usize = 512 * 1024;
const DEFAULT_ROUTE_OUTPUT_TOKENS: usize = 65_536;
const DEFAULT_ROUTE_TIMEOUT_MS: u64 = 360_000;
const DEFAULT_ROUTE_COST_MICROUSD: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAiResponses,
    AnthropicMessages,
    MistralChat,
    OpenCodeZen,
    OpenCodeAnonymous,
    OpenAiCompatible,
}

impl ProviderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::MistralChat => "mistral_chat",
            Self::OpenCodeZen => "opencode_zen",
            Self::OpenCodeAnonymous => "opencode_anonymous",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    pub const fn is_anonymous(self) -> bool {
        matches!(self, Self::OpenCodeAnonymous)
    }
}

pub(super) fn classify_http_status(status: u16) -> ProviderFailureClass {
    match status {
        HTTP_AUTHENTICATION_REQUIRED => ProviderFailureClass::Authentication,
        HTTP_AUTHORIZATION_DENIED => ProviderFailureClass::Authorization,
        HTTP_REQUEST_TIMEOUT | HTTP_GATEWAY_TIMEOUT => ProviderFailureClass::Timeout,
        HTTP_CONTENT_TOO_LARGE => ProviderFailureClass::ContextOverflow,
        HTTP_RATE_LIMITED => ProviderFailureClass::RateLimit,
        HTTP_SERVER_ERROR_START..=HTTP_SERVER_ERROR_END => ProviderFailureClass::Unavailable,
        HTTP_BAD_REQUEST
        | HTTP_NOT_FOUND
        | HTTP_METHOD_NOT_ALLOWED
        | HTTP_UNSUPPORTED_MEDIA_TYPE
        | HTTP_UNPROCESSABLE_CONTENT => ProviderFailureClass::Configuration,
        _ => ProviderFailureClass::Refusal,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    Responses,
    Messages,
    ChatCompletions,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Zero,
    Known,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyClass {
    HostedCommercial,
    HostedAnonymous,
    OperatorConfigured,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMaturity {
    Stable,
    Experimental,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub endpoint_base: String,
    pub api_key_env: Option<String>,
    pub models: Vec<String>,
    #[serde(default)]
    pub model_protocols: BTreeMap<String, ProviderProtocol>,
    #[serde(default)]
    pub chat_completions_path: Option<String>,
    #[serde(default = "default_cost_class")]
    pub cost_class: CostClass,
    #[serde(default)]
    pub estimated_cost_microusd_per_request: Option<u64>,
    #[serde(default = "default_privacy_class")]
    pub privacy_class: PrivacyClass,
}

struct ProviderSeed {
    id: String,
    kind: ProviderKind,
    endpoint_base: String,
    api_key_env: Option<String>,
    models: Vec<String>,
    privacy_class: PrivacyClass,
    cost_class: CostClass,
}

const fn default_cost_class() -> CostClass {
    CostClass::Unknown
}

const fn default_privacy_class() -> PrivacyClass {
    PrivacyClass::OperatorConfigured
}

impl ProviderConfig {
    pub fn openai(models: Vec<String>) -> Self {
        Self::from_seed(ProviderSeed {
            id: "openai".to_string(),
            kind: ProviderKind::OpenAiResponses,
            endpoint_base: "https://api.openai.com".to_string(),
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            models,
            privacy_class: PrivacyClass::HostedCommercial,
            cost_class: CostClass::Unknown,
        })
    }

    pub fn anthropic(models: Vec<String>) -> Self {
        Self::from_seed(ProviderSeed {
            id: "anthropic".to_string(),
            kind: ProviderKind::AnthropicMessages,
            endpoint_base: "https://api.anthropic.com".to_string(),
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            models,
            privacy_class: PrivacyClass::HostedCommercial,
            cost_class: CostClass::Unknown,
        })
    }

    pub fn mistral(models: Vec<String>) -> Self {
        Self::from_seed(ProviderSeed {
            id: "mistral".to_string(),
            kind: ProviderKind::MistralChat,
            endpoint_base: "https://api.mistral.ai".to_string(),
            api_key_env: Some("MISTRAL_API_KEY".to_string()),
            models,
            privacy_class: PrivacyClass::HostedCommercial,
            cost_class: CostClass::Unknown,
        })
    }

    pub fn opencode_zen(models: Vec<String>) -> Self {
        Self::from_seed(ProviderSeed {
            id: "opencode_zen".to_string(),
            kind: ProviderKind::OpenCodeZen,
            endpoint_base: "https://opencode.ai/zen/v1".to_string(),
            api_key_env: Some("OPENCODE_ZEN_API_KEY".to_string()),
            models,
            privacy_class: PrivacyClass::HostedCommercial,
            cost_class: CostClass::Unknown,
        })
    }

    pub fn opencode_anonymous(models: Vec<String>) -> Self {
        Self::from_seed(ProviderSeed {
            id: "opencode_anonymous".to_string(),
            kind: ProviderKind::OpenCodeAnonymous,
            endpoint_base: "https://opencode.ai/zen/v1".to_string(),
            api_key_env: None,
            models,
            privacy_class: PrivacyClass::HostedAnonymous,
            cost_class: CostClass::Zero,
        })
    }

    pub fn openai_compatible(
        id: impl Into<String>,
        endpoint_base: impl Into<String>,
        api_key_env: Option<impl Into<String>>,
        models: Vec<String>,
    ) -> Self {
        Self::from_seed(ProviderSeed {
            id: id.into(),
            kind: ProviderKind::OpenAiCompatible,
            endpoint_base: endpoint_base.into(),
            api_key_env: api_key_env.map(Into::into),
            models,
            privacy_class: PrivacyClass::OperatorConfigured,
            cost_class: CostClass::Unknown,
        })
    }

    fn from_seed(seed: ProviderSeed) -> Self {
        let endpoint_base = seed.endpoint_base.trim_end_matches('/').to_string();
        let chat_completions_path = (seed.kind == ProviderKind::OpenAiCompatible).then(|| {
            if endpoint_base.ends_with("/v1") {
                "/chat/completions".to_string()
            } else {
                "/v1/chat/completions".to_string()
            }
        });
        Self {
            id: seed.id,
            kind: seed.kind,
            endpoint_base,
            api_key_env: seed.api_key_env,
            models: seed.models,
            model_protocols: BTreeMap::new(),
            chat_completions_path,
            cost_class: seed.cost_class,
            estimated_cost_microusd_per_request: None,
            privacy_class: seed.privacy_class,
        }
    }

    pub fn protocol_for(&self, model: &str) -> ProviderProtocol {
        self.model_protocols
            .get(model)
            .copied()
            .unwrap_or(match self.kind {
                ProviderKind::OpenAiResponses => ProviderProtocol::Responses,
                ProviderKind::AnthropicMessages => ProviderProtocol::Messages,
                ProviderKind::MistralChat
                | ProviderKind::OpenCodeZen
                | ProviderKind::OpenCodeAnonymous
                | ProviderKind::OpenAiCompatible => ProviderProtocol::ChatCompletions,
            })
    }

    pub fn validate(&self) -> Result<(), String> {
        let models = validate_models(self)?;
        validate_protocols(self, &models)?;
        validate_chat_path(self)?;
        validate_endpoint(self.kind, &self.endpoint_base)?;
        validate_security_classification(self)?;
        validate_cost(self)
    }

    pub fn manifest(&self) -> ProviderManifest {
        ProviderManifest {
            provider_id: self.id.clone(),
            provider_kind: self.kind,
            origin: self.endpoint_base.clone(),
            protocols: self
                .models
                .iter()
                .map(|model| self.protocol_for(model))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            auth_mode: if self.kind.is_anonymous() {
                "public_compatibility_credential".to_string()
            } else if self.api_key_env.is_some() {
                "environment_bearer".to_string()
            } else {
                "none".to_string()
            },
            structured_output_strength: "provider_native_tools_v2".to_string(),
            cost_class: self.cost_class,
            estimated_cost_microusd_per_request: self.estimated_cost_microusd_per_request,
            privacy_class: self.privacy_class,
            maturity: if matches!(
                self.kind,
                ProviderKind::OpenCodeAnonymous | ProviderKind::OpenAiCompatible
            ) {
                ProviderMaturity::Experimental
            } else {
                ProviderMaturity::Stable
            },
            adapter_version: PROVIDER_ADAPTER_VERSION,
            evidence_source: "checked-in adapter contract plus qualification artifact".to_string(),
        }
    }
}

fn validate_models(config: &ProviderConfig) -> Result<BTreeSet<&str>, String> {
    if config.id.is_empty() || config.models.is_empty() {
        return Err("provider requires a nonempty id and model list".to_string());
    }
    let mut models = BTreeSet::new();
    if config
        .models
        .iter()
        .any(|model| model.is_empty() || !models.insert(model.as_str()))
    {
        return Err("provider models must be unique and nonempty".to_string());
    }
    Ok(models)
}

fn validate_protocols(config: &ProviderConfig, models: &BTreeSet<&str>) -> Result<(), String> {
    if config
        .model_protocols
        .keys()
        .any(|model| !models.contains(model.as_str()))
    {
        return Err("provider protocol override references an unconfigured model".to_string());
    }
    Ok(())
}

fn validate_chat_path(config: &ProviderConfig) -> Result<(), String> {
    match (&config.kind, &config.chat_completions_path) {
        (ProviderKind::OpenAiCompatible, Some(path)) if valid_api_path(path) => Ok(()),
        (ProviderKind::OpenAiCompatible, _) => Err(
            "custom compatible provider requires an explicit safe chat-completions path"
                .to_string(),
        ),
        (_, None) => Ok(()),
        (_, Some(_)) => {
            Err("chat-completions path is only valid for a custom compatible provider".to_string())
        }
    }
}

fn validate_security_classification(config: &ProviderConfig) -> Result<(), String> {
    if config.api_key_env.as_deref().is_some_and(|variable| {
        variable.is_empty()
            || !variable
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    }) {
        return Err("provider API key environment variable name is invalid".to_string());
    }
    if config.kind.is_anonymous()
        && (config.api_key_env.is_some() || config.cost_class != CostClass::Zero)
    {
        return Err("anonymous provider must use secret-free zero-cost configuration".to_string());
    }
    if config.kind == ProviderKind::OpenAiCompatible
        && config.privacy_class != PrivacyClass::OperatorConfigured
    {
        return Err("custom compatible providers require operator-configured privacy".to_string());
    }
    Ok(())
}

fn validate_cost(config: &ProviderConfig) -> Result<(), String> {
    match (
        config.cost_class,
        config.estimated_cost_microusd_per_request,
    ) {
        (CostClass::Zero, None | Some(0)) | (CostClass::Unknown, None) => Ok(()),
        (CostClass::Known, Some(1..)) => Ok(()),
        (CostClass::Zero, Some(_)) => {
            Err("zero-cost provider cannot declare a positive request cost".to_string())
        }
        (CostClass::Known, _) => {
            Err("known-cost provider requires a positive request estimate".to_string())
        }
        (CostClass::Unknown, Some(_)) => {
            Err("unknown-cost provider cannot declare a request estimate".to_string())
        }
    }
}

fn validate_endpoint(kind: ProviderKind, endpoint: &str) -> Result<(), String> {
    if endpoint.is_empty()
        || endpoint.contains('@')
        || endpoint.contains('?')
        || endpoint.contains('#')
        || endpoint.ends_with('/')
    {
        return Err("provider endpoint must be a credential-free origin/base URL".to_string());
    }
    let https = endpoint.starts_with("https://");
    let loopback = endpoint.starts_with("http://127.0.0.1")
        || endpoint.starts_with("http://localhost")
        || endpoint.starts_with("http://[::1]");
    if !(https || kind == ProviderKind::OpenAiCompatible && loopback) {
        return Err(
            "provider endpoint requires HTTPS or loopback HTTP for a custom route".to_string(),
        );
    }
    Ok(())
}

fn valid_api_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("..")
        && !path.contains('\\')
        && !path.contains('?')
        && !path.contains('#')
        && path.ends_with("/chat/completions")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderManifest {
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub origin: String,
    pub protocols: Vec<ProviderProtocol>,
    pub auth_mode: String,
    pub structured_output_strength: String,
    pub cost_class: CostClass,
    pub estimated_cost_microusd_per_request: Option<u64>,
    pub privacy_class: PrivacyClass,
    pub maturity: ProviderMaturity,
    pub adapter_version: u32,
    pub evidence_source: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostedConsent {
    pub allow_hosted: bool,
    pub allow_anonymous: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteBudget {
    pub max_requests: usize,
    pub max_input_bytes: usize,
    /// Per-request generation allowance. Earlier calls never deplete the
    /// allowance of a later initial, repair, or host-correction turn.
    pub max_output_tokens: usize,
    pub timeout_ms: u64,
    pub max_cost_microusd: u64,
    pub allow_unknown_cost: bool,
}

impl Default for RouteBudget {
    fn default() -> Self {
        Self {
            max_requests: DEFAULT_ROUTE_REQUESTS,
            max_input_bytes: DEFAULT_ROUTE_INPUT_BYTES,
            max_output_tokens: DEFAULT_ROUTE_OUTPUT_TOKENS,
            timeout_ms: DEFAULT_ROUTE_TIMEOUT_MS,
            max_cost_microusd: DEFAULT_ROUTE_COST_MICROUSD,
            allow_unknown_cost: false,
        }
    }
}

impl RouteBudget {
    pub fn validate(self) -> Result<(), String> {
        if self.max_requests == 0
            || self.max_input_bytes == 0
            || self.max_output_tokens == 0
            || self.timeout_ms == 0
        {
            return Err(
                "route request, input, output, and timeout budgets must be positive".to_string(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ProviderRequest {
    pub request_id: String,
    pub stage: String,
    pub system_instruction: String,
    pub context: Value,
    pub context_markdown: Option<String>,
    pub output_schema_name: String,
    pub output_schema: Value,
    pub max_input_bytes: usize,
    pub max_output_tokens: usize,
    pub timeout_ms: u64,
}

impl ProviderRequest {
    pub fn context_digest(&self) -> String {
        sha256_digest_bytes(
            serde_json::to_string(&self.context)
                .expect("provider context is serializable")
                .as_bytes(),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Usage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    /// Input tokens the provider served from its prompt cache; included in
    /// `input_tokens`.
    #[serde(default)]
    pub cached_input_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct ProviderResponse {
    pub provider_id: String,
    pub model: String,
    pub provider_request_id: Option<String>,
    pub finish_reason: Option<String>,
    pub structured_output: Value,
    pub usage: Usage,
    pub capability_observations: Vec<String>,
    pub native_assistant_output: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailureClass {
    Authentication,
    Authorization,
    RateLimit,
    Timeout,
    Unavailable,
    ContextOverflow,
    Refusal,
    Truncated,
    InvalidOutput,
    Transport,
    Policy,
    Configuration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderFailure {
    pub provider_id: String,
    pub model: String,
    pub class: ProviderFailureClass,
    pub retryable_without_model_output: bool,
    pub message: String,
    pub usage: Usage,
    /// Boxed so the failure stays within clippy's `result_large_err` bound.
    pub(crate) prior_assistant_output: Option<Box<str>>,
    pub(crate) assessment_mode: super::native_tool_adapter::AssessmentModeRecorded,
}

impl ProviderFailure {
    pub(crate) fn new(
        provider_id: impl Into<String>,
        model: impl Into<String>,
        class: ProviderFailureClass,
        retryable_without_model_output: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model: model.into(),
            class,
            retryable_without_model_output,
            message: message.into(),
            usage: Usage::default(),
            prior_assistant_output: None,
            assessment_mode: Default::default(),
        }
    }

    pub(crate) fn with_repair_output(
        mut self,
        prior_assistant_output: String,
        assessment_mode: super::native_tool_adapter::AssessmentModeRecorded,
    ) -> Self {
        self.prior_assistant_output = Some(prior_assistant_output.into_boxed_str());
        self.assessment_mode = assessment_mode;
        self
    }

    pub(crate) fn with_output_usage(mut self, usage: Usage) -> Self {
        self.usage = usage;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RouteTarget {
    pub provider_id: String,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DiscoveryRecord {
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub origin: String,
    pub discovered_models: Vec<String>,
    pub configured_missing: Vec<String>,
    pub discovered_unconfigured: Vec<String>,
    pub failure_class: Option<ProviderFailureClass>,
    pub observed_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderCallRecord {
    pub request_id: String,
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub model: String,
    pub stage: String,
    pub started_at_unix_millis: u128,
    pub finished_at_unix_millis: u128,
    pub context_digest: String,
    pub output_digest: Option<String>,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub finish_reason: Option<String>,
    pub usage: Usage,
    pub failure_class: Option<ProviderFailureClass>,
    pub discarded: bool,
    /// Provider-native tool mode used for this call.
    #[serde(default)]
    pub assessment_mode: super::native_tool_adapter::AssessmentModeRecorded,
    /// v0.9.13: per-shard repair attempts. `0` for the original (v0.9.12)
    /// call; `1` records a single round-trip Pass 2 repair.
    #[serde(default)]
    pub repair_attempts: u8,
}

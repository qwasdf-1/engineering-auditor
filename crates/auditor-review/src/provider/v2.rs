use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    CostClass, ProviderCallRecord, ProviderConfig, ProviderFailureClass, RouteBudget, RouteTarget,
    Usage,
};
use crate::{CanonicalIdentity, CaseKind, ContentDigest, ContractKind, InputContract, StableId};

pub const REVIEW_ROUTES_V2_SCHEMA_VERSION: u32 = 3;
pub const REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRole {
    SingleReviewer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct V2ProviderChain {
    pub targets: Vec<RouteTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseBudgetPolicy {
    pub defaults: RouteBudget,
    pub per_template: BTreeMap<CaseKind, RouteBudget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRoutesV2 {
    pub schema_version: u32,
    pub providers: Vec<ProviderConfig>,
    pub single_reviewer: V2ProviderChain,
    pub review_budget: RouteBudget,
    pub case_budgets: CaseBudgetPolicy,
}

impl ReviewRoutesV2 {
    pub fn parse(text: &str) -> Result<Self, String> {
        let routes: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid v2 review routes: {error}"))?;
        routes.validate()?;
        Ok(routes)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != REVIEW_ROUTES_V2_SCHEMA_VERSION {
            return Err("unsupported v2 review route schema".to_string());
        }
        self.review_budget.validate()?;
        self.case_budgets.defaults.validate()?;
        validate_tightening(self.case_budgets.defaults, self.review_budget)?;
        for budget in self.case_budgets.per_template.values().copied() {
            budget.validate()?;
            validate_tightening(budget, self.case_budgets.defaults)?;
        }
        let providers = provider_registry(&self.providers, self.review_budget)?;
        validate_chain(&self.single_reviewer, &providers, "single_reviewer")
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|provider| provider.id == id)
    }

    pub fn budget_for(&self, kind: CaseKind) -> RouteBudget {
        self.case_budgets
            .per_template
            .get(&kind)
            .copied()
            .unwrap_or(self.case_budgets.defaults)
    }
}

impl CanonicalIdentity for ReviewRoutesV2 {
    fn identity_fields(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("v2 routes are serializable")
    }
}

impl InputContract for ReviewRoutesV2 {
    fn contract_kind(&self) -> ContractKind {
        ContractKind::RouteConfiguration
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        REVIEW_ROUTES_V2_SCHEMA_VERSION
    }

    fn validate_contract(&self) -> Result<(), String> {
        self.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPromptBundleV2 {
    pub schema_version: u32,
    pub bundle_id: StableId,
    pub core_instruction: String,
    pub reviewer_instruction: String,
}

impl ReviewPromptBundleV2 {
    pub fn parse(text: &str) -> Result<Self, String> {
        let bundle: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid v2 prompt bundle: {error}"))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION
            || self.bundle_id != self.expected_id()
            || self
                .instructions()
                .iter()
                .any(|value| value.trim().is_empty())
        {
            return Err("v2 prompt bundle identity or instruction is invalid".to_string());
        }
        Ok(())
    }

    pub fn expected_id(&self) -> StableId {
        StableId::from_identity(
            "review-prompt-bundle",
            REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION,
            &json!({
                "core_instruction": self.core_instruction,
                "reviewer_instruction": self.reviewer_instruction,
            }),
        )
        .expect("review-prompt-bundle is a valid typed prefix")
    }

    pub fn digest(&self) -> ContentDigest {
        ContentDigest::from_canonical_value(
            &serde_json::to_value(self).expect("v2 prompt bundle is serializable"),
        )
    }

    fn instructions(&self) -> [&str; 2] {
        [&self.core_instruction, &self.reviewer_instruction]
    }
}

impl CanonicalIdentity for ReviewPromptBundleV2 {
    fn identity_fields(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("v2 prompt bundle is serializable")
    }
}

impl InputContract for ReviewPromptBundleV2 {
    fn contract_kind(&self) -> ContractKind {
        ContractKind::PromptBundle
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION
    }

    fn validate_contract(&self) -> Result<(), String> {
        self.validate()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BudgetUsage {
    requests: usize,
    input_bytes: usize,
    cost_microusd: u64,
}

pub struct CaseBudgetLedger {
    review_budget: RouteBudget,
    case_budgets: CaseBudgetPolicy,
    review_usage: Mutex<BudgetUsage>,
    case_usage: Mutex<BTreeMap<StableId, BudgetUsage>>,
}

impl CaseBudgetLedger {
    pub fn new(review_budget: RouteBudget, case_budgets: CaseBudgetPolicy) -> Result<Self, String> {
        review_budget.validate()?;
        case_budgets.defaults.validate()?;
        validate_tightening(case_budgets.defaults, review_budget)?;
        Ok(Self {
            review_budget,
            case_budgets,
            review_usage: Mutex::new(BudgetUsage::default()),
            case_usage: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn reserve(
        &self,
        case_id: &StableId,
        case_kind: CaseKind,
        input_bytes: usize,
        requested_output_tokens: usize,
        cost_microusd: u64,
    ) -> Result<usize, BudgetExhaustion> {
        let case_budget = self
            .case_budgets
            .per_template
            .get(&case_kind)
            .copied()
            .unwrap_or(self.case_budgets.defaults);
        let increment = BudgetUsage {
            requests: 1,
            input_bytes,
            cost_microusd,
        };
        let mut review = self.review_usage.lock().expect("review budget mutex");
        let mut cases = self.case_usage.lock().expect("case budget mutex");
        let case = cases.entry(case_id.clone()).or_default();
        if exceeds(*case, increment, case_budget) {
            return Err(BudgetExhaustion::Case);
        }
        if exceeds(*review, increment, self.review_budget) {
            return Err(BudgetExhaustion::Review);
        }
        if requested_output_tokens > case_budget.max_output_tokens {
            return Err(BudgetExhaustion::Case);
        }
        if requested_output_tokens > self.review_budget.max_output_tokens {
            return Err(BudgetExhaustion::Review);
        }
        add_usage(case, increment);
        add_usage(&mut review, increment);
        Ok(requested_output_tokens)
    }

    pub fn settle_output(
        &self,
        case_id: &StableId,
        reserved_output_tokens: usize,
        actual_output_tokens: usize,
    ) -> Result<(), BudgetExhaustion> {
        if actual_output_tokens > reserved_output_tokens {
            return Err(BudgetExhaustion::Case);
        }
        let cases = self.case_usage.lock().expect("case budget mutex");
        if !cases.contains_key(case_id) {
            return Err(BudgetExhaustion::Case);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetExhaustion {
    Case,
    Review,
}

impl fmt::Display for BudgetExhaustion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Case => "case provider budget exhausted",
            Self::Review => "review-wide provider budget exhausted",
        })
    }
}

impl std::error::Error for BudgetExhaustion {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseProviderCallRecord {
    pub call: ProviderCallRecord,
    pub packet_digest: ContentDigest,
    pub role: ReviewRole,
    pub case_id: StableId,
    pub pass: u8,
    pub usage: Usage,
    pub failure_class: Option<ProviderFailureClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_detail: Option<String>,
    pub repair_count: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseCallContext {
    pub packet_digest: ContentDigest,
    pub role: ReviewRole,
    pub case_id: StableId,
    pub pass: u8,
}

impl CaseProviderCallRecord {
    pub fn from_call(call: ProviderCallRecord, context: CaseCallContext) -> Self {
        Self {
            usage: call.usage,
            failure_class: call.failure_class,
            failure_detail: None,
            repair_count: call.repair_attempts,
            call,
            packet_digest: context.packet_digest,
            role: context.role,
            case_id: context.case_id,
            pass: context.pass,
        }
    }

    pub fn with_failure_detail(mut self, detail: impl Into<String>) -> Self {
        self.failure_detail = Some(detail.into());
        self
    }
}

fn provider_registry(
    providers: &[ProviderConfig],
    budget: RouteBudget,
) -> Result<BTreeMap<&str, &ProviderConfig>, String> {
    let mut registry = BTreeMap::new();
    for provider in providers {
        provider.validate()?;
        if registry.insert(provider.id.as_str(), provider).is_some() {
            return Err(format!("duplicate provider registry id `{}`", provider.id));
        }
        if provider.cost_class == CostClass::Unknown && !budget.allow_unknown_cost {
            return Err(format!(
                "provider `{}` requires unknown-cost consent",
                provider.id
            ));
        }
    }
    Ok(registry)
}

fn validate_chain(
    chain: &V2ProviderChain,
    providers: &BTreeMap<&str, &ProviderConfig>,
    name: &str,
) -> Result<(), String> {
    if chain.targets.is_empty() {
        return Err(format!("v2 route chain `{name}` is empty"));
    }
    let mut seen = BTreeSet::new();
    for target in &chain.targets {
        let provider = providers.get(target.provider_id.as_str()).ok_or_else(|| {
            format!(
                "route target references unknown provider `{}`",
                target.provider_id
            )
        })?;
        if !provider.models.contains(&target.model) || !seen.insert(target) {
            return Err(format!(
                "route chain `{name}` has an invalid or duplicate target"
            ));
        }
    }
    Ok(())
}

fn validate_tightening(narrow: RouteBudget, broad: RouteBudget) -> Result<(), String> {
    let valid = narrow.max_requests <= broad.max_requests
        && narrow.max_input_bytes <= broad.max_input_bytes
        && narrow.max_output_tokens <= broad.max_output_tokens
        && narrow.timeout_ms <= broad.timeout_ms
        && narrow.max_cost_microusd <= broad.max_cost_microusd
        && (!narrow.allow_unknown_cost || broad.allow_unknown_cost);
    valid
        .then_some(())
        .ok_or_else(|| "case budgets may tighten but not widen their owner budget".to_string())
}

fn exceeds(current: BudgetUsage, add: BudgetUsage, budget: RouteBudget) -> bool {
    current.requests.saturating_add(add.requests) > budget.max_requests
        || current.input_bytes.saturating_add(add.input_bytes) > budget.max_input_bytes
        || current.cost_microusd.saturating_add(add.cost_microusd) > budget.max_cost_microusd
}

fn add_usage(current: &mut BudgetUsage, add: BudgetUsage) {
    current.requests = current.requests.saturating_add(add.requests);
    current.input_bytes = current.input_bytes.saturating_add(add.input_bytes);
    current.cost_microusd = current.cost_microusd.saturating_add(add.cost_microusd);
}

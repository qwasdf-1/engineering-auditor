use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{CostClass, ProviderConfig, RouteBudget, RouteTarget};
use crate::{ContentDigest, StableId};

pub const REVIEW_ROUTES_V2_SCHEMA_VERSION: u32 = 4;
pub const REVIEW_PROMPT_BUNDLE_V2_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct V2ProviderChain {
    pub targets: Vec<RouteTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRoutesV2 {
    pub schema_version: u32,
    pub providers: Vec<ProviderConfig>,
    pub single_reviewer: V2ProviderChain,
    pub review_budget: RouteBudget,
    /// What one question may spend; it may tighten the review budget but
    /// not widen it.
    pub question_budget: RouteBudget,
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
        self.question_budget.validate()?;
        validate_tightening(self.question_budget, self.review_budget)?;
        let providers = provider_registry(&self.providers, self.review_budget)?;
        validate_chain(&self.single_reviewer, &providers, "single_reviewer")
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|provider| provider.id == id)
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
    valid.then_some(()).ok_or_else(|| {
        "the question budget may tighten but not widen the review budget".to_string()
    })
}

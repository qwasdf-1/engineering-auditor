use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::CaseTemplateDefinition;
use crate::case_review::identity::{canonical_json, CanonicalIdentity};
use crate::{CaseKind, ContentDigest, ContractKind, InputContract};

pub const CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateStatus {
    Production,
    Shadow,
    NotEnabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseTemplateEntry {
    pub kind: CaseKind,
    pub version: u32,
    pub status: TemplateStatus,
    pub template: Option<CaseTemplateDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseTemplateBundle {
    pub schema_version: u32,
    pub bundle_version: u32,
    pub entries: Vec<CaseTemplateEntry>,
}

impl CaseTemplateBundle {
    pub fn checked_in() -> Result<Self, String> {
        Self::from_json(include_str!(
            "../../../../../config/auditor/review-case-templates-v1.json"
        ))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let bundle: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid case template bundle: {error}"))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        Self::from_json(&canonical_json(value))
    }

    pub fn to_canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).expect("template bundle is serializable"))
    }

    pub fn digest(&self) -> ContentDigest {
        ContentDigest::from_canonical_value(&self.identity_fields())
    }

    pub fn validate(&self) -> Result<(), String> {
        super::validate::validate_bundle(self)
    }

    pub fn entry(&self, kind: CaseKind) -> Option<&CaseTemplateEntry> {
        self.entries.iter().find(|entry| entry.kind == kind)
    }
}

impl CanonicalIdentity for CaseTemplateBundle {
    fn identity_fields(&self) -> Value {
        serde_json::to_value(self).expect("template bundle is serializable")
    }
}

impl InputContract for CaseTemplateBundle {
    fn contract_kind(&self) -> ContractKind {
        ContractKind::CaseTemplateBundle
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION
    }

    fn validate_contract(&self) -> Result<(), String> {
        self.validate()
    }
}

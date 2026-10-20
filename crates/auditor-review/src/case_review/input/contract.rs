use serde::Serialize;
use serde_json::Value;

use crate::case_review::identity::CanonicalIdentity;
use crate::{ContentDigest, StableId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    CaseTemplateBundle,
    PromptBundle,
    RouteConfiguration,
}

impl ContractKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::CaseTemplateBundle => "case-template-bundle",
            Self::PromptBundle => "prompt-bundle",
            Self::RouteConfiguration => "route-configuration",
        }
    }
}

pub trait InputContract: CanonicalIdentity {
    fn contract_kind(&self) -> ContractKind;
    fn schema_version(&self) -> u32;
    fn supported_schema_version(&self) -> u32;
    fn validate_contract(&self) -> Result<(), String>;
}

#[derive(Clone, Debug, Serialize)]
pub struct VerifiedContract {
    pub(super) kind: ContractKind,
    pub(super) schema_version: u32,
    pub(super) id: StableId,
    pub(super) digest: ContentDigest,
    pub(super) semantic_snapshot: Value,
}

impl VerifiedContract {
    pub fn kind(&self) -> ContractKind {
        self.kind
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn id(&self) -> &StableId {
        &self.id
    }

    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }

    pub fn semantic_snapshot(&self) -> &Value {
        &self.semantic_snapshot
    }
}

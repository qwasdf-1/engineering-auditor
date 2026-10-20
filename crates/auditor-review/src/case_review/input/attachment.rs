use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::case_review::identity::CanonicalIdentity;
use crate::{ContentDigest, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAttachmentKind {
    RuntimeMeasurement,
    Benchmark,
    TraceSummary,
}

impl EvidenceAttachmentKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeMeasurement => "runtime-measurement",
            Self::Benchmark => "benchmark",
            Self::TraceSummary => "trace-summary",
        }
    }
}

pub trait EvidenceAttachmentAdapter: CanonicalIdentity {
    fn attachment_kind(&self) -> EvidenceAttachmentKind;
    fn schema_version(&self) -> u32;
    fn supported_schema_version(&self) -> u32;
    fn validate_attachment(&self) -> Result<(), String>;
}

#[derive(Clone, Debug, Serialize)]
pub struct EvidenceAttachment {
    pub(super) kind: EvidenceAttachmentKind,
    pub(super) schema_version: u32,
    pub(super) id: StableId,
    pub(super) digest: ContentDigest,
    pub(super) semantic_snapshot: Value,
}

impl EvidenceAttachment {
    pub fn kind(&self) -> EvidenceAttachmentKind {
        self.kind
    }

    pub fn id(&self) -> &StableId {
        &self.id
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }

    pub fn semantic_snapshot(&self) -> &Value {
        &self.semantic_snapshot
    }
}

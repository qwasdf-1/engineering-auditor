use serde_json::Value;

use super::packet::EvidencePacketSemantics;
use super::{EvidenceError, EVIDENCE_PACKET_SCHEMA_VERSION};
use crate::{ContentDigest, StableId};

pub(super) fn packet_semantic_value(
    semantics: &EvidencePacketSemantics,
) -> Result<Value, EvidenceError> {
    serde_json::to_value(semantics).map_err(|error| EvidenceError::InvalidPacket(error.to_string()))
}

pub(super) fn packet_id(semantic: &Value) -> Result<StableId, EvidenceError> {
    StableId::from_identity("evidence-packet", EVIDENCE_PACKET_SCHEMA_VERSION, semantic)
        .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))
}

pub(super) fn packet_digest(semantic: &Value) -> ContentDigest {
    ContentDigest::from_canonical_value(semantic)
}

pub(super) fn generated_item_id(prefix: &str, content: &Value) -> Result<String, EvidenceError> {
    StableId::from_identity(prefix, EVIDENCE_PACKET_SCHEMA_VERSION, content)
        .map(|id| id.to_string())
        .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))
}

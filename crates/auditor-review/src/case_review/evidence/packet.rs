use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::identity::{packet_digest, packet_id, packet_semantic_value};
use super::{EvidenceError, EvidencePacketBudget};
use crate::{ClaimSlot, ContentDigest, EvidenceClass, StableId, TrustLabel};

pub const EVIDENCE_PACKET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceItemKind {
    Finding,
    Fact,
    Measurement,
    SourceSpan,
    Symbol,
    Edge,
    Capability,
    BriefConstraint,
    Test,
}

impl EvidenceItemKind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Finding => "finding:",
            Self::Fact => "fact:",
            Self::Measurement => "measurement:",
            Self::SourceSpan => "source-span:",
            Self::Symbol => "symbol:",
            Self::Edge => "edge:",
            Self::Capability => "capability:",
            Self::BriefConstraint => "brief-constraint:",
            Self::Test => "test:",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: EvidenceItemKind,
    pub trust: TrustLabel,
    pub provenance: String,
    pub digest: ContentDigest,
    pub content: Value,
}

impl EvidenceItem {
    pub(crate) fn new(
        id: String,
        kind: EvidenceItemKind,
        trust: TrustLabel,
        provenance: String,
        content: Value,
    ) -> Result<Self, EvidenceError> {
        if !id.starts_with(kind.prefix())
            || id.len() == kind.prefix().len()
            || provenance.trim().is_empty()
        {
            return Err(EvidenceError::InvalidPacket(
                "evidence item has an invalid typed id or provenance".to_string(),
            ));
        }
        let digest = ContentDigest::from_canonical_value(&content);
        Ok(Self {
            id,
            kind,
            trust,
            provenance,
            digest,
            content,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceIndexes {
    pub required: BTreeMap<String, Vec<String>>,
    pub optional: BTreeMap<String, Vec<String>>,
    pub blocking: BTreeMap<String, Vec<String>>,
    pub counterevidence: BTreeMap<String, Vec<String>>,
    pub unavailable: BTreeMap<String, EvidenceUnavailable>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceUnavailable {
    pub evidence_class: EvidenceClass,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedEvidenceNeighborhood {
    pub finding_ids: Vec<String>,
    pub graph_node_ids: Vec<String>,
    pub map_symbol_ids: Vec<String>,
    pub maximum_graph_hops: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosureState {
    Closed,
    PartiallyClosed,
    Blocked,
    OverBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceClosure {
    pub state: ClosureState,
    pub reasons: Vec<String>,
    pub missing_evidence_classes: Vec<EvidenceClass>,
    pub affected_claim_slots: Vec<ClaimSlot>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOmissionReason {
    Unavailable,
    TargetBudget,
    HardBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceOmission {
    pub requirement_id: String,
    pub item_id: Option<String>,
    pub reason: EvidenceOmissionReason,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePacket {
    pub schema_version: u32,
    pub packet_id: StableId,
    pub packet_digest: ContentDigest,
    pub case_id: StableId,
    pub input_binding_id: StableId,
    pub items: Vec<EvidenceItem>,
    pub indexes: EvidenceIndexes,
    pub allowed_neighborhood: AllowedEvidenceNeighborhood,
    pub budget: EvidencePacketBudget,
    pub closure: EvidenceClosure,
    pub omissions: Vec<EvidenceOmission>,
    pub byte_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EvidencePacketSemantics {
    pub schema_version: u32,
    pub case_id: StableId,
    pub input_binding_id: StableId,
    pub items: Vec<EvidenceItem>,
    pub indexes: EvidenceIndexes,
    pub allowed_neighborhood: AllowedEvidenceNeighborhood,
    pub budget: EvidencePacketBudget,
    pub closure: EvidenceClosure,
    pub omissions: Vec<EvidenceOmission>,
}

impl EvidencePacket {
    pub(crate) fn build(mut semantics: EvidencePacketSemantics) -> Result<Self, EvidenceError> {
        semantics
            .items
            .sort_by(|left, right| left.id.cmp(&right.id));
        sort_index(&mut semantics.indexes.required);
        sort_index(&mut semantics.indexes.optional);
        sort_index(&mut semantics.indexes.blocking);
        sort_index(&mut semantics.indexes.counterevidence);
        semantics.allowed_neighborhood.finding_ids.sort();
        semantics.allowed_neighborhood.finding_ids.dedup();
        semantics.allowed_neighborhood.graph_node_ids.sort();
        semantics.allowed_neighborhood.graph_node_ids.dedup();
        semantics.allowed_neighborhood.map_symbol_ids.sort();
        semantics.allowed_neighborhood.map_symbol_ids.dedup();
        semantics.closure.reasons.sort();
        semantics.closure.reasons.dedup();
        semantics.closure.missing_evidence_classes.sort();
        semantics.closure.missing_evidence_classes.dedup();
        semantics.closure.affected_claim_slots.sort();
        semantics.closure.affected_claim_slots.dedup();
        semantics.omissions.sort_by(|left, right| {
            (&left.requirement_id, &left.item_id).cmp(&(&right.requirement_id, &right.item_id))
        });
        let semantic = packet_semantic_value(&semantics)?;
        let byte_count = serde_json::to_vec(&semantic)
            .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?
            .len();
        let packet_id = packet_id(&semantic)?;
        let packet_digest = packet_digest(&semantic);
        let packet = Self {
            schema_version: EVIDENCE_PACKET_SCHEMA_VERSION,
            packet_id,
            packet_digest,
            case_id: semantics.case_id,
            input_binding_id: semantics.input_binding_id,
            items: semantics.items,
            indexes: semantics.indexes,
            allowed_neighborhood: semantics.allowed_neighborhood,
            budget: semantics.budget,
            closure: semantics.closure,
            omissions: semantics.omissions,
            byte_count,
        };
        packet.validate()?;
        Ok(packet)
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        self.budget.validate()?;
        if self.schema_version != EVIDENCE_PACKET_SCHEMA_VERSION
            || self.byte_count > self.budget.hard_packet_bytes
        {
            return Err(EvidenceError::InvalidPacket(
                "schema version or hard packet ceiling is invalid".to_string(),
            ));
        }
        let mut ids = BTreeSet::new();
        if self.items.iter().any(|item| {
            !item.id.starts_with(item.kind.prefix())
                || !ids.insert(item.id.as_str())
                || item.digest != ContentDigest::from_canonical_value(&item.content)
        }) {
            return Err(EvidenceError::InvalidPacket(
                "evidence items are invalid, duplicated, or stale".to_string(),
            ));
        }
        let indexed = self
            .indexes
            .required
            .values()
            .chain(self.indexes.optional.values())
            .chain(self.indexes.blocking.values())
            .chain(self.indexes.counterevidence.values())
            .flatten();
        if indexed.into_iter().any(|id| !ids.contains(id.as_str())) {
            return Err(EvidenceError::InvalidPacket(
                "evidence index references an absent item".to_string(),
            ));
        }
        let semantic = packet_semantic_value(&EvidencePacketSemantics {
            schema_version: self.schema_version,
            case_id: self.case_id.clone(),
            input_binding_id: self.input_binding_id.clone(),
            items: self.items.clone(),
            indexes: self.indexes.clone(),
            allowed_neighborhood: self.allowed_neighborhood.clone(),
            budget: self.budget,
            closure: self.closure.clone(),
            omissions: self.omissions.clone(),
        })?;
        if self.packet_id != packet_id(&semantic)?
            || self.packet_digest != packet_digest(&semantic)
            || self.byte_count
                != serde_json::to_vec(&semantic)
                    .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?
                    .len()
        {
            return Err(EvidenceError::InvalidPacket(
                "packet identity does not match canonical semantics".to_string(),
            ));
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> String {
        crate::case_review::identity::canonical_json(
            &serde_json::to_value(self).expect("evidence packet is serializable"),
        )
    }
}

fn sort_index(index: &mut BTreeMap<String, Vec<String>>) {
    for ids in index.values_mut() {
        ids.sort();
        ids.dedup();
    }
}

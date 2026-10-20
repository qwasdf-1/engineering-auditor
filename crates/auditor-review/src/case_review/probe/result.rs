use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::validate::validate_result;
use super::{ProbeDefinition, ProbeError};
use crate::{EvidencePacket, StableId};

pub const PROBE_RESULT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeAnswer {
    Yes,
    No,
    Unknown,
    NotApplicable,
    ConflictingEvidence,
}

impl ProbeAnswer {
    pub const ALL: [Self; 5] = [
        Self::Yes,
        Self::No,
        Self::Unknown,
        Self::NotApplicable,
        Self::ConflictingEvidence,
    ];

    pub const fn is_definitive(self) -> bool {
        matches!(self, Self::Yes | Self::No | Self::NotApplicable)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolverKind {
    ExistingDeterministicFact,
    EvidenceGraphQuery,
    RepositoryMapQuery,
    BoundRuntimeMeasurement,
    OperatorBrief,
    ModelInterpretation,
    UnknownFallback,
}

impl ResolverKind {
    pub const PRECEDENCE: [Self; 7] = [
        Self::ExistingDeterministicFact,
        Self::EvidenceGraphQuery,
        Self::RepositoryMapQuery,
        Self::BoundRuntimeMeasurement,
        Self::OperatorBrief,
        Self::ModelInterpretation,
        Self::UnknownFallback,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolverRecord {
    pub kind: ResolverKind,
    pub id: String,
    pub version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeResolution {
    pub answer: ProbeAnswer,
    pub evidence_ids: Vec<String>,
    pub explanation: String,
    pub limitations: Vec<String>,
    pub reviewer_run_id: Option<StableId>,
}

impl ProbeResolution {
    pub fn new(
        answer: ProbeAnswer,
        evidence_ids: Vec<String>,
        explanation: impl Into<String>,
        limitations: Vec<String>,
    ) -> Self {
        Self {
            answer,
            evidence_ids,
            explanation: explanation.into(),
            limitations,
            reviewer_run_id: None,
        }
    }

    pub fn model(
        answer: ProbeAnswer,
        evidence_ids: Vec<String>,
        explanation: impl Into<String>,
        limitations: Vec<String>,
        reviewer_run_id: StableId,
    ) -> Self {
        Self {
            answer,
            evidence_ids,
            explanation: explanation.into(),
            limitations,
            reviewer_run_id: Some(reviewer_run_id),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeResult {
    pub schema_version: u32,
    pub result_id: StableId,
    pub probe_id: StableId,
    pub case_id: StableId,
    pub answer: ProbeAnswer,
    pub resolver: ResolverRecord,
    pub evidence_ids: Vec<String>,
    pub explanation: String,
    pub limitations: Vec<String>,
    pub reviewer_run_id: Option<StableId>,
}

impl ProbeResult {
    pub(crate) fn from_resolution(
        definition: &ProbeDefinition,
        resolver: ResolverRecord,
        mut resolution: ProbeResolution,
        packet: &EvidencePacket,
    ) -> Result<Self, ProbeError> {
        resolution.evidence_ids.sort();
        resolution.evidence_ids.dedup();
        resolution.limitations.sort();
        resolution.limitations.dedup();
        let result_id = derive_result_id(definition, &resolver, &resolution)?;
        let result = Self {
            schema_version: PROBE_RESULT_SCHEMA_VERSION,
            result_id,
            probe_id: definition.probe_id.clone(),
            case_id: definition.case_id.clone(),
            answer: resolution.answer,
            resolver,
            evidence_ids: resolution.evidence_ids,
            explanation: resolution.explanation,
            limitations: resolution.limitations,
            reviewer_run_id: resolution.reviewer_run_id,
        };
        result.validate(definition, packet)?;
        Ok(result)
    }

    pub fn validate(
        &self,
        definition: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<(), ProbeError> {
        validate_result(self, definition, packet)
    }
}

pub(super) fn derive_result_id(
    definition: &ProbeDefinition,
    resolver: &ResolverRecord,
    resolution: &ProbeResolution,
) -> Result<StableId, ProbeError> {
    let identity = json!({
        "answer": resolution.answer,
        "case_id": definition.case_id,
        "evidence_ids": resolution.evidence_ids,
        "explanation": resolution.explanation,
        "limitations": resolution.limitations,
        "probe_id": definition.probe_id,
        "resolver": resolver,
        "reviewer_run_id": resolution.reviewer_run_id,
    });
    StableId::from_identity("probe-result", PROBE_RESULT_SCHEMA_VERSION, &identity)
        .map_err(|error| ProbeError::InvalidResult(error.to_string()))
}

pub(super) fn combined_resolution(
    answer: ProbeAnswer,
    resolutions: &[ProbeResolution],
    explanation: String,
) -> ProbeResolution {
    let evidence_ids = resolutions
        .iter()
        .flat_map(|resolution| resolution.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let limitations = resolutions
        .iter()
        .flat_map(|resolution| resolution.limitations.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    ProbeResolution {
        answer,
        evidence_ids,
        explanation,
        limitations,
        reviewer_run_id: resolutions
            .iter()
            .find_map(|resolution| resolution.reviewer_run_id.clone()),
    }
}

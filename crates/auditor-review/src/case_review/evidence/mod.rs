mod budget;
mod case_validation;
mod compile;
mod graph_candidates;
mod identity;
mod lookup;
mod neighborhood;
mod packet;
mod packet_compile;
mod union;

use std::fmt;

use crate::RepositoryMapError;

pub use budget::{
    EvidencePacketBudget, ABSOLUTE_SOURCE_EXCERPTS, EVIDENCE_UNION_ROUNDS, HARD_PACKET_BYTES,
    INITIAL_SOURCE_EXCERPTS, MAXIMUM_GRAPH_HOPS, TARGET_PACKET_BYTES,
};
pub use compile::EvidenceCompiler;
pub use lookup::{
    EvidenceLookup, EvidenceLookupRequest, EvidenceLookupRequestKind, EvidenceLookupResult,
};
pub use packet::{
    AllowedEvidenceNeighborhood, ClosureState, EvidenceClosure, EvidenceIndexes, EvidenceItem,
    EvidenceItemKind, EvidenceOmission, EvidenceOmissionReason, EvidencePacket,
    EvidenceUnavailable, EVIDENCE_PACKET_SCHEMA_VERSION,
};
pub use union::{
    plan_evidence_union, EvidenceUnionBoundary, EvidenceUnionPlan, ReviewerEvidenceRequest,
    UnionEvidenceRequest,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceError {
    CaseBindingMismatch,
    InvalidBudget(String),
    InvalidPacket(String),
    InvalidLookup(String),
    RepositoryMap(RepositoryMapError),
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CaseBindingMismatch => {
                formatter.write_str("evidence case and review input binding do not match")
            }
            Self::InvalidBudget(detail) => write!(formatter, "invalid evidence budget: {detail}"),
            Self::InvalidPacket(detail) => write!(formatter, "invalid evidence packet: {detail}"),
            Self::InvalidLookup(detail) => write!(formatter, "invalid evidence lookup: {detail}"),
            Self::RepositoryMap(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EvidenceError {}

impl From<RepositoryMapError> for EvidenceError {
    fn from(error: RepositoryMapError) -> Self {
        Self::RepositoryMap(error)
    }
}

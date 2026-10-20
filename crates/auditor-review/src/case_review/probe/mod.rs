mod definition;
mod deterministic;
mod resolver;
mod result;
mod validate;

use std::fmt;

pub use definition::{
    OptionalProbeRequest, ProbeDecisionTarget, ProbeDefinition, ProbeLookupPlan,
    ProbeLookupPlanner, ProbeOrigin, ProbePlanner, PROBE_DEFINITION_SCHEMA_VERSION,
};
pub use resolver::{
    ProbeResolutionError, ProbeResolver, ProbeResolverRegistry, ResolverRegistration,
};
pub use result::{
    ProbeAnswer, ProbeResolution, ProbeResult, ResolverKind, ResolverRecord,
    PROBE_RESULT_SCHEMA_VERSION,
};
pub(crate) use validate::{
    admissible_evidence_ids, evidence_class_compatible, evidence_classes_for_item,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeError {
    InvalidDefinition(String),
    InvalidOptionalProbe(String),
    InvalidResult(String),
    ResolverRegistry(String),
    Resolution(ProbeResolutionError),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDefinition(detail) => {
                write!(formatter, "invalid probe definition: {detail}")
            }
            Self::InvalidOptionalProbe(detail) => {
                write!(formatter, "invalid optional probe: {detail}")
            }
            Self::InvalidResult(detail) => write!(formatter, "invalid probe result: {detail}"),
            Self::ResolverRegistry(detail) => {
                write!(formatter, "invalid resolver registry: {detail}")
            }
            Self::Resolution(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<ProbeResolutionError> for ProbeError {
    fn from(error: ProbeResolutionError) -> Self {
        Self::Resolution(error)
    }
}

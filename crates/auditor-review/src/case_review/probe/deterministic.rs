use serde_json::Value;

use super::{
    ProbeAnswer, ProbeDefinition, ProbeResolution, ProbeResolutionError, ProbeResolver,
    ResolverKind, ResolverRegistration,
};
use crate::{EngineeringCase, EvidenceItem, EvidenceItemKind, EvidencePacket, ProbeKind};

pub(super) fn builtin_registrations() -> Vec<ResolverRegistration> {
    vec![
        ResolverRegistration::new(
            ResolverKind::ExistingDeterministicFact,
            Box::new(ExistingFactResolver),
        ),
        ResolverRegistration::new(ResolverKind::EvidenceGraphQuery, Box::new(GraphResolver)),
        ResolverRegistration::new(ResolverKind::RepositoryMapQuery, Box::new(MapResolver)),
        ResolverRegistration::new(
            ResolverKind::BoundRuntimeMeasurement,
            Box::new(MeasurementResolver),
        ),
        ResolverRegistration::new(ResolverKind::OperatorBrief, Box::new(OperatorBriefResolver)),
    ]
}

struct ExistingFactResolver;

impl ProbeResolver for ExistingFactResolver {
    fn id(&self) -> &'static str {
        "existing-deterministic-fact"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        probe.kind == ProbeKind::Existence
    }

    fn resolve(
        &self,
        case: &EngineeringCase,
        _probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let evidence_ids = matching_evidence_ids(packet, |item| {
            item.kind == EvidenceItemKind::Finding
                && case
                    .finding_ids
                    .iter()
                    .any(|id| item.id == format!("finding:{id}"))
        });
        required_resolution(
            ProbeAnswer::Yes,
            evidence_ids,
            "the selected deterministic finding records establish existence",
            "selected deterministic finding records are absent",
        )
    }
}

struct GraphResolver;

impl ProbeResolver for GraphResolver {
    fn id(&self) -> &'static str {
        "evidence-graph-query"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        matches!(
            probe.kind,
            ProbeKind::Identity
                | ProbeKind::Ordering
                | ProbeKind::Mutation
                | ProbeKind::ConsumerRequirement
        )
    }

    fn resolve(
        &self,
        _case: &EngineeringCase,
        _probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let evidence_ids = matching_evidence_ids(packet, |item| {
            matches!(
                item.kind,
                EvidenceItemKind::Fact | EvidenceItemKind::Edge | EvidenceItemKind::Symbol
            ) && item.provenance.contains("evidence-graph")
        });
        unknown_from_evidence(
            evidence_ids,
            "the case packet has no graph facts for this probe",
            "the graph bounds the relevant neighborhood but does not encode this semantic answer",
            "semantic interpretation remains required",
        )
    }
}

struct MapResolver;

impl ProbeResolver for MapResolver {
    fn id(&self) -> &'static str {
        "repository-map-query"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        matches!(
            probe.kind,
            ProbeKind::Lifecycle | ProbeKind::VerificationAvailability
        )
    }

    fn resolve(
        &self,
        _case: &EngineeringCase,
        _probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let evidence_ids = matching_evidence_ids(packet, |item| {
            item.kind == EvidenceItemKind::Symbol && item.provenance.starts_with("repository-map#")
        });
        unknown_from_evidence(
            evidence_ids,
            "no mapped case symbol is available",
            "mapped symbols identify current owners but do not prove a safe wider lifetime",
            "ownership and invalidation semantics are not encoded by the map",
        )
    }
}

struct MeasurementResolver;

impl ProbeResolver for MeasurementResolver {
    fn id(&self) -> &'static str {
        "bound-runtime-measurement"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        probe.kind == ProbeKind::Cost
    }

    fn resolve(
        &self,
        _case: &EngineeringCase,
        _probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let measurements = packet
            .items
            .iter()
            .filter(|item| {
                item.kind == EvidenceItemKind::Measurement
                    && item.provenance.starts_with("input-binding#")
            })
            .collect::<Vec<_>>();
        if measurements.is_empty() {
            return Err(ProbeResolutionError::Unavailable(
                "no bound runtime measurement is available".to_string(),
            ));
        }
        let material = measurements
            .iter()
            .filter_map(|item| find_bool(&item.content, "material"))
            .collect::<Vec<_>>();
        let evidence_ids = measurements
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if material.contains(&true) && material.contains(&false) {
            return Ok(ProbeResolution::new(
                ProbeAnswer::ConflictingEvidence,
                evidence_ids,
                "bound measurements disagree about materiality",
                Vec::new(),
            ));
        }
        if let Some(value) = material.first() {
            return Ok(ProbeResolution::new(
                if *value {
                    ProbeAnswer::Yes
                } else {
                    ProbeAnswer::No
                },
                evidence_ids,
                "a bound measurement supplies an explicit materiality classification",
                Vec::new(),
            ));
        }
        Ok(ProbeResolution::new(
            ProbeAnswer::Unknown,
            evidence_ids,
            "measurements are bound but no operator-relative materiality threshold is present",
            vec!["a raw metric cannot establish material impact without a threshold".to_string()],
        ))
    }
}

struct OperatorBriefResolver;

impl ProbeResolver for OperatorBriefResolver {
    fn id(&self) -> &'static str {
        "operator-brief"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        matches!(
            probe.kind,
            ProbeKind::Constraint
                | ProbeKind::AlternativeFeasibility
                | ProbeKind::VerificationAvailability
        )
    }

    fn resolve(
        &self,
        _case: &EngineeringCase,
        _probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let evidence_ids = matching_evidence_ids(packet, |item| {
            item.kind == EvidenceItemKind::BriefConstraint
        });
        required_resolution(
            ProbeAnswer::Unknown,
            evidence_ids,
            "operator constraints bound the decision but do not answer the probe by themselves",
            "the operator brief contains no bound case constraint",
        )
    }
}

fn required_resolution(
    answer: ProbeAnswer,
    evidence_ids: Vec<String>,
    rationale: &str,
    unavailable: &str,
) -> Result<ProbeResolution, ProbeResolutionError> {
    if evidence_ids.is_empty() {
        return Err(ProbeResolutionError::Unavailable(unavailable.to_string()));
    }
    Ok(ProbeResolution::new(
        answer,
        evidence_ids,
        rationale,
        Vec::new(),
    ))
}

fn find_bool(value: &Value, field: &str) -> Option<bool> {
    value.get(field).and_then(Value::as_bool).or_else(|| {
        value
            .get("semantic_snapshot")
            .and_then(|value| find_bool(value, field))
    })
}

fn matching_evidence_ids(
    packet: &EvidencePacket,
    predicate: impl Fn(&EvidenceItem) -> bool,
) -> Vec<String> {
    packet
        .items
        .iter()
        .filter(|item| predicate(item))
        .map(|item| item.id.clone())
        .collect()
}

fn unknown_from_evidence(
    evidence_ids: Vec<String>,
    unavailable: &str,
    explanation: &str,
    limitation: &str,
) -> Result<ProbeResolution, ProbeResolutionError> {
    if evidence_ids.is_empty() {
        return Err(ProbeResolutionError::Unavailable(unavailable.to_string()));
    }
    Ok(ProbeResolution::new(
        ProbeAnswer::Unknown,
        evidence_ids,
        explanation,
        vec![limitation.to_string()],
    ))
}

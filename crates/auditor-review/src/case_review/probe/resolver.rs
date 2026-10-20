use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::deterministic::builtin_registrations;
use super::result::combined_resolution;
use super::{
    ProbeAnswer, ProbeDefinition, ProbeError, ProbeResolution, ProbeResult, ResolverKind,
    ResolverRecord,
};
use crate::{EngineeringCase, EvidencePacket};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeResolutionError {
    Unavailable(String),
    InvalidEvidence(String),
    UnsupportedProbe(String),
    ResolverFailure(String),
}

impl fmt::Display for ProbeResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(formatter, "probe resolver unavailable: {detail}"),
            Self::InvalidEvidence(detail) => {
                write!(
                    formatter,
                    "probe resolver returned invalid evidence: {detail}"
                )
            }
            Self::UnsupportedProbe(detail) => write!(formatter, "unsupported probe: {detail}"),
            Self::ResolverFailure(detail) => write!(formatter, "probe resolver failed: {detail}"),
        }
    }
}

impl std::error::Error for ProbeResolutionError {}

pub trait ProbeResolver {
    fn id(&self) -> &'static str;
    fn version(&self) -> u32;
    fn supports(&self, probe: &ProbeDefinition) -> bool;
    fn resolve(
        &self,
        case: &EngineeringCase,
        probe: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError>;
}

pub struct ResolverRegistration {
    pub kind: ResolverKind,
    pub resolver: Box<dyn ProbeResolver>,
}

impl ResolverRegistration {
    pub fn new(kind: ResolverKind, resolver: Box<dyn ProbeResolver>) -> Self {
        Self { kind, resolver }
    }
}

pub struct ProbeResolverRegistry {
    resolvers: BTreeMap<ResolverKind, Vec<Box<dyn ProbeResolver>>>,
}

impl ProbeResolverRegistry {
    pub fn built_in() -> Result<Self, ProbeError> {
        Self::new(builtin_registrations())
    }

    pub fn new(registrations: Vec<ResolverRegistration>) -> Result<Self, ProbeError> {
        let mut seen = BTreeSet::new();
        let mut resolvers = BTreeMap::<ResolverKind, Vec<Box<dyn ProbeResolver>>>::new();
        for registration in registrations {
            let resolver = registration.resolver;
            if resolver.id().trim().is_empty()
                || resolver.version() == 0
                || !seen.insert((registration.kind, resolver.id()))
                || registration.kind == ResolverKind::UnknownFallback
            {
                return Err(ProbeError::ResolverRegistry(
                    "resolver identity is empty, duplicated, or host-reserved".to_string(),
                ));
            }
            resolvers
                .entry(registration.kind)
                .or_default()
                .push(resolver);
        }
        for family in resolvers.values_mut() {
            family.sort_by_key(|resolver| resolver.id());
        }
        Ok(Self { resolvers })
    }

    pub fn resolve(
        &self,
        case: &EngineeringCase,
        definition: &ProbeDefinition,
        packet: &EvidencePacket,
    ) -> Result<ProbeResult, ProbeError> {
        definition.validate(case, packet)?;
        let mut accumulated_limitations = Vec::new();
        for kind in &definition.resolver_precedence {
            if *kind == ResolverKind::UnknownFallback {
                break;
            }
            let Some(family) = self.resolvers.get(kind) else {
                accumulated_limitations.push(format!("{kind:?} resolver is unavailable"));
                continue;
            };
            if let Some(result) = resolve_family(FamilyResolutionContext {
                kind: *kind,
                family,
                case,
                definition,
                packet,
                accumulated_limitations: &mut accumulated_limitations,
            })? {
                return Ok(result);
            }
        }
        ProbeResult::from_resolution(
            definition,
            ResolverRecord {
                kind: ResolverKind::UnknownFallback,
                id: "host-unknown-fallback".to_string(),
                version: 1,
            },
            ProbeResolution::new(
                ProbeAnswer::Unknown,
                Vec::new(),
                "no allowed resolver produced a definitive answer",
                accumulated_limitations,
            ),
            packet,
        )
    }
}

struct FamilyResolutionContext<'a> {
    kind: ResolverKind,
    family: &'a [Box<dyn ProbeResolver>],
    case: &'a EngineeringCase,
    definition: &'a ProbeDefinition,
    packet: &'a EvidencePacket,
    accumulated_limitations: &'a mut Vec<String>,
}

fn resolve_family(context: FamilyResolutionContext<'_>) -> Result<Option<ProbeResult>, ProbeError> {
    let FamilyResolutionContext {
        kind,
        family,
        case,
        definition,
        packet,
        accumulated_limitations,
    } = context;
    let mut resolutions = Vec::<(&dyn ProbeResolver, ProbeResolution)>::new();
    for resolver in family
        .iter()
        .map(Box::as_ref)
        .filter(|resolver| resolver.supports(definition))
    {
        match resolver.resolve(case, definition, packet) {
            Ok(resolution) => {
                super::validate::validate_resolution(&resolution, definition, packet, kind)?;
                resolutions.push((resolver, resolution));
            }
            Err(ProbeResolutionError::Unavailable(detail)) => {
                accumulated_limitations.push(format!("{}: {detail}", resolver.id()));
            }
            Err(error) => return Err(error.into()),
        }
    }
    if resolutions.is_empty() {
        return Ok(None);
    }
    let answers = resolutions
        .iter()
        .map(|(_, resolution)| resolution.answer)
        .filter(|answer| *answer != ProbeAnswer::Unknown)
        .collect::<BTreeSet<_>>();
    if answers.contains(&ProbeAnswer::ConflictingEvidence) || answers.len() > 1 {
        let values = resolutions
            .iter()
            .map(|(_, resolution)| resolution.clone())
            .collect::<Vec<_>>();
        return ProbeResult::from_resolution(
            definition,
            family_record(kind, &resolutions),
            combined_resolution(
                ProbeAnswer::ConflictingEvidence,
                &values,
                "authoritative resolvers in one precedence family disagree".to_string(),
            ),
            packet,
        )
        .map(Some);
    }
    let Some(answer) = answers.first().copied() else {
        accumulated_limitations.extend(
            resolutions
                .into_iter()
                .flat_map(|(_, resolution)| resolution.limitations),
        );
        return Ok(None);
    };
    if answer.is_definitive() {
        let values = resolutions
            .iter()
            .filter(|(_, resolution)| resolution.answer == answer)
            .map(|(_, resolution)| resolution.clone())
            .collect::<Vec<_>>();
        return ProbeResult::from_resolution(
            definition,
            family_record(kind, &resolutions),
            combined_resolution(
                answer,
                &values,
                values
                    .iter()
                    .map(|resolution| resolution.explanation.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            packet,
        )
        .map(Some);
    }
    Ok(None)
}

fn family_record(
    kind: ResolverKind,
    resolutions: &[(&dyn ProbeResolver, ProbeResolution)],
) -> ResolverRecord {
    let ids = resolutions
        .iter()
        .map(|(resolver, _)| format!("{}@{}", resolver.id(), resolver.version()))
        .collect::<Vec<_>>()
        .join("+");
    ResolverRecord {
        kind,
        id: ids,
        version: 1,
    }
}

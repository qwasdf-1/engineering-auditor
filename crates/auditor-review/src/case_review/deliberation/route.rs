use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    CaseClosure, CaseFailure, ClosureState, EngineeringCase, EvidencePacket, ProbeAnswer,
    ProbeDefinition, ProbeResult, ResolverKind, RoutePredicate,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliberationMode {
    Deterministic,
    SingleReviewer,
    Ensemble,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeReason {
    NotSubstantiated,
    EvidenceUnavailable,
    ActionFullyDetermined,
    NarrowUnresolvedJudgment,
    ResourceLifetimeChange,
    ComponentOwnershipChange,
    PublicContractChange,
    CriticalSemanticProbe,
    ThreeLayerSpan,
    MultipleComponents,
    MultipleCredibleActions,
    MaterialEvidenceConflict,
    SignificantBenefitClaim,
    CombinedMediumConsequenceUncertaintyScope,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModeSelection {
    pub mode: DeliberationMode,
    pub reasons: Vec<ModeReason>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeterministicDisposition {
    NotSubstantiated,
    EvidenceUnavailable,
    ActionFullyDetermined,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionMagnitude {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePredicateEvidence {
    pub predicate: RoutePredicate,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingFacts {
    pub deterministic_disposition: DeterministicDisposition,
    pub predicate_evidence: Vec<RoutePredicateEvidence>,
    pub operator_intent_known: bool,
    pub local_and_reversible: bool,
    pub credible_action_families: usize,
    pub higher_order_inference_required: bool,
    pub consequence: DecisionMagnitude,
    pub uncertainty: DecisionMagnitude,
    pub scope: DecisionMagnitude,
    pub magnitude_evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeAvailability {
    pub single_reviewer: bool,
    pub ensemble: bool,
}

impl ModeAvailability {
    pub const fn pre_ensemble() -> Self {
        Self {
            single_reviewer: true,
            ensemble: false,
        }
    }

    pub const fn with_shadow_ensemble(enabled: bool) -> Self {
        Self {
            single_reviewer: true,
            ensemble: enabled,
        }
    }
}

impl Default for ModeAvailability {
    fn default() -> Self {
        Self::pre_ensemble()
    }
}

pub struct RoutingInput<'a> {
    pub case: &'a EngineeringCase,
    pub packet: &'a EvidencePacket,
    pub definitions: &'a [ProbeDefinition],
    pub results: &'a [ProbeResult],
    pub facts: &'a RoutingFacts,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModeRoutingOutcome {
    pub selection: ModeSelection,
    pub closure: Option<CaseClosure>,
}

impl ModeRoutingOutcome {
    pub const fn is_ready(&self) -> bool {
        self.closure.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeliberationRouter {
    availability: ModeAvailability,
}

impl DeliberationRouter {
    pub const fn new(availability: ModeAvailability) -> Self {
        Self { availability }
    }

    pub fn route(&self, input: RoutingInput<'_>) -> Result<ModeRoutingOutcome, RoutingError> {
        validate_routing_input(&input)?;
        let selection = select_mode(&input)?;
        let failure = if !input.facts.operator_intent_known {
            Some((
                CaseFailure::OperatorInputRequired,
                "operator_intent_unknown",
            ))
        } else {
            match selection.mode {
                DeliberationMode::SingleReviewer if !self.availability.single_reviewer => Some((
                    CaseFailure::ModelRouteUnavailable,
                    "single_reviewer_route_unavailable",
                )),
                DeliberationMode::Ensemble if !self.availability.ensemble => {
                    Some((CaseFailure::ModeUnavailable, "ensemble_mode_not_enabled"))
                }
                _ => None,
            }
        };
        let closure = failure.map(|(failure, detail)| {
            CaseClosure::failed(
                input.case.case_id.clone(),
                failure,
                vec![detail.to_string()],
            )
        });
        Ok(ModeRoutingOutcome { selection, closure })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingError {
    CaseBinding,
    IncompleteMandatoryProbes,
    InvalidProbeResult,
    InvalidPredicateEvidence,
    InconsistentDisposition,
    IncompleteRoutingFacts,
    ModeForbidden,
}

impl fmt::Display for RoutingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::CaseBinding => "routing input is not bound to one case and packet",
            Self::IncompleteMandatoryProbes => {
                "routing requires exactly one result for every mandatory probe"
            }
            Self::InvalidProbeResult => "routing received an invalid or unmatched probe result",
            Self::InvalidPredicateEvidence => {
                "route predicate evidence is unsupported, empty, or outside the packet"
            }
            Self::InconsistentDisposition => {
                "deterministic disposition is inconsistent with the probe results"
            }
            Self::IncompleteRoutingFacts => {
                "routing facts do not justify deterministic, single, or ensemble mode"
            }
            Self::ModeForbidden => "the case template forbids the selected deliberation mode",
        };
        formatter.write_str(detail)
    }
}

impl std::error::Error for RoutingError {}

fn validate_routing_input(input: &RoutingInput<'_>) -> Result<(), RoutingError> {
    if input.packet.case_id != input.case.case_id
        || input.packet.input_binding_id != input.case.input_binding_id
    {
        return Err(RoutingError::CaseBinding);
    }
    let definitions = unique_definitions(input)?;
    let results = unique_results(input, &definitions)?;
    let mandatory = input
        .case
        .mandatory_probes
        .iter()
        .map(|probe| &probe.probe_id)
        .collect::<BTreeSet<_>>();
    if !mandatory
        .iter()
        .all(|probe_id| definitions.contains_key(probe_id) && results.contains_key(probe_id))
    {
        return Err(RoutingError::IncompleteMandatoryProbes);
    }
    validate_predicate_evidence(input)?;
    validate_disposition(input, &mandatory, &results)
}

fn unique_definitions<'a>(
    input: &'a RoutingInput<'_>,
) -> Result<BTreeMap<&'a crate::StableId, &'a ProbeDefinition>, RoutingError> {
    let mut definitions = BTreeMap::new();
    for definition in input.definitions {
        if definition.case_id != input.case.case_id
            || definitions
                .insert(&definition.probe_id, definition)
                .is_some()
        {
            return Err(RoutingError::InvalidProbeResult);
        }
    }
    Ok(definitions)
}

fn unique_results<'a>(
    input: &'a RoutingInput<'_>,
    definitions: &BTreeMap<&crate::StableId, &ProbeDefinition>,
) -> Result<BTreeMap<&'a crate::StableId, &'a ProbeResult>, RoutingError> {
    let mut results = BTreeMap::new();
    for result in input.results {
        let Some(definition) = definitions.get(&result.probe_id) else {
            return Err(RoutingError::InvalidProbeResult);
        };
        if result.case_id != input.case.case_id
            || result.validate(definition, input.packet).is_err()
            || results.insert(&result.probe_id, result).is_some()
        {
            return Err(RoutingError::InvalidProbeResult);
        }
    }
    Ok(results)
}

fn validate_predicate_evidence(input: &RoutingInput<'_>) -> Result<(), RoutingError> {
    let allowed = input
        .case
        .route_policy
        .ensemble_required_when
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let packet_ids = input
        .packet
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let invalid_predicate = input.facts.predicate_evidence.iter().any(|evidence| {
        !allowed.contains(&evidence.predicate)
            || evidence.evidence_ids.is_empty()
            || evidence
                .evidence_ids
                .iter()
                .any(|id| !packet_ids.contains(id.as_str()))
    });
    let combined_medium = [
        input.facts.consequence,
        input.facts.uncertainty,
        input.facts.scope,
    ]
    .iter()
    .all(|magnitude| *magnitude >= DecisionMagnitude::Medium);
    let invalid_magnitude_evidence = combined_medium
        && (input.facts.magnitude_evidence_ids.is_empty()
            || input
                .facts
                .magnitude_evidence_ids
                .iter()
                .any(|id| !packet_ids.contains(id.as_str())));
    if invalid_predicate || invalid_magnitude_evidence {
        return Err(RoutingError::InvalidPredicateEvidence);
    }
    Ok(())
}

fn validate_disposition(
    input: &RoutingInput<'_>,
    mandatory: &BTreeSet<&crate::StableId>,
    results: &BTreeMap<&crate::StableId, &ProbeResult>,
) -> Result<(), RoutingError> {
    let mandatory_results = mandatory
        .iter()
        .filter_map(|probe_id| results.get(*probe_id).copied())
        .collect::<Vec<_>>();
    let consistent = match input.facts.deterministic_disposition {
        DeterministicDisposition::NotSubstantiated => mandatory_results
            .iter()
            .any(|result| matches!(result.answer, ProbeAnswer::No | ProbeAnswer::NotApplicable)),
        DeterministicDisposition::EvidenceUnavailable => {
            input.packet.closure.state != ClosureState::Closed
                || mandatory_results
                    .iter()
                    .any(|result| result.answer == ProbeAnswer::Unknown)
        }
        DeterministicDisposition::ActionFullyDetermined => mandatory_results
            .iter()
            .all(|result| result.answer.is_definitive()),
        DeterministicDisposition::Unresolved => true,
    };
    consistent
        .then_some(())
        .ok_or(RoutingError::InconsistentDisposition)
}

fn select_mode(input: &RoutingInput<'_>) -> Result<ModeSelection, RoutingError> {
    let ensemble_reasons = ensemble_reasons(input);
    if !ensemble_reasons.is_empty() {
        return Ok(ModeSelection {
            mode: DeliberationMode::Ensemble,
            reasons: ensemble_reasons.into_iter().collect(),
        });
    }
    if let Some(selection) = deterministic_selection(input) {
        return Ok(selection);
    }
    single_selection(input)
}

fn ensemble_reasons(input: &RoutingInput<'_>) -> BTreeSet<ModeReason> {
    let mut reasons = input
        .facts
        .predicate_evidence
        .iter()
        .map(|evidence| predicate_reason(evidence.predicate))
        .collect::<BTreeSet<_>>();
    let has_conflict = input
        .results
        .iter()
        .any(|result| result.answer == ProbeAnswer::ConflictingEvidence);
    if has_conflict
        && input
            .case
            .route_policy
            .ensemble_required_when
            .contains(&RoutePredicate::MaterialEvidenceConflict)
    {
        reasons.insert(ModeReason::MaterialEvidenceConflict);
    }
    if input.facts.credible_action_families > 1
        && input
            .case
            .route_policy
            .ensemble_required_when
            .contains(&RoutePredicate::MultipleCredibleActions)
    {
        reasons.insert(ModeReason::MultipleCredibleActions);
    }
    if [
        input.facts.consequence,
        input.facts.uncertainty,
        input.facts.scope,
    ]
    .iter()
    .all(|magnitude| *magnitude >= DecisionMagnitude::Medium)
    {
        reasons.insert(ModeReason::CombinedMediumConsequenceUncertaintyScope);
    }
    reasons
}

fn deterministic_selection(input: &RoutingInput<'_>) -> Option<ModeSelection> {
    let deterministic_reason = match input.facts.deterministic_disposition {
        DeterministicDisposition::NotSubstantiated => Some(ModeReason::NotSubstantiated),
        DeterministicDisposition::EvidenceUnavailable => Some(ModeReason::EvidenceUnavailable),
        DeterministicDisposition::ActionFullyDetermined => Some(ModeReason::ActionFullyDetermined),
        DeterministicDisposition::Unresolved => None,
    };
    let has_model_result = input
        .results
        .iter()
        .any(|result| result.resolver.kind == ResolverKind::ModelInterpretation);
    deterministic_reason
        .filter(|_| {
            input.case.route_policy.deterministic_allowed
                && !has_model_result
                && !input.facts.higher_order_inference_required
        })
        .map(|reason| ModeSelection {
            mode: DeliberationMode::Deterministic,
            reasons: vec![reason],
        })
}

fn single_selection(input: &RoutingInput<'_>) -> Result<ModeSelection, RoutingError> {
    if input.case.route_policy.single_reviewer_allowed
        && input.facts.local_and_reversible
        && input.facts.credible_action_families <= 1
    {
        return Ok(ModeSelection {
            mode: DeliberationMode::SingleReviewer,
            reasons: vec![ModeReason::NarrowUnresolvedJudgment],
        });
    }
    if !input.case.route_policy.single_reviewer_allowed {
        return Err(RoutingError::ModeForbidden);
    }
    Err(RoutingError::IncompleteRoutingFacts)
}

const PREDICATE_REASONS: [(RoutePredicate, ModeReason); 9] = [
    (
        RoutePredicate::ResourceLifetimeChange,
        ModeReason::ResourceLifetimeChange,
    ),
    (
        RoutePredicate::ComponentOwnershipChange,
        ModeReason::ComponentOwnershipChange,
    ),
    (
        RoutePredicate::PublicContractChange,
        ModeReason::PublicContractChange,
    ),
    (
        RoutePredicate::CriticalSemanticProbe,
        ModeReason::CriticalSemanticProbe,
    ),
    (RoutePredicate::ThreeLayerSpan, ModeReason::ThreeLayerSpan),
    (
        RoutePredicate::MultipleComponents,
        ModeReason::MultipleComponents,
    ),
    (
        RoutePredicate::MultipleCredibleActions,
        ModeReason::MultipleCredibleActions,
    ),
    (
        RoutePredicate::MaterialEvidenceConflict,
        ModeReason::MaterialEvidenceConflict,
    ),
    (
        RoutePredicate::SignificantBenefitClaim,
        ModeReason::SignificantBenefitClaim,
    ),
];

fn predicate_reason(predicate: RoutePredicate) -> ModeReason {
    PREDICATE_REASONS
        .iter()
        .find(|(candidate, _)| *candidate == predicate)
        .map(|(_, reason)| *reason)
        .expect("every route predicate has one auditable mode reason")
}

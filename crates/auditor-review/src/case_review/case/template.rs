use serde::{Deserialize, Serialize};

use super::{CaseInstantiationError, EngineeringCase};
use crate::{CaseKind, DecisionKind, ReviewCandidate, ReviewInput, SelectionReason};

pub const CASE_TEMPLATE_SCHEMA_VERSION: u32 = 1;
pub const CASE_TEMPLATE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    Existence,
    Identity,
    SemanticEquivalence,
    Ordering,
    Mutation,
    Cost,
    Cardinality,
    Lifecycle,
    ConsumerRequirement,
    Constraint,
    Counterexample,
    AlternativeFeasibility,
    VerificationAvailability,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Finding,
    EvidenceGraph,
    RepositoryMap,
    SourceRole,
    Capability,
    SourceSpan,
    RuntimeMeasurement,
    OperatorConstraint,
    Mutation,
    ConsumerRequirement,
    RetentionConstraint,
    Test,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimSlot {
    Repetition,
    InputEquivalence,
    MutationBoundary,
    MaterialImpact,
    ConsumerRequirement,
    WiderOwner,
    InvalidationFeasibility,
    RetentionRisk,
    LifecycleConclusion,
    Responsibility,
    CausalOverlap,
    IndependentActionability,
    CouplingRisk,
    RemediationBoundary,
    Compatibility,
    AbstractionLevel,
    Supersession,
    PropertyConflict,
    RequiredOrdering,
    Proportionality,
    Deferral,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum CaseConclusion {
    LifecycleMismatchSubstantiated,
    RepetitionJustified,
    ImpactImmaterial,
    WiderOwnershipInfeasible,
    InsufficientEvidence,
    SharedRootCause,
    PartialOverlap,
    IndependentFindings,
    CorrelationOnly,
    ConsolidatedAction,
    SequencedActions,
    PreservedIndependentActions,
    NarrowedAction,
    DeferredAction,
    NoRepositorySpecificAction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationFamily {
    LifecycleReownership,
    ExplicitRetentionAvoidance,
    MeasurementBeforeChange,
    SharedRootRemediation,
    PreserveIndependentActions,
    ConsolidatedRemediation,
    SequencedRemediation,
    NarrowedRemediation,
    DeferredRemediation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceGuardKind {
    RelevantMutation,
    DistinctConfiguration,
    TestOnlyRepetition,
    FreshnessConstraint,
    RetentionRisk,
    QuantitativeBenefit,
    CorrelationNotCausation,
    CouplingIncrease,
    DistinctSemantics,
    IndependentActionability,
    ActionCompatibility,
    AbstractionMismatch,
    PropertyRegression,
    DisproportionateChange,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutePredicate {
    ResourceLifetimeChange,
    ComponentOwnershipChange,
    PublicContractChange,
    CriticalSemanticProbe,
    ThreeLayerSpan,
    MultipleComponents,
    MultipleCredibleActions,
    MaterialEvidenceConflict,
    SignificantBenefitClaim,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeTrigger {
    LifecycleReownership,
    SharedRootRemediation,
    ConsolidatedRemediation,
    SequencedRemediation,
    NarrowedRemediation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerPolicy {
    pub decision_kind: DecisionKind,
    pub selection_reasons_any: Vec<SelectionReason>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectPolicy {
    pub minimum_subjects: usize,
    pub require_connected: bool,
    pub allow_multiple_findings: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub id: String,
    pub evidence_class: EvidenceClass,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceContractTemplate {
    pub required: Vec<EvidenceRequirement>,
    pub optional: Vec<EvidenceRequirement>,
    pub blocking: Vec<EvidenceRequirement>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MandatoryProbeTemplate {
    pub slot: String,
    pub kind: ProbeKind,
    pub question: String,
    pub required_evidence_classes: Vec<EvidenceClass>,
    pub positive_boundary: String,
    pub negative_boundary: String,
    pub blocked_claim_slots: Vec<ClaimSlot>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalProbePolicy {
    pub allowed_kinds: Vec<ProbeKind>,
    pub maximum_count: usize,
    pub require_named_decision: bool,
    pub require_case_boundary: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceGuard {
    pub kind: InferenceGuardKind,
    pub probe_slots: Vec<String>,
    pub blocked_claim_slots: Vec<ClaimSlot>,
    pub blocked_conclusions: Vec<CaseConclusion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePolicy {
    pub deterministic_allowed: bool,
    pub single_reviewer_allowed: bool,
    pub ensemble_required_when: Vec<RoutePredicate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengePolicy {
    pub mandatory_when: Vec<ChallengeTrigger>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClosurePolicy {
    pub allowed_conclusions: Vec<CaseConclusion>,
    pub insufficient_evidence_conclusion: CaseConclusion,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseTemplateDefinition {
    pub schema_version: u32,
    pub kind: CaseKind,
    pub version: u32,
    pub trigger: TriggerPolicy,
    pub subject_policy: SubjectPolicy,
    pub evidence_contract: EvidenceContractTemplate,
    pub mandatory_probes: Vec<MandatoryProbeTemplate>,
    pub optional_probe_policy: OptionalProbePolicy,
    pub claim_slots: Vec<ClaimSlot>,
    pub inference_guards: Vec<InferenceGuard>,
    pub recommendation_families: Vec<RecommendationFamily>,
    pub route_policy: RoutePolicy,
    pub challenge_policy: ChallengePolicy,
    pub closure_policy: ClosurePolicy,
}

impl CaseTemplateDefinition {
    pub fn validate(&self) -> Result<(), String> {
        super::validate::validate_template(self)
    }

    pub fn allows_optional_probe(&self, kind: ProbeKind) -> bool {
        self.optional_probe_policy.allowed_kinds.contains(&kind)
    }
}

pub trait CaseTemplate {
    fn kind(&self) -> CaseKind;
    fn version(&self) -> u32;
    fn instantiate(
        &self,
        candidate: &ReviewCandidate,
        input: &ReviewInput,
    ) -> Result<EngineeringCase, CaseInstantiationError>;
}

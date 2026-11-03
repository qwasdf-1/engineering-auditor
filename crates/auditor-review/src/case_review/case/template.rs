use serde::{Deserialize, Serialize};

use super::{CaseInstantiationError, EngineeringCase};
use crate::{CaseKind, DecisionKind, ReviewCandidate, ReviewInput, SelectionReason};

pub const CASE_TEMPLATE_SCHEMA_VERSION: u32 = 2;
pub const CASE_TEMPLATE_VERSION: u32 = 1;

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
pub struct CaseTemplateDefinition {
    pub schema_version: u32,
    pub kind: CaseKind,
    pub version: u32,
    pub trigger: TriggerPolicy,
    pub subject_policy: SubjectPolicy,
    pub evidence_contract: EvidenceContractTemplate,
    pub claim_slots: Vec<ClaimSlot>,
    pub recommendation_families: Vec<RecommendationFamily>,
}

impl CaseTemplateDefinition {
    pub fn validate(&self) -> Result<(), String> {
        super::validate::validate_template(self)
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

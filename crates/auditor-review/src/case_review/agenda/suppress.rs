use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ReviewCandidate;
use crate::{ReviewInput, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningPriority {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
// Declaration order is the canonical suppression precedence, strongest first.
pub enum SuppressionReason {
    UnsupportedEvidenceNeighborhood,
    DuplicateOfCase,
    OutsideOperatorScope,
    PrivacyPolicyExcludesRequiredContext,
    DeterministicActionAlreadyComplete,
    NoAdditionalContextCanChangeAction,
    InsufficientMateriality,
    ReviewBudgetDeferred,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PolicySuppressionReason {
    UnsupportedEvidenceNeighborhood,
    OutsideOperatorScope,
    PrivacyPolicyExcludesRequiredContext,
    DeterministicActionAlreadyComplete,
    NoAdditionalContextCanChangeAction,
    InsufficientMateriality,
}

impl From<PolicySuppressionReason> for SuppressionReason {
    fn from(value: PolicySuppressionReason) -> Self {
        match value {
            PolicySuppressionReason::UnsupportedEvidenceNeighborhood => {
                Self::UnsupportedEvidenceNeighborhood
            }
            PolicySuppressionReason::OutsideOperatorScope => Self::OutsideOperatorScope,
            PolicySuppressionReason::PrivacyPolicyExcludesRequiredContext => {
                Self::PrivacyPolicyExcludesRequiredContext
            }
            PolicySuppressionReason::DeterministicActionAlreadyComplete => {
                Self::DeterministicActionAlreadyComplete
            }
            PolicySuppressionReason::NoAdditionalContextCanChangeAction => {
                Self::NoAdditionalContextCanChangeAction
            }
            PolicySuppressionReason::InsufficientMateriality => Self::InsufficientMateriality,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuppressionSignal {
    pub reason: PolicySuppressionReason,
    pub supporting_evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgendaPolicyDecision {
    pub priority: PlanningPriority,
    pub suppression_signals: Vec<SuppressionSignal>,
}

impl Default for AgendaPolicyDecision {
    fn default() -> Self {
        Self {
            priority: PlanningPriority::Normal,
            suppression_signals: Vec::new(),
        }
    }
}

pub trait AgendaPolicy {
    fn evaluate(
        &self,
        candidate: &ReviewCandidate,
        input: &ReviewInput,
    ) -> Result<AgendaPolicyDecision, String>;

    fn max_planned_cases(&self) -> Option<usize> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FoundationAgendaPolicy;

impl AgendaPolicy for FoundationAgendaPolicy {
    fn evaluate(
        &self,
        _candidate: &ReviewCandidate,
        _input: &ReviewInput,
    ) -> Result<AgendaPolicyDecision, String> {
        Ok(AgendaPolicyDecision::default())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuppressionRecord {
    pub suppression_id: StableId,
    pub candidate_id: StableId,
    pub reason: SuppressionReason,
    pub supporting_evidence_refs: Vec<String>,
    pub duplicate_of_candidate_id: Option<StableId>,
}

impl SuppressionRecord {
    pub(super) fn new(
        candidate_id: StableId,
        reason: SuppressionReason,
        mut supporting_evidence_refs: Vec<String>,
        duplicate_of_candidate_id: Option<StableId>,
    ) -> Self {
        supporting_evidence_refs.sort();
        supporting_evidence_refs.dedup();
        let identity = json!({
            "candidate_id": candidate_id,
            "duplicate_of_candidate_id": duplicate_of_candidate_id,
            "reason": reason,
            "supporting_evidence_refs": supporting_evidence_refs,
        });
        let suppression_id = StableId::from_identity("review-suppression", 1, &identity)
            .expect("review-suppression is a valid typed prefix");
        Self {
            suppression_id,
            candidate_id,
            reason,
            supporting_evidence_refs,
            duplicate_of_candidate_id,
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if !self
            .suppression_id
            .as_str()
            .starts_with("review-suppression:")
        {
            return Err("suppression id has the wrong typed prefix".to_string());
        }
        if self.supporting_evidence_refs.iter().any(String::is_empty)
            || self
                .supporting_evidence_refs
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("suppression evidence refs must be sorted and unique".to_string());
        }
        if (self.reason == SuppressionReason::DuplicateOfCase)
            != self.duplicate_of_candidate_id.is_some()
        {
            return Err(
                "only duplicate suppression may carry a replacement candidate id".to_string(),
            );
        }
        let expected = Self::new(
            self.candidate_id.clone(),
            self.reason,
            self.supporting_evidence_refs.clone(),
            self.duplicate_of_candidate_id.clone(),
        );
        if self.suppression_id != expected.suppression_id {
            return Err("suppression stable id mismatch".to_string());
        }
        Ok(())
    }
}

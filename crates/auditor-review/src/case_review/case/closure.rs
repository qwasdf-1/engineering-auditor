use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::StableId;

pub const CASE_CLOSURE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseFailure {
    ModelRouteUnavailable,
    ModelOutputInvalid,
    ProviderBudgetExhausted,
    LookupBudgetExhausted,
    EvidencePacketInvalid,
    ChallengeUnresolved,
    OperatorInputRequired,
    PrivacyPolicyBlocked,
    ModeUnavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalCaseState {
    Resolved,
    ResolvedWithUncertainty,
    NotSubstantiated,
    BlockedByMissingEvidence,
    Contested,
    NotApplicable,
    ExecutionFailed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseClosureReason {
    Resolved,
    ResolvedWithUncertainty,
    NotSubstantiated,
    BlockedByMissingEvidence,
    Contested,
    NotApplicable,
    ExecutionFailed,
    BlockedByModeUnavailable,
}

impl CaseClosureReason {
    pub const fn final_state(self) -> FinalCaseState {
        match self {
            Self::Resolved => FinalCaseState::Resolved,
            Self::ResolvedWithUncertainty => FinalCaseState::ResolvedWithUncertainty,
            Self::NotSubstantiated => FinalCaseState::NotSubstantiated,
            Self::BlockedByMissingEvidence => FinalCaseState::BlockedByMissingEvidence,
            Self::Contested => FinalCaseState::Contested,
            Self::NotApplicable => FinalCaseState::NotApplicable,
            Self::ExecutionFailed | Self::BlockedByModeUnavailable => {
                FinalCaseState::ExecutionFailed
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseClosure {
    pub schema_version: u32,
    pub closure_id: StableId,
    pub case_id: StableId,
    pub final_state: FinalCaseState,
    pub reason: CaseClosureReason,
    pub failure: Option<CaseFailure>,
    pub detail_codes: Vec<String>,
}

impl CaseClosure {
    pub fn terminal(
        case_id: StableId,
        reason: CaseClosureReason,
        detail_codes: Vec<String>,
    ) -> Result<Self, CaseClosureError> {
        if matches!(
            reason,
            CaseClosureReason::ExecutionFailed | CaseClosureReason::BlockedByModeUnavailable
        ) {
            return Err(CaseClosureError::InvalidClosure);
        }
        Ok(Self::build(case_id, reason, None, detail_codes))
    }

    pub fn failed(case_id: StableId, failure: CaseFailure, detail_codes: Vec<String>) -> Self {
        let reason = if failure == CaseFailure::ModeUnavailable {
            CaseClosureReason::BlockedByModeUnavailable
        } else {
            CaseClosureReason::ExecutionFailed
        };
        Self::build(case_id, reason, Some(failure), detail_codes)
    }

    pub fn validate(&self) -> Result<(), CaseClosureError> {
        let expected_failure = match self.reason {
            CaseClosureReason::ExecutionFailed => self.failure.is_some(),
            CaseClosureReason::BlockedByModeUnavailable => {
                self.failure == Some(CaseFailure::ModeUnavailable)
            }
            _ => self.failure.is_none(),
        };
        let normalized_details = normalized_detail_codes(self.detail_codes.clone());
        let expected_id = closure_id(
            &self.case_id,
            self.reason,
            self.failure,
            &normalized_details,
        );
        if self.schema_version != CASE_CLOSURE_SCHEMA_VERSION
            || self.final_state != self.reason.final_state()
            || !expected_failure
            || self.detail_codes != normalized_details
            || self.closure_id != expected_id
        {
            return Err(CaseClosureError::InvalidClosure);
        }
        Ok(())
    }

    fn build(
        case_id: StableId,
        reason: CaseClosureReason,
        failure: Option<CaseFailure>,
        detail_codes: Vec<String>,
    ) -> Self {
        let detail_codes = normalized_detail_codes(detail_codes);
        Self {
            schema_version: CASE_CLOSURE_SCHEMA_VERSION,
            closure_id: closure_id(&case_id, reason, failure, &detail_codes),
            case_id,
            final_state: reason.final_state(),
            reason,
            failure,
            detail_codes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseClosureError {
    InvalidClosure,
}

impl fmt::Display for CaseClosureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("case closure identity, state, failure, or details are invalid")
    }
}

impl std::error::Error for CaseClosureError {}

fn normalized_detail_codes(mut details: Vec<String>) -> Vec<String> {
    details.retain(|detail| !detail.trim().is_empty());
    details.sort();
    details.dedup();
    details
}

fn closure_id(
    case_id: &StableId,
    reason: CaseClosureReason,
    failure: Option<CaseFailure>,
    detail_codes: &[String],
) -> StableId {
    StableId::from_identity(
        "case-closure",
        CASE_CLOSURE_SCHEMA_VERSION,
        &json!({
            "case_id": case_id,
            "detail_codes": detail_codes,
            "failure": failure,
            "reason": reason,
        }),
    )
    .expect("case-closure is a valid typed prefix")
}

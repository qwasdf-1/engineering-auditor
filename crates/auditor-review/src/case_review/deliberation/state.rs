use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{CaseClosure, CaseClosureError, DeliberationMode, FinalCaseState, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseExecutionState {
    Planned,
    EvidenceCompiling,
    ProbesResolving,
    Reviewing,
    Reconsidering,
    Comparing,
    Challenging,
    Responding,
    Finalizing,
    Closed,
}

impl CaseExecutionState {
    const ORDER: [Self; 10] = [
        Self::Planned,
        Self::EvidenceCompiling,
        Self::ProbesResolving,
        Self::Reviewing,
        Self::Reconsidering,
        Self::Comparing,
        Self::Challenging,
        Self::Responding,
        Self::Finalizing,
        Self::Closed,
    ];

    fn position(self) -> usize {
        Self::ORDER
            .iter()
            .position(|state| *state == self)
            .expect("all execution states are ordered")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageSkipReason {
    DeterministicMode,
    NotApplicableToMode,
    NoMaterialRecommendation,
    FailureShortCircuit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkippedStage {
    pub state: CaseExecutionState,
    pub reason: StageSkipReason,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseStateTransition {
    pub sequence: usize,
    pub from: Option<CaseExecutionState>,
    pub to: CaseExecutionState,
    pub skipped_stages: Vec<SkippedStage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseStateLedger {
    case_id: StableId,
    mode: DeliberationMode,
    transitions: Vec<CaseStateTransition>,
    closure: Option<CaseClosure>,
}

impl CaseStateLedger {
    pub fn new(case_id: StableId, mode: DeliberationMode) -> Self {
        Self {
            case_id,
            mode,
            transitions: vec![CaseStateTransition {
                sequence: 0,
                from: None,
                to: CaseExecutionState::Planned,
                skipped_stages: Vec::new(),
            }],
            closure: None,
        }
    }

    pub fn current(&self) -> CaseExecutionState {
        self.transitions
            .last()
            .map_or(CaseExecutionState::Planned, |transition| transition.to)
    }

    pub fn case_id(&self) -> &StableId {
        &self.case_id
    }

    pub const fn mode(&self) -> DeliberationMode {
        self.mode
    }

    pub fn transitions(&self) -> &[CaseStateTransition] {
        &self.transitions
    }

    pub fn closure(&self) -> Option<&CaseClosure> {
        self.closure.as_ref()
    }

    pub fn advance(
        &mut self,
        to: CaseExecutionState,
        skipped_stages: Vec<SkippedStage>,
    ) -> Result<(), CaseTransitionError> {
        if self.closure.is_some() || self.current() == CaseExecutionState::Closed {
            return Err(CaseTransitionError::AlreadyClosed);
        }
        if to == CaseExecutionState::Closed {
            return Err(CaseTransitionError::ClosureRequired);
        }
        let from = self.current();
        let intermediate = intermediate_states(from, to)?;
        if intermediate.len() != skipped_stages.len()
            || intermediate
                .iter()
                .zip(&skipped_stages)
                .any(|(expected, actual)| {
                    *expected != actual.state
                        || actual.reason == StageSkipReason::FailureShortCircuit
                        || required_for_mode(self.mode, actual.state)
                })
        {
            return Err(CaseTransitionError::InvalidSkippedStages);
        }
        self.push_transition(from, to, skipped_stages);
        Ok(())
    }

    pub fn close(&mut self, closure: CaseClosure) -> Result<(), CaseTransitionError> {
        closure.validate()?;
        if self.closure.is_some() || self.current() == CaseExecutionState::Closed {
            return Err(CaseTransitionError::AlreadyClosed);
        }
        if closure.case_id != self.case_id {
            return Err(CaseTransitionError::CaseBinding);
        }
        if self.current() != CaseExecutionState::Finalizing {
            return Err(CaseTransitionError::FinalizingRequired);
        }
        self.finish(closure);
        Ok(())
    }

    pub fn fail(&mut self, closure: CaseClosure) -> Result<(), CaseTransitionError> {
        closure.validate()?;
        if closure.final_state != FinalCaseState::ExecutionFailed {
            return Err(CaseTransitionError::FailureClosureRequired);
        }
        if closure.case_id != self.case_id {
            return Err(CaseTransitionError::CaseBinding);
        }
        if self.closure.is_some() || self.current() == CaseExecutionState::Closed {
            return Err(CaseTransitionError::AlreadyClosed);
        }
        let current = self.current();
        if current != CaseExecutionState::Finalizing {
            let skipped_stages = intermediate_states(current, CaseExecutionState::Finalizing)?
                .into_iter()
                .map(|state| SkippedStage {
                    state,
                    reason: StageSkipReason::FailureShortCircuit,
                })
                .collect();
            self.push_transition(current, CaseExecutionState::Finalizing, skipped_stages);
        }
        self.finish(closure);
        Ok(())
    }

    fn finish(&mut self, closure: CaseClosure) {
        self.push_transition(
            CaseExecutionState::Finalizing,
            CaseExecutionState::Closed,
            Vec::new(),
        );
        self.closure = Some(closure);
    }

    fn push_transition(
        &mut self,
        from: CaseExecutionState,
        to: CaseExecutionState,
        skipped_stages: Vec<SkippedStage>,
    ) {
        self.transitions.push(CaseStateTransition {
            sequence: self.transitions.len(),
            from: Some(from),
            to,
            skipped_stages,
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseTransitionError {
    AlreadyClosed,
    BackwardOrRepeated,
    InvalidSkippedStages,
    ClosureRequired,
    FinalizingRequired,
    FailureClosureRequired,
    CaseBinding,
    InvalidClosure,
}

impl fmt::Display for CaseTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::AlreadyClosed => "case state is already closed",
            Self::BackwardOrRepeated => "case state transition must move forward",
            Self::InvalidSkippedStages => "every conditional skipped stage must be recorded",
            Self::ClosureRequired => "closed state requires a typed case closure",
            Self::FinalizingRequired => "normal closure requires finalizing state",
            Self::FailureClosureRequired => "failure path requires execution_failed closure",
            Self::CaseBinding => "case closure belongs to another case",
            Self::InvalidClosure => "case closure is invalid",
        };
        formatter.write_str(detail)
    }
}

impl std::error::Error for CaseTransitionError {}

impl From<CaseClosureError> for CaseTransitionError {
    fn from(_: CaseClosureError) -> Self {
        Self::InvalidClosure
    }
}

fn intermediate_states(
    from: CaseExecutionState,
    to: CaseExecutionState,
) -> Result<Vec<CaseExecutionState>, CaseTransitionError> {
    let from = from.position();
    let to = to.position();
    if to <= from {
        return Err(CaseTransitionError::BackwardOrRepeated);
    }
    Ok(CaseExecutionState::ORDER[from + 1..to].to_vec())
}

fn required_for_mode(mode: DeliberationMode, state: CaseExecutionState) -> bool {
    match state {
        CaseExecutionState::EvidenceCompiling
        | CaseExecutionState::ProbesResolving
        | CaseExecutionState::Finalizing => true,
        CaseExecutionState::Reviewing => mode != DeliberationMode::Deterministic,
        CaseExecutionState::Reconsidering | CaseExecutionState::Comparing => {
            mode == DeliberationMode::Ensemble
        }
        CaseExecutionState::Planned
        | CaseExecutionState::Challenging
        | CaseExecutionState::Responding
        | CaseExecutionState::Closed => false,
    }
}

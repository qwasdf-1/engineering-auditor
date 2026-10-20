mod candidate;
mod index;
mod normalize;
mod planner;
mod planning_ops;
mod report_index;
mod selector;
mod suppress;

pub use candidate::{
    CandidateSelection, CaseKind, DecisionKind, ReviewCandidate, SelectionReason,
    SourceCandidateRef, REVIEW_CANDIDATE_SCHEMA_VERSION,
};
pub use planner::{
    AgendaCoverage, AgendaPlanner, CasePlanningRecord, CasePlanningStatus, ReviewAgenda,
    AGENDA_PLANNER_VERSION, REVIEW_AGENDA_SCHEMA_VERSION,
};
pub use selector::{
    CandidateSelectionError, CandidateSelector, SelectorManifestEntry, SelectorRegistry,
    SELECTOR_VERSION,
};
pub use suppress::{
    AgendaPolicy, AgendaPolicyDecision, FoundationAgendaPolicy, PlanningPriority,
    PolicySuppressionReason, SuppressionReason, SuppressionRecord, SuppressionSignal,
};

pub(crate) fn validate_case_candidate(
    input: &crate::ReviewInput,
    candidate: &ReviewCandidate,
) -> Result<bool, CandidateSelectionError> {
    let index = index::AgendaIndex::from_input(input)?;
    index.validate_candidate(candidate)?;
    Ok(index.connected_components(&candidate.subject_ids).len() == 1)
}

#[cfg(test)]
mod tests;

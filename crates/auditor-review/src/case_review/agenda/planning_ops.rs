use std::collections::{BTreeMap, BTreeSet};

use super::index::AgendaIndex;
use super::normalize::DuplicateCandidate;
use super::planner::{AgendaCoverage, CasePlanningRecord, CasePlanningStatus};
use super::selector::{CandidateSelectionError, SelectorRegistry};
use super::suppress::{AgendaPolicy, PlanningPriority, SuppressionReason, SuppressionRecord};
use super::ReviewCandidate;
use crate::{ReviewInput, StableId};

pub(super) struct CandidateDecision {
    candidate_id: StableId,
    priority: PlanningPriority,
    suppression: Option<(SuppressionReason, Vec<String>)>,
}

pub(super) fn select_candidates(
    input: &ReviewInput,
    registry: &SelectorRegistry,
    index: &AgendaIndex,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    let mut candidates = Vec::new();
    for selector in registry.selectors() {
        let selected = selector.select(input)?;
        for candidate in &selected {
            if candidate.selector_id != selector.id()
                || candidate.selector_version != selector.version()
            {
                return Err(CandidateSelectionError::InvalidSelector {
                    selector_id: selector.id().to_string(),
                    detail: "candidate selector identity does not match registry owner".to_string(),
                });
            }
            index.validate_candidate(candidate)?;
        }
        candidates.extend(selected);
    }
    Ok(candidates)
}

pub(super) fn duplicate_suppressions(
    duplicates: Vec<DuplicateCandidate>,
) -> Vec<SuppressionRecord> {
    duplicates
        .into_iter()
        .map(|duplicate| {
            SuppressionRecord::new(
                duplicate.source_candidate_id,
                SuppressionReason::DuplicateOfCase,
                duplicate.evidence_refs,
                Some(duplicate.duplicate_of_candidate_id),
            )
        })
        .collect()
}

pub(super) fn decide_candidates(
    input: &ReviewInput,
    policy: &dyn AgendaPolicy,
    index: &AgendaIndex,
    candidates: &[ReviewCandidate],
) -> Result<Vec<CandidateDecision>, CandidateSelectionError> {
    let excluded_domains = input
        .operator_brief()
        .map(|brief| brief.excluded_domains.iter().map(String::as_str).collect())
        .unwrap_or_default();
    candidates
        .iter()
        .map(|candidate| decide_candidate(input, policy, index, candidate, &excluded_domains))
        .collect()
}

fn decide_candidate(
    input: &ReviewInput,
    policy: &dyn AgendaPolicy,
    index: &AgendaIndex,
    candidate: &ReviewCandidate,
    excluded_domains: &BTreeSet<&str>,
) -> Result<CandidateDecision, CandidateSelectionError> {
    let decision = policy
        .evaluate(candidate, input)
        .map_err(|detail| CandidateSelectionError::Policy { detail })?;
    let mut signals = decision
        .suppression_signals
        .into_iter()
        .map(|signal| {
            validate_policy_evidence(&signal.supporting_evidence_refs, index)?;
            Ok((
                SuppressionReason::from(signal.reason),
                signal.supporting_evidence_refs,
            ))
        })
        .collect::<Result<Vec<_>, CandidateSelectionError>>()?;
    if !has_reviewable_findings(candidate) {
        signals.push((
            SuppressionReason::UnsupportedEvidenceNeighborhood,
            Vec::new(),
        ));
    }
    let domains = index.domains_for_candidate(candidate);
    if !domains.is_empty()
        && domains
            .iter()
            .all(|domain| excluded_domains.contains(domain))
    {
        signals.push((SuppressionReason::OutsideOperatorScope, Vec::new()));
    }
    Ok(CandidateDecision {
        candidate_id: candidate.candidate_id.clone(),
        priority: decision.priority,
        suppression: strongest_signal(signals),
    })
}

/// Every case template binds at least one deterministic finding, so a
/// finding-free candidate (a material capability limitation) is recorded on
/// the agenda but can never be instantiated as a case.
pub(super) fn has_reviewable_findings(candidate: &ReviewCandidate) -> bool {
    !candidate.finding_ids.is_empty()
}

fn validate_policy_evidence(
    evidence_refs: &[String],
    index: &AgendaIndex,
) -> Result<(), CandidateSelectionError> {
    if let Some(reference) = evidence_refs
        .iter()
        .find(|reference| !index.contains_node(reference))
    {
        return Err(CandidateSelectionError::Policy {
            detail: format!("suppression signal contains dangling evidence {reference}"),
        });
    }
    Ok(())
}

pub(super) fn materialize_plans(
    candidates: &[ReviewCandidate],
    decisions: Vec<CandidateDecision>,
    suppression_records: &mut Vec<SuppressionRecord>,
) -> Vec<CasePlanningRecord> {
    let mut indexed_decisions = BTreeMap::new();
    for decision in decisions {
        indexed_decisions.insert(decision.candidate_id.clone(), decision);
    }
    candidates
        .iter()
        .flat_map(|candidate| {
            let suppression_id = materialize_suppression(
                candidate,
                &indexed_decisions[&candidate.candidate_id],
                suppression_records,
            );
            candidate.eligible_case_kinds.iter().map(move |case_kind| {
                CasePlanningRecord::new(
                    candidate.candidate_id.clone(),
                    *case_kind,
                    if suppression_id.is_some() {
                        CasePlanningStatus::Suppressed
                    } else {
                        CasePlanningStatus::Planned
                    },
                    suppression_id.clone(),
                )
            })
        })
        .collect()
}

fn materialize_suppression(
    candidate: &ReviewCandidate,
    decision: &CandidateDecision,
    records: &mut Vec<SuppressionRecord>,
) -> Option<StableId> {
    decision.suppression.as_ref().map(|(reason, evidence)| {
        let record = SuppressionRecord::new(
            candidate.candidate_id.clone(),
            *reason,
            evidence.clone(),
            None,
        );
        let id = record.suppression_id.clone();
        records.push(record);
        id
    })
}

fn strongest_signal(
    signals: Vec<(SuppressionReason, Vec<String>)>,
) -> Option<(SuppressionReason, Vec<String>)> {
    let strongest = signals.iter().map(|(reason, _)| *reason).min()?;
    let mut evidence = signals
        .into_iter()
        .filter(|(reason, _)| *reason == strongest)
        .flat_map(|(_, evidence)| evidence)
        .collect::<Vec<_>>();
    evidence.sort();
    evidence.dedup();
    Some((strongest, evidence))
}

pub(super) fn apply_budget(
    max_planned_cases: Option<usize>,
    candidates: &[ReviewCandidate],
    decisions: &mut [CandidateDecision],
) -> Result<(), CandidateSelectionError> {
    let Some(limit) = max_planned_cases else {
        return Ok(());
    };
    if limit == 0 {
        return Err(CandidateSelectionError::Policy {
            detail: "max planned cases must be positive".to_string(),
        });
    }
    let candidate_by_id = candidates
        .iter()
        .map(|candidate| (&candidate.candidate_id, candidate))
        .collect::<BTreeMap<_, _>>();
    let mut order = decisions
        .iter()
        .enumerate()
        .filter(|(_, decision)| decision.suppression.is_none())
        .map(|(index, decision)| (decision.priority, decision.candidate_id.clone(), index))
        .collect::<Vec<_>>();
    order.sort();
    let mut planned = 0usize;
    for (_, candidate_id, index) in order {
        let required = candidate_by_id[&candidate_id].eligible_case_kinds.len();
        if planned.saturating_add(required) > limit {
            decisions[index].suppression =
                Some((SuppressionReason::ReviewBudgetDeferred, Vec::new()));
        } else {
            planned += required;
        }
    }
    Ok(())
}

pub(super) fn coverage(
    candidates: &[ReviewCandidate],
    suppressions: &[SuppressionRecord],
    plans: &[CasePlanningRecord],
    index: &AgendaIndex,
) -> AgendaCoverage {
    let source_candidate_count = candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .source_candidates
                .iter()
                .map(|source| &source.candidate_id)
        })
        .collect::<BTreeSet<_>>()
        .len();
    let referenced = candidates
        .iter()
        .flat_map(|candidate| candidate.finding_ids.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    AgendaCoverage {
        source_candidate_count,
        candidate_count: candidates.len(),
        suppression_count: suppressions.len(),
        planned_case_count: plans
            .iter()
            .filter(|record| record.status == CasePlanningStatus::Planned)
            .count(),
        referenced_finding_count: referenced.len(),
        not_selected_finding_count: index.all_finding_ids().difference(&referenced).count(),
    }
}

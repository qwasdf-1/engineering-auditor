use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::candidate::{
    CaseKind, DecisionKind, NormalizedCandidateFields, ReviewCandidate, SelectionReason,
    SourceCandidateRef,
};
use super::index::AgendaIndex;
use super::CandidateSelectionError;
use crate::StableId;

#[derive(Clone, Debug)]
pub(super) struct DuplicateCandidate {
    pub source_candidate_id: StableId,
    pub duplicate_of_candidate_id: StableId,
    pub evidence_refs: Vec<String>,
}

pub(super) struct NormalizationResult {
    pub candidates: Vec<ReviewCandidate>,
    pub duplicates: Vec<DuplicateCandidate>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MergeKey {
    subjects: Vec<String>,
    decision: DecisionKind,
    cases: Vec<CaseKind>,
}

struct MergedCandidate {
    decision_kind: DecisionKind,
    subject_ids: Vec<String>,
    finding_ids: Vec<String>,
    evidence_refs: Vec<String>,
    case_kinds: Vec<CaseKind>,
    reasons: Vec<SelectionReason>,
    sources: Vec<SourceCandidateRef>,
}

pub(super) fn normalize_candidates(
    mut candidates: Vec<ReviewCandidate>,
    index: &AgendaIndex,
) -> Result<NormalizationResult, CandidateSelectionError> {
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    let groups = group_candidates(candidates, index)?;
    let mut normalized = Vec::new();
    let mut duplicates = Vec::new();
    for (_, group) in groups {
        let (merged, duplicate_sources) = merge_group(group);
        let mut parts = split_group(index, merged)?;
        parts.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
        let winner = parts
            .first()
            .map(|candidate| candidate.candidate_id.clone())
            .expect("normalization group produces at least one candidate");
        duplicates.extend(duplicate_sources.into_iter().map(
            |(source_candidate_id, evidence_refs)| DuplicateCandidate {
                source_candidate_id,
                duplicate_of_candidate_id: winner.clone(),
                evidence_refs,
            },
        ));
        normalized.extend(parts);
    }
    normalized.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    normalized.dedup_by(|left, right| left.candidate_id == right.candidate_id);
    duplicates.sort_by(|left, right| {
        (&left.source_candidate_id, &left.duplicate_of_candidate_id)
            .cmp(&(&right.source_candidate_id, &right.duplicate_of_candidate_id))
    });
    duplicates.dedup_by(|left, right| {
        left.source_candidate_id == right.source_candidate_id
            && left.duplicate_of_candidate_id == right.duplicate_of_candidate_id
    });
    Ok(NormalizationResult {
        candidates: normalized,
        duplicates,
    })
}

fn group_candidates(
    candidates: Vec<ReviewCandidate>,
    index: &AgendaIndex,
) -> Result<BTreeMap<MergeKey, Vec<ReviewCandidate>>, CandidateSelectionError> {
    let mut groups = BTreeMap::new();
    for candidate in candidates {
        index.validate_candidate(&candidate)?;
        groups
            .entry(MergeKey {
                subjects: candidate.subject_ids.clone(),
                decision: candidate.decision_kind,
                cases: candidate.eligible_case_kinds.clone(),
            })
            .or_insert_with(Vec::new)
            .push(candidate);
    }
    Ok(groups)
}

fn merge_group(group: Vec<ReviewCandidate>) -> (MergedCandidate, Vec<(StableId, Vec<String>)>) {
    let first = &group[0];
    let mut merged = MergedCandidate {
        decision_kind: first.decision_kind,
        subject_ids: first.subject_ids.clone(),
        finding_ids: Vec::new(),
        evidence_refs: Vec::new(),
        case_kinds: first.eligible_case_kinds.clone(),
        reasons: Vec::new(),
        sources: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();
    for candidate in group {
        if !seen.insert(candidate_fingerprint(&candidate)) {
            duplicates.extend(candidate.source_candidates.iter().map(|source| {
                (
                    source.candidate_id.clone(),
                    candidate.selection_evidence_refs.clone(),
                )
            }));
        }
        merged.finding_ids.extend(candidate.finding_ids);
        merged
            .evidence_refs
            .extend(candidate.selection_evidence_refs);
        merged.reasons.extend(candidate.selection_reasons);
        merged.sources.extend(candidate.source_candidates);
    }
    sort_dedup(&mut merged.finding_ids);
    sort_dedup(&mut merged.evidence_refs);
    sort_dedup(&mut merged.reasons);
    sort_dedup(&mut merged.sources);
    (merged, duplicates)
}

fn candidate_fingerprint(candidate: &ReviewCandidate) -> String {
    serde_json::to_string(&json!({
        "decision_kind": candidate.decision_kind,
        "eligible_case_kinds": candidate.eligible_case_kinds,
        "finding_ids": candidate.finding_ids,
        "selection_evidence_refs": candidate.selection_evidence_refs,
        "selection_reasons": candidate.selection_reasons,
        "subject_ids": candidate.subject_ids,
    }))
    .expect("candidate fingerprint is serializable")
}

fn split_group(
    index: &AgendaIndex,
    merged: MergedCandidate,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    let selector_id = merged
        .sources
        .first()
        .map(|source| source.selector_id.clone())
        .unwrap_or_else(|| "<missing>".to_string());
    index
        .connected_components(&merged.subject_ids)
        .into_iter()
        .map(|component| {
            let component_set = component
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let component_findings = merged
                .finding_ids
                .iter()
                .filter(|finding_id| {
                    index
                        .finding_subject(finding_id)
                        .is_some_and(|subject| component_set.contains(subject))
                })
                .cloned()
                .collect::<Vec<_>>();
            let mut component_evidence = index.evidence_for_findings(component_findings.iter());
            component_evidence.extend(
                merged
                    .evidence_refs
                    .iter()
                    .filter(|reference| component_set.contains(reference.as_str()))
                    .cloned(),
            );
            sort_dedup(&mut component_evidence);
            if component_evidence.is_empty() {
                component_evidence = merged.evidence_refs.clone();
            }
            ReviewCandidate::from_normalized(NormalizedCandidateFields {
                decision_kind: merged.decision_kind,
                subject_ids: component,
                finding_ids: component_findings,
                selection_evidence_refs: component_evidence,
                eligible_case_kinds: merged.case_kinds.clone(),
                selection_reasons: merged.reasons.clone(),
                source_candidates: merged.sources.clone(),
            })
            .map_err(|detail| CandidateSelectionError::InvalidCandidate {
                selector_id: selector_id.clone(),
                detail,
            })
        })
        .collect()
}

fn sort_dedup<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

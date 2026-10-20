use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{EnsembleReviewerSlot, ProbeAnswer, ProbeResult, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeComparisonState {
    Unanimous,
    QualifiedAgreement,
    Split,
    AllDifferent,
    SharedUnknown,
    EvidenceConflict,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeComparison {
    pub probe_id: StableId,
    pub state: ProbeComparisonState,
    pub answers: BTreeMap<EnsembleReviewerSlot, ProbeAnswer>,
    pub evidence_ids: BTreeMap<EnsembleReviewerSlot, Vec<String>>,
}

pub fn compare_probe_results(
    probe_id: StableId,
    results: &[(EnsembleReviewerSlot, &ProbeResult)],
) -> Result<ProbeComparison, String> {
    if results.len() != EnsembleReviewerSlot::ALL.len() {
        return Err("probe comparison requires all three reviewer slots".to_string());
    }
    let mut answers = BTreeMap::new();
    let mut evidence_ids = BTreeMap::new();
    for (slot, result) in results {
        if result.probe_id != probe_id || answers.insert(*slot, result.answer).is_some() {
            return Err("probe comparison contains a mismatched or duplicate slot".to_string());
        }
        let mut evidence = result.evidence_ids.clone();
        evidence.sort();
        evidence.dedup();
        evidence_ids.insert(*slot, evidence);
    }
    if answers.len() != EnsembleReviewerSlot::ALL.len() {
        return Err("probe comparison is missing a reviewer slot".to_string());
    }
    let unique_answers = answers.values().copied().collect::<BTreeSet<_>>();
    let unique_evidence = evidence_ids.values().cloned().collect::<BTreeSet<_>>();
    let state = if answers
        .values()
        .all(|answer| *answer == ProbeAnswer::Unknown)
    {
        ProbeComparisonState::SharedUnknown
    } else if answers
        .values()
        .any(|answer| *answer == ProbeAnswer::ConflictingEvidence)
    {
        ProbeComparisonState::EvidenceConflict
    } else if unique_answers.len() == 1 && unique_evidence.len() == 1 {
        ProbeComparisonState::Unanimous
    } else if unique_answers.len() == 1 {
        ProbeComparisonState::QualifiedAgreement
    } else if unique_answers.len() == 2 {
        ProbeComparisonState::Split
    } else {
        ProbeComparisonState::AllDifferent
    };
    Ok(ProbeComparison {
        probe_id,
        state,
        answers,
        evidence_ids,
    })
}

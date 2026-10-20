use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::{
    EgressManifest, FinalizedCase, FindingDispositionKind, FindingDispositionRecord, ReviewInput,
    StableId,
};

use super::offline::{EngineeringReviewV2, EngineeringReviewV2Draft, ReviewVerificationError};

pub(super) fn verify_finding_dispositions(
    artifact: &EngineeringReviewV2,
    input: &ReviewInput,
) -> Result<(), ReviewVerificationError> {
    let report_ids = input.report()["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| finding["id"].as_str())
        .collect::<BTreeSet<_>>();
    let mut covered = BTreeSet::new();
    for disposition in &artifact.finding_dispositions {
        if !report_ids.contains(disposition.finding_id.as_str())
            || !covered.insert(disposition.finding_id.as_str())
            || (disposition.disposition == FindingDispositionKind::NotSelectedForContextualReview
                && disposition.case_id.is_some())
        {
            return Err(ReviewVerificationError::FindingDisposition);
        }
    }
    if covered != report_ids {
        return Err(ReviewVerificationError::FindingDisposition);
    }
    Ok(())
}

pub(super) fn verify_egress(manifests: &[EgressManifest]) -> Result<(), ReviewVerificationError> {
    if manifests.iter().any(|manifest| {
        manifest.schema_version != 1
            || manifest.block_count != manifest.blocks.len()
            || manifest.byte_count
                != manifest
                    .blocks
                    .iter()
                    .map(|block| block.byte_count)
                    .sum::<usize>()
            || manifest.blocks.iter().any(|block| {
                block.provenance.trim().is_empty()
                    || block.digest.trim().is_empty()
                    || block.byte_count == 0
            })
    }) {
        return Err(ReviewVerificationError::Egress);
    }
    Ok(())
}

pub(super) fn complete_finding_dispositions(
    input: &ReviewInput,
    finalizations: &[FinalizedCase],
) -> Result<Vec<FindingDispositionRecord>, ReviewVerificationError> {
    let selected = finalizations
        .iter()
        .flat_map(|case| &case.finding_dispositions)
        .map(|record| (record.finding_id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut records = Vec::new();
    for finding in input.report()["findings"].as_array().into_iter().flatten() {
        let id = finding["id"]
            .as_str()
            .ok_or(ReviewVerificationError::FindingDisposition)?;
        records.push(selected.get(id).map(|record| (*record).clone()).unwrap_or(
            FindingDispositionRecord {
                finding_id: id.to_string(),
                case_id: None,
                disposition: FindingDispositionKind::NotSelectedForContextualReview,
            },
        ));
    }
    records.sort_by(|left, right| left.finding_id.cmp(&right.finding_id));
    Ok(records)
}

pub(super) fn sort_draft(draft: &mut EngineeringReviewV2Draft) {
    sort_on(&mut draft.cases, |value| &value.case_id);
    sort_on(&mut draft.evidence_packets, |value| &value.case_id);
    draft.evidence_packet_history.sort_by(|left, right| {
        (&left.case_id, &left.packet_digest).cmp(&(&right.case_id, &right.packet_digest))
    });
    sort_on(&mut draft.probe_definitions, |value| &value.probe_id);
    sort_on(&mut draft.probe_results, |value| &value.probe_id);
    sort_on(&mut draft.reviewer_runs, |value| &value.case_id);
    sort_on(&mut draft.ensemble_runs, |value| &value.case_id);
    sort_on(&mut draft.ensemble_failures, |value| &value.case_id);
    sort_on(&mut draft.ensemble_evidence_packets, |value| &value.case_id);
    draft.ensemble_challenges.sort_by(|left, right| {
        (&left.case_id, left.reviewer).cmp(&(&right.case_id, right.reviewer))
    });
    sort_on(&mut draft.claims, |value| &value.claim_id);
    sort_on(&mut draft.recommendations, |value| &value.recommendation_id);
    sort_on(&mut draft.challenges, |value| &value.challenge.case_id);
    sort_on(&mut draft.execution_truth, |value| &value.case_id);
}

fn sort_on<T, K: Ord + ?Sized>(values: &mut [T], key: impl Fn(&T) -> &K) {
    values.sort_by(|left, right| key(left).cmp(key(right)));
}

pub(super) fn semantic_index<'a, T: Serialize + 'a>(
    values: impl IntoIterator<Item = &'a T>,
    id: impl Fn(&T) -> &StableId,
) -> Result<BTreeMap<StableId, Value>, ReviewVerificationError> {
    let mut indexed = BTreeMap::new();
    for value in values {
        let serialized = serde_json::to_value(value)
            .map_err(|error| ReviewVerificationError::Identity(error.to_string()))?;
        match indexed.entry(id(value).clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(serialized);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() != &serialized => {
                return Err(ReviewVerificationError::Reference(
                    "conflicting semantic index entry".to_string(),
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }
    Ok(indexed)
}

pub(super) fn unique_by<'a, T, K: Ord + ?Sized>(
    values: &'a [T],
    key: impl Fn(&'a T) -> &'a K,
    name: &str,
) -> Result<BTreeMap<&'a K, &'a T>, ReviewVerificationError> {
    let mut indexed = BTreeMap::new();
    for value in values {
        if indexed.insert(key(value), value).is_some() {
            return Err(ReviewVerificationError::Reference(format!(
                "duplicate {name}"
            )));
        }
    }
    Ok(indexed)
}

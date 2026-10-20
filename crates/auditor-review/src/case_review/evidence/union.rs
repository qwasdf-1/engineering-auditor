use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{CaseEvidenceRequest, EnsembleReviewerSlot, StableId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerEvidenceRequest {
    pub reviewer: EnsembleReviewerSlot,
    pub request: CaseEvidenceRequest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnionEvidenceRequest {
    pub request: CaseEvidenceRequest,
    pub requested_by: Vec<EnsembleReviewerSlot>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceUnionPlan {
    pub case_id: StableId,
    pub requests: Vec<UnionEvidenceRequest>,
}

pub struct EvidenceUnionBoundary<'a> {
    pub case_id: &'a StableId,
    pub probe_ids: &'a BTreeSet<StableId>,
    pub target_ids: &'a BTreeSet<String>,
    pub maximum_requests: usize,
}

pub fn plan_evidence_union(
    requests: impl IntoIterator<Item = ReviewerEvidenceRequest>,
    boundary: EvidenceUnionBoundary<'_>,
) -> Result<EvidenceUnionPlan, String> {
    let mut union = BTreeMap::<(StableId, String, String), UnionEvidenceRequest>::new();
    for item in requests {
        let request = item.request;
        if &request.case_id != boundary.case_id
            || !boundary.probe_ids.contains(&request.probe_id)
            || !boundary.target_ids.contains(&request.target_id)
            || request.decision_relevance.trim().is_empty()
        {
            return Err(format!(
                "reviewer {:?} requested evidence outside the case boundary",
                item.reviewer
            ));
        }
        let key = (
            request.probe_id.clone(),
            format!("{:?}", request.request_kind),
            request.target_id.clone(),
        );
        union
            .entry(key)
            .and_modify(|entry| entry.requested_by.push(item.reviewer))
            .or_insert(UnionEvidenceRequest {
                request,
                requested_by: vec![item.reviewer],
            });
    }
    if union.len() > boundary.maximum_requests {
        return Err("ensemble evidence union exceeds its request budget".to_string());
    }
    let mut requests = union.into_values().collect::<Vec<_>>();
    for request in &mut requests {
        request.requested_by.sort();
        request.requested_by.dedup();
    }
    Ok(EvidenceUnionPlan {
        case_id: boundary.case_id.clone(),
        requests,
    })
}

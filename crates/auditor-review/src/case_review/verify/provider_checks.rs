use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CaseProviderCallRecord, CaseQualificationRecord, ContentDigest, EngineeringCase,
    EvidencePacket, ReviewRole, RouteTarget, StableId,
};

use super::offline::{EngineeringReviewV2, ReviewVerificationError};

pub(super) fn verify_provider_records(
    artifact: &EngineeringReviewV2,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
    packet_versions: &BTreeMap<&ContentDigest, &EvidencePacket>,
) -> Result<(), ReviewVerificationError> {
    verify_qualification_identities(&artifact.qualification_records)?;
    verify_provider_call_bindings(artifact, cases, packet_versions)?;
    verify_reviewer_bindings(artifact, cases)
}

fn verify_qualification_identities(
    records: &[CaseQualificationRecord],
) -> Result<(), ReviewVerificationError> {
    let mut qualification_ids = BTreeSet::new();
    let valid = records.iter().all(|record| {
        record.has_valid_identity()
            && record.binding.renderer_version == crate::MODEL_MARKDOWN_RENDERER_VERSION
            && record.binding.tool_schema_digest == crate::case_tool_schema_digest()
            && record.binding.adapter_version == crate::PROVIDER_ADAPTER_VERSION
            && qualification_ids.insert(record.qualification_id.clone())
    });
    valid
        .then_some(())
        .ok_or_else(|| provider_error("qualification record identity is invalid or duplicated"))
}

fn verify_provider_call_bindings(
    artifact: &EngineeringReviewV2,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
    packet_versions: &BTreeMap<&ContentDigest, &EvidencePacket>,
) -> Result<(), ReviewVerificationError> {
    for record in &artifact.provider_records {
        verify_provider_call_binding(artifact, cases, packet_versions, record)?;
    }
    Ok(())
}

fn verify_provider_call_binding(
    artifact: &EngineeringReviewV2,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
    packet_versions: &BTreeMap<&ContentDigest, &EvidencePacket>,
    record: &CaseProviderCallRecord,
) -> Result<(), ReviewVerificationError> {
    let failure_valid = record
        .failure_detail
        .as_ref()
        .is_none_or(|detail| !detail.trim().is_empty() && record.failure_class.is_some());
    if !failure_valid {
        return Err(provider_error(
            "provider call failure detail is empty or lacks a failure class",
        ));
    }
    let case = cases
        .get(&record.case_id)
        .ok_or_else(|| provider_error("provider call references an unknown case"))?;
    let packet = packet_versions
        .get(&record.packet_digest)
        .copied()
        .or_else(|| {
            artifact
                .ensemble_evidence_packets
                .iter()
                .find(|packet| packet.packet_digest == record.packet_digest)
        })
        .ok_or_else(|| {
            provider_error("provider call references an unknown evidence packet version")
        })?;
    let target = RouteTarget {
        provider_id: record.call.provider_id.clone(),
        model: record.call.model.clone(),
    };
    let valid = packet.case_id == record.case_id
        && has_qualified_binding(
            &artifact.qualification_records,
            &target,
            &case.case_kind,
            &[record.role],
        );
    valid
        .then_some(())
        .ok_or_else(|| provider_error("provider call lacks a matching qualified role binding"))
}

fn verify_reviewer_bindings(
    artifact: &EngineeringReviewV2,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
) -> Result<(), ReviewVerificationError> {
    for review in &artifact.reviewer_runs {
        let case = cases
            .get(&review.case_id)
            .ok_or_else(|| provider_error("reviewer result references an unknown case"))?;
        let roles = [
            ReviewRole::SingleReviewer,
            ReviewRole::ReviewerA,
            ReviewRole::ReviewerB,
            ReviewRole::ReviewerC,
        ];
        if !has_qualified_binding(
            &artifact.qualification_records,
            &review.route_target,
            &case.case_kind,
            &roles,
        ) {
            return Err(provider_error(
                "reviewer result lacks a matching qualified reviewer binding",
            ));
        }
    }
    for run in &artifact.ensemble_runs {
        for pass in run.pass_a.iter().chain(&run.pass_b) {
            let case = cases
                .get(&pass.review.case_id)
                .ok_or_else(|| provider_error("ensemble review references an unknown case"))?;
            let expected_role = match pass.reviewer {
                crate::EnsembleReviewerSlot::ReviewerA => ReviewRole::ReviewerA,
                crate::EnsembleReviewerSlot::ReviewerB => ReviewRole::ReviewerB,
                crate::EnsembleReviewerSlot::ReviewerC => ReviewRole::ReviewerC,
            };
            if !has_qualified_binding(
                &artifact.qualification_records,
                &pass.review.route_target,
                &case.case_kind,
                &[expected_role],
            ) {
                return Err(provider_error(
                    "ensemble result lacks its exact qualified reviewer-slot binding",
                ));
            }
        }
    }
    Ok(())
}

fn has_qualified_binding(
    records: &[CaseQualificationRecord],
    target: &RouteTarget,
    case_kind: &crate::CaseKind,
    roles: &[ReviewRole],
) -> bool {
    records.iter().any(|qualification| {
        qualification.is_qualified()
            && qualification.binding.target == *target
            && qualification.binding.case_kind == *case_kind
            && roles.contains(&qualification.binding.role)
    })
}

fn provider_error(message: &str) -> ReviewVerificationError {
    ReviewVerificationError::Provider(message.to_string())
}

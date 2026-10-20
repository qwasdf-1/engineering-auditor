use std::collections::BTreeSet;

use super::offline::{EngineeringReviewV2, ReviewVerificationError};

pub(super) fn verify_protocol_audits(
    artifact: &EngineeringReviewV2,
) -> Result<(), ReviewVerificationError> {
    verify_projection_audits(artifact)?;
    verify_semantic_revisions(artifact)?;
    verify_task_dispositions(artifact)?;
    verify_challenge_dispositions(artifact)
}

fn verify_projection_audits(artifact: &EngineeringReviewV2) -> Result<(), ReviewVerificationError> {
    let valid = artifact.projection_audits.iter().all(|audit| {
        audit.renderer_version == crate::MODEL_MARKDOWN_RENDERER_VERSION
            && audit.source_digest.len() == 64
            && audit.markdown_digest.len() == 64
            && audit.markdown_bytes > 0
            && audit
                .excluded_field_paths
                .windows(2)
                .all(|pair| pair[0] < pair[1])
    });
    valid.then_some(()).ok_or_else(|| {
        ReviewVerificationError::Reference(
            "model projection audit is invalid or non-canonical".to_string(),
        )
    })
}

fn verify_semantic_revisions(
    artifact: &EngineeringReviewV2,
) -> Result<(), ReviewVerificationError> {
    let valid = artifact.semantic_revisions.iter().all(|revision| {
        !revision.reference.trim().is_empty()
            && revision.revision >= 2
            && revision.before_digest.len() == 64
            && revision.after_digest.len() == 64
            && !revision.reason.trim().is_empty()
            && !revision.request_id.trim().is_empty()
            && !revision.tool_call_id.trim().is_empty()
    });
    valid.then_some(()).ok_or_else(|| {
        ReviewVerificationError::Reference("semantic revision audit is invalid".to_string())
    })
}

fn verify_task_dispositions(artifact: &EngineeringReviewV2) -> Result<(), ReviewVerificationError> {
    let valid = artifact
        .claim_slot_dispositions
        .iter()
        .chain(&artifact.recommendation_family_dispositions)
        .all(|disposition| {
            !disposition.task_id.trim().is_empty() && !disposition.reason.trim().is_empty()
        });
    valid.then_some(()).ok_or_else(|| {
        ReviewVerificationError::Reference("task decline disposition is invalid".to_string())
    })
}

fn verify_challenge_dispositions(
    artifact: &EngineeringReviewV2,
) -> Result<(), ReviewVerificationError> {
    let mut targets = BTreeSet::new();
    let valid = artifact.challenge_target_dispositions.iter().all(|item| {
        !item.target_id.trim().is_empty()
            && matches!(item.target_type.as_str(), "claim" | "recommendation")
            && targets.insert((
                &item.reviewer_review_id,
                &item.challenger_run_id,
                &item.target_id,
            ))
    });
    valid.then_some(()).ok_or_else(|| {
        ReviewVerificationError::Reference(
            "challenge target disposition is invalid or duplicated".to_string(),
        )
    })
}

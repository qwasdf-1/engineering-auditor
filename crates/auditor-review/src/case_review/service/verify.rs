use std::path::{Component, Path, PathBuf};

use auditor_core::sha256_digest_bytes;
use auditor_report::validate_report_json;
use serde_json::Value;

use crate::{verify_engineering_review_v2, EngineeringReviewV2, FinalCaseState};

use super::load::load_inputs;
use super::{
    failed, review_plan_path, ReviewPlanOutput, ReviewPlanRequest, ReviewServiceError,
    ReviewVerifyOutput, ReviewVerifyRequest,
};

pub fn verify_case_engineering_review(
    request: &ReviewVerifyRequest,
) -> Result<ReviewVerifyOutput, ReviewServiceError> {
    let report_text = read(&request.report, "reviewed report")?;
    validate_report_json(&report_text).map_err(failed)?;
    let report: Value =
        serde_json::from_str(&report_text).map_err(|error| failed(error.to_string()))?;
    let envelope = report
        .get("engineering_review")
        .and_then(Value::as_object)
        .ok_or_else(|| failed("reviewed report has no engineering review envelope"))?;
    if envelope.get("schema_version").and_then(Value::as_u64) != Some(2)
        || envelope.get("status").and_then(Value::as_str) != Some("complete")
        || envelope.get("completeness").and_then(Value::as_str) != Some("complete")
    {
        return Err(failed(
            "reviewed report does not contain one complete v2 review",
        ));
    }

    let artifact_path = sibling_from_envelope(
        &request.report,
        envelope.get("path").and_then(Value::as_str),
        "review artifact",
    )?;
    let artifact_text = read(&artifact_path, "review artifact")?;
    let artifact_digest = sha256_digest_bytes(artifact_text.as_bytes());
    if envelope.get("digest").and_then(Value::as_str) != Some(artifact_digest.as_str()) {
        return Err(failed("review artifact digest does not match its report"));
    }
    let artifact: EngineeringReviewV2 =
        serde_json::from_str(&artifact_text).map_err(|error| failed(error.to_string()))?;

    let plan_text = read(&review_plan_path(&request.report), "review plan")?;
    let plan = ReviewPlanOutput::parse(&plan_text)?;
    let source_request = ReviewPlanRequest {
        report: plan.sources.report.clone(),
        repo_root: request.repo_root.clone(),
        case_templates: plan.sources.case_templates.clone(),
        routes: plan.sources.routes.clone(),
        prompt_bundle: plan.sources.prompt_bundle.clone(),
        brief: plan.sources.brief.clone(),
        evidence: plan.sources.evidence.clone(),
    };
    let loaded = load_inputs(&source_request, Some(&plan.input_binding))?;
    let verification = verify_engineering_review_v2(&artifact, &loaded.input)
        .map_err(|error| failed(error.to_string()))?;
    if !verification.verified || artifact.verification != verification {
        return Err(failed(
            "review artifact does not carry its current offline verification",
        ));
    }
    verify_envelope(envelope, &artifact)?;
    Ok(ReviewVerifyOutput {
        schema_version: 2,
        review_id: artifact.review_id.clone(),
        artifact_digest,
        case_count: artifact.cases.len(),
        accepted_recommendation_count: artifact.portfolio.accepted_recommendation_ids.len(),
        blocked_case_count: count_state(&artifact, FinalCaseState::BlockedByMissingEvidence),
        contested_case_count: count_state(&artifact, FinalCaseState::Contested),
        challenge_response_count: artifact.coverage.challenge_response_count,
        valid: true,
    })
}

fn verify_envelope(
    envelope: &serde_json::Map<String, Value>,
    artifact: &EngineeringReviewV2,
) -> Result<(), ReviewServiceError> {
    let expected = [
        ("case_count", artifact.cases.len()),
        (
            "accepted_recommendation_count",
            artifact.portfolio.accepted_recommendation_ids.len(),
        ),
        (
            "blocked_case_count",
            count_state(artifact, FinalCaseState::BlockedByMissingEvidence),
        ),
        (
            "contested_case_count",
            count_state(artifact, FinalCaseState::Contested),
        ),
        (
            "challenge_response_count",
            artifact.coverage.challenge_response_count,
        ),
    ];
    let counts_match = expected.iter().all(|(field, expected)| {
        envelope.get(*field).and_then(Value::as_u64) == u64::try_from(*expected).ok()
    });
    if !counts_match
        || envelope.get("review_id").and_then(Value::as_str) != Some(artifact.review_id.as_str())
        || envelope.get("input_report_digest").and_then(Value::as_str)
            != Some(artifact.input_binding.report_digest.as_str())
    {
        return Err(failed(
            "reviewed report envelope does not match its v2 artifact",
        ));
    }
    Ok(())
}

fn count_state(artifact: &EngineeringReviewV2, state: FinalCaseState) -> usize {
    artifact
        .execution_truth
        .iter()
        .filter(|case| case.closure.final_state == state)
        .count()
}

fn sibling_from_envelope(
    report: &Path,
    name: Option<&str>,
    label: &str,
) -> Result<PathBuf, ReviewServiceError> {
    let name = name.ok_or_else(|| failed(format!("{label} path is absent")))?;
    let path = Path::new(name);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(failed(format!("{label} path is not a safe sibling")));
    }
    Ok(report.parent().unwrap_or_else(|| Path::new(".")).join(path))
}

fn read(path: &Path, label: &str) -> Result<String, ReviewServiceError> {
    std::fs::read_to_string(path).map_err(|error| {
        failed(format!(
            "failed to read {label} {}: {error}",
            path.display()
        ))
    })
}

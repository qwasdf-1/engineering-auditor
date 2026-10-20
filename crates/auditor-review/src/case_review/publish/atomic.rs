use std::fmt;
use std::path::{Path, PathBuf};

use auditor_core::{
    render_typed_markdown, sha256_digest_bytes, MarkdownDocument, MarkdownField, MarkdownSection,
    MarkdownValue,
};
use auditor_report::validate_report_json;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    verify_engineering_review_v2, ContextualRecommendation, EngineeringClaim, EngineeringReviewV2,
    FinalCaseState, ReviewInput, ENGINEERING_REVIEW_V2_SCHEMA_VERSION,
};

pub struct ReviewV2PublishRequest<'a> {
    pub input_report_path: &'a Path,
    pub reviewed_report_path: &'a Path,
    pub input_report_json: &'a str,
    pub input: &'a ReviewInput,
    pub artifact: &'a EngineeringReviewV2,
}

pub struct ReviewV2PlanSidecar<'a> {
    pub path: &'a Path,
    pub json: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReviewV2PublishOutput {
    pub review_id: crate::StableId,
    pub reviewed_report_path: PathBuf,
    pub reviewed_report_digest: String,
    pub artifact_path: PathBuf,
    pub artifact_digest: String,
    pub markdown_path: PathBuf,
}

pub fn publish_engineering_review_v2(
    request: ReviewV2PublishRequest<'_>,
) -> Result<ReviewV2PublishOutput, ReviewV2PublishError> {
    publish(request, None)
}

pub fn publish_engineering_review_v2_with_plan(
    request: ReviewV2PublishRequest<'_>,
    plan: ReviewV2PlanSidecar<'_>,
) -> Result<ReviewV2PublishOutput, ReviewV2PublishError> {
    publish(request, Some(plan))
}

fn publish(
    request: ReviewV2PublishRequest<'_>,
    plan: Option<ReviewV2PlanSidecar<'_>>,
) -> Result<ReviewV2PublishOutput, ReviewV2PublishError> {
    ensure_siblings(request.input_report_path, request.reviewed_report_path)?;
    let verification = verify_engineering_review_v2(request.artifact, request.input)
        .map_err(|error| ReviewV2PublishError::Verification(error.to_string()))?;
    if !verification.verified || request.artifact.verification != verification {
        return Err(ReviewV2PublishError::Verification(
            "artifact does not carry its current offline verification".to_string(),
        ));
    }
    let artifact_json = serde_json::to_string_pretty(request.artifact)
        .map_err(|error| ReviewV2PublishError::Serialization(error.to_string()))?;
    let artifact_digest = sha256_digest_bytes(artifact_json.as_bytes());
    let artifact_path = artifact_path(request.reviewed_report_path, &artifact_digest);
    let markdown_path = markdown_path(request.reviewed_report_path);
    let markdown = render_markdown(request.artifact);
    let reviewed_report = reviewed_report_json(
        request.input_report_json,
        request.artifact,
        &artifact_path,
        &artifact_digest,
    )?;
    validate_report_json(&reviewed_report).map_err(ReviewV2PublishError::Report)?;

    let mut files = vec![
        (artifact_path.as_path(), artifact_json.as_bytes()),
        (markdown_path.as_path(), markdown.as_bytes()),
    ];
    if let Some(plan) = &plan {
        files.push((plan.path, plan.json.as_bytes()));
    }
    files.push((request.reviewed_report_path, reviewed_report.as_bytes()));
    let staged = stage_files(&files)?;
    commit_staged(staged)?;
    Ok(ReviewV2PublishOutput {
        review_id: request.artifact.review_id.clone(),
        reviewed_report_path: request.reviewed_report_path.to_path_buf(),
        reviewed_report_digest: sha256_digest_bytes(reviewed_report.as_bytes()),
        artifact_path,
        artifact_digest,
        markdown_path,
    })
}

fn reviewed_report_json(
    input_report_json: &str,
    artifact: &EngineeringReviewV2,
    artifact_path: &Path,
    artifact_digest: &str,
) -> Result<String, ReviewV2PublishError> {
    let mut report: Value = serde_json::from_str(input_report_json)
        .map_err(|error| ReviewV2PublishError::Report(error.to_string()))?;
    let counts = case_outcome_counts(artifact);
    report["engineering_review"] = json!({
        "accepted_recommendation_count": artifact.portfolio.accepted_recommendation_ids.len(),
        "blocked_case_count": counts.blocked,
        "case_count": artifact.cases.len(),
        "challenge_response_count": artifact.coverage.challenge_response_count,
        "completeness": "complete",
        "contested_case_count": counts.contested,
        "digest": artifact_digest,
        "input_report_digest": artifact.input_binding.report_digest,
        "path": artifact_path.file_name().and_then(|name| name.to_str()),
        "review_id": artifact.review_id,
        "schema_version": ENGINEERING_REVIEW_V2_SCHEMA_VERSION,
        "status": "complete",
    });
    serde_json::to_string_pretty(&report)
        .map_err(|error| ReviewV2PublishError::Serialization(error.to_string()))
}

pub(crate) struct CaseOutcomeCounts {
    pub blocked: usize,
    pub contested: usize,
}

pub(crate) fn case_outcome_counts(artifact: &EngineeringReviewV2) -> CaseOutcomeCounts {
    CaseOutcomeCounts {
        blocked: artifact
            .execution_truth
            .iter()
            .filter(|case| case.closure.final_state == FinalCaseState::BlockedByMissingEvidence)
            .count(),
        contested: artifact
            .execution_truth
            .iter()
            .filter(|case| case.closure.final_state == FinalCaseState::Contested)
            .count(),
    }
}

pub fn engineering_review_markdown_document(artifact: &EngineeringReviewV2) -> MarkdownDocument {
    MarkdownDocument::new(
        "Engineering Review v2",
        vec![
            MarkdownSection::new(
                "Summary",
                vec![
                    text_field("Review ID", &artifact.review_id),
                    text_field("Schema version", artifact.schema_version),
                    text_field("Report findings", artifact.coverage.report_finding_count),
                    text_field(
                        "Selected findings",
                        artifact.coverage.selected_finding_count,
                    ),
                    text_field("Planned cases", artifact.coverage.planned_case_count),
                    text_field("Terminal cases", artifact.coverage.terminal_case_count),
                    text_field("Challenges", artifact.coverage.challenge_count),
                    text_field(
                        "Challenge responses",
                        artifact.coverage.challenge_response_count,
                    ),
                    text_field(
                        "Accepted recommendations",
                        artifact.portfolio.accepted_recommendation_ids.len(),
                    ),
                    MarkdownField::new(
                        "Accepted recommendation IDs",
                        MarkdownValue::list(
                            artifact
                                .portfolio
                                .accepted_recommendation_ids
                                .iter()
                                .map(ToString::to_string),
                        ),
                    ),
                ],
            ),
            MarkdownSection::new(
                "Case Closures",
                vec![MarkdownField::new(
                    "Cases",
                    MarkdownValue::Items(
                        artifact
                            .execution_truth
                            .iter()
                            .map(|case| {
                                MarkdownValue::fields(vec![
                                    text_field("Case ID", &case.case_id),
                                    MarkdownField::new(
                                        "Reviewer conclusions",
                                        MarkdownValue::list(
                                            artifact
                                                .reviewer_runs
                                                .iter()
                                                .filter(|review| review.case_id == case.case_id)
                                                .map(|review| enum_text(&review.case_conclusion)),
                                        ),
                                    ),
                                    MarkdownField::new(
                                        "Reviewer positions",
                                        MarkdownValue::list(
                                            artifact
                                                .reviewer_runs
                                                .iter()
                                                .filter(|review| review.case_id == case.case_id)
                                                .map(|review| enum_text(&review.case_position)),
                                        ),
                                    ),
                                    text_field("Final state", enum_text(&case.closure.final_state)),
                                    text_field("Reason", enum_text(&case.closure.reason)),
                                    text_field(
                                        "Failure",
                                        case.closure
                                            .failure
                                            .as_ref()
                                            .map(enum_text)
                                            .unwrap_or_default(),
                                    ),
                                    MarkdownField::new(
                                        "Detail codes",
                                        MarkdownValue::list(case.closure.detail_codes.clone()),
                                    ),
                                    MarkdownField::new(
                                        "Accepted recommendation IDs",
                                        MarkdownValue::list(
                                            case.accepted_recommendations
                                                .iter()
                                                .map(|item| item.recommendation_id.to_string()),
                                        ),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                )],
            ),
            MarkdownSection::new(
                "Admitted Claims",
                vec![MarkdownField::new(
                    "Claims",
                    MarkdownValue::Items(artifact.claims.iter().map(claim_value).collect()),
                )],
            ),
            MarkdownSection::new(
                "Contextual Recommendations",
                vec![MarkdownField::new(
                    "Recommendations",
                    MarkdownValue::Items(
                        artifact
                            .recommendations
                            .iter()
                            .map(recommendation_value)
                            .collect(),
                    ),
                )],
            ),
            MarkdownSection::new(
                "Challenges and Responses",
                vec![MarkdownField::new(
                    "Challenges",
                    MarkdownValue::Items(
                        artifact
                            .challenges
                            .iter()
                            .map(|outcome| {
                                let objections = outcome
                                    .challenge
                                    .objections
                                    .iter()
                                    .map(|objection| {
                                        MarkdownValue::fields(vec![
                                            text_field("Objection ID", &objection.objection_id),
                                            text_field(
                                                "Target type",
                                                enum_text(&objection.target_type),
                                            ),
                                            text_field("Target ID", &objection.target_id),
                                            text_field("Reason", enum_text(&objection.reason)),
                                            text_field(
                                                "Materiality",
                                                enum_text(&objection.materiality),
                                            ),
                                            text_field(
                                                "Required resolution",
                                                &objection.required_resolution,
                                            ),
                                            MarkdownField::new(
                                                "Evidence IDs",
                                                MarkdownValue::list(objection.evidence_ids.clone()),
                                            ),
                                        ])
                                    })
                                    .collect();
                                let responses = outcome
                                    .response
                                    .iter()
                                    .flat_map(|response| &response.responses)
                                    .map(|response| {
                                        MarkdownValue::fields(vec![
                                            text_field("Objection ID", &response.objection_id),
                                            text_field("Action", enum_text(&response.action)),
                                            text_field("Rationale", &response.rationale),
                                            MarkdownField::new(
                                                "Evidence probe IDs",
                                                MarkdownValue::list(
                                                    response
                                                        .evidence_probe_ids
                                                        .iter()
                                                        .map(ToString::to_string),
                                                ),
                                            ),
                                        ])
                                    })
                                    .collect();
                                MarkdownValue::fields(vec![
                                    text_field("Challenge ID", &outcome.challenge.challenge_id),
                                    text_field("Case ID", &outcome.challenge.case_id),
                                    text_field("Status", enum_text(&outcome.challenge.status)),
                                    MarkdownField::new(
                                        "Common-mode concerns",
                                        MarkdownValue::list(
                                            outcome.challenge.common_mode_concerns.clone(),
                                        ),
                                    ),
                                    MarkdownField::new(
                                        "Objections",
                                        MarkdownValue::Items(objections),
                                    ),
                                    MarkdownField::new(
                                        "Responses",
                                        MarkdownValue::Items(responses),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                )],
            ),
            MarkdownSection::new(
                "Integrity",
                vec![
                    text_field("Input report digest", &artifact.input_binding.report_digest),
                    text_field(
                        "Evidence graph digest",
                        &artifact.input_binding.evidence_graph_digest,
                    ),
                    text_field(
                        "Repository map digest",
                        &artifact.input_binding.repository_map_digest,
                    ),
                    text_field(
                        "Source snapshot digest",
                        &artifact.input_binding.source_snapshot_digest,
                    ),
                    text_field("Offline verified", artifact.verification.verified),
                    MarkdownField::new(
                        "Offline checks",
                        MarkdownValue::list(artifact.verification.checks.clone()),
                    ),
                ],
            ),
        ],
    )
}

fn render_markdown(artifact: &EngineeringReviewV2) -> String {
    render_typed_markdown(&engineering_review_markdown_document(artifact))
        .expect("rendering Markdown into a String cannot fail")
}

fn text_field(label: &str, value: impl ToString) -> MarkdownField {
    MarkdownField::new(label, MarkdownValue::text(value.to_string()))
}

fn enum_text(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn claim_value(claim: &EngineeringClaim) -> MarkdownValue {
    MarkdownValue::fields(vec![
        text_field("Claim ID", &claim.claim_id),
        text_field("Case ID", &claim.case_id),
        text_field("Slot", enum_text(&claim.slot)),
        text_field("Kind", enum_text(&claim.kind)),
        text_field("Status", enum_text(&claim.status)),
        text_field("Proposition", &claim.proposition),
        text_field("Confidence", enum_text(&claim.confidence)),
        MarkdownField::new(
            "Supporting probe IDs",
            MarkdownValue::list(claim.supporting_probe_ids.iter().map(ToString::to_string)),
        ),
        MarkdownField::new(
            "Counter probe IDs",
            MarkdownValue::list(claim.counter_probe_ids.iter().map(ToString::to_string)),
        ),
        MarkdownField::new(
            "Dependency claim IDs",
            MarkdownValue::list(claim.dependency_claim_ids.iter().map(ToString::to_string)),
        ),
        MarkdownField::new(
            "Assumptions",
            MarkdownValue::list(claim.assumptions.clone()),
        ),
    ])
}

fn recommendation_value(recommendation: &ContextualRecommendation) -> MarkdownValue {
    let benefits = recommendation
        .expected_benefits
        .iter()
        .map(|benefit| {
            MarkdownValue::fields(vec![
                text_field("Description", &benefit.description),
                text_field("Evidence mode", enum_text(&benefit.evidence_mode)),
                text_field(
                    "Numeric value",
                    benefit.numeric_value.as_deref().unwrap_or_default(),
                ),
                text_field("Unit", benefit.unit.as_deref().unwrap_or_default()),
                text_field(
                    "Extrapolation model",
                    benefit.extrapolation_model.as_deref().unwrap_or_default(),
                ),
                MarkdownField::new(
                    "Evidence probe IDs",
                    MarkdownValue::list(benefit.evidence_probe_ids.iter().map(ToString::to_string)),
                ),
            ])
        })
        .collect();
    let alternatives = recommendation
        .alternatives
        .iter()
        .map(|alternative| {
            MarkdownValue::fields(vec![
                text_field("Summary", &alternative.summary),
                MarkdownField::new(
                    "Tradeoffs",
                    MarkdownValue::list(alternative.tradeoffs.clone()),
                ),
            ])
        })
        .collect();
    let verification = recommendation
        .verification_steps
        .iter()
        .map(|step| {
            MarkdownValue::fields(vec![
                text_field("Method", &step.method),
                text_field("Expected observation", &step.expected_observation),
                MarkdownField::new(
                    "Evidence probe IDs",
                    MarkdownValue::list(step.evidence_probe_ids.iter().map(ToString::to_string)),
                ),
            ])
        })
        .collect();
    MarkdownValue::fields(vec![
        text_field("Recommendation ID", &recommendation.recommendation_id),
        text_field("Case ID", &recommendation.case_id),
        text_field("Family", enum_text(&recommendation.family)),
        text_field("Summary", &recommendation.summary),
        MarkdownField::new(
            "Scope IDs",
            MarkdownValue::list(recommendation.scope_ids.clone()),
        ),
        MarkdownField::new(
            "Premise claim IDs",
            MarkdownValue::list(
                recommendation
                    .premise_claim_ids
                    .iter()
                    .map(ToString::to_string),
            ),
        ),
        MarkdownField::new(
            "Affected finding IDs",
            MarkdownValue::list(recommendation.affected_finding_ids.clone()),
        ),
        text_field(
            "Detector advice relationship",
            enum_text(&recommendation.detector_advice_relationship),
        ),
        MarkdownField::new(
            "Preconditions",
            MarkdownValue::list(recommendation.preconditions.clone()),
        ),
        MarkdownField::new("Expected benefits", MarkdownValue::Items(benefits)),
        MarkdownField::new("Risks", MarkdownValue::list(recommendation.risks.clone())),
        MarkdownField::new("Costs", MarkdownValue::list(recommendation.costs.clone())),
        MarkdownField::new("Alternatives", MarkdownValue::Items(alternatives)),
        text_field(
            "No-alternative reason",
            recommendation
                .no_alternative_reason
                .as_deref()
                .unwrap_or_default(),
        ),
        MarkdownField::new("Verification steps", MarkdownValue::Items(verification)),
        text_field("Materiality", enum_text(&recommendation.materiality)),
        text_field("Materiality basis", &recommendation.materiality_basis),
        text_field("Confidence", enum_text(&recommendation.confidence)),
    ])
}

struct StagedFile {
    destination: PathBuf,
    temporary: PathBuf,
    backup: Option<PathBuf>,
    existed: bool,
}

fn stage_files(files: &[(&Path, &[u8])]) -> Result<Vec<StagedFile>, ReviewV2PublishError> {
    let mut staged = Vec::with_capacity(files.len());
    let mut destinations = std::collections::BTreeSet::new();
    for &(destination, bytes) in files {
        if !destinations.insert(destination) {
            cleanup_staged(&staged);
            return Err(ReviewV2PublishError::UnsafeOutput);
        }
        let result = stage_file(destination, bytes);
        match result {
            Ok(file) => staged.push(file),
            Err(error) => {
                cleanup_staged(&staged);
                return Err(error);
            }
        }
    }
    Ok(staged)
}

fn stage_file(destination: &Path, bytes: &[u8]) -> Result<StagedFile, ReviewV2PublishError> {
    if destination.exists() && !destination.is_file() {
        return Err(ReviewV2PublishError::UnsafeOutput);
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| ReviewV2PublishError::Io(error.to_string()))?;
    let temporary = destination.with_extension(format!("v2-tmp-{}", std::process::id()));
    std::fs::write(&temporary, bytes)
        .map_err(|error| ReviewV2PublishError::Io(error.to_string()))?;
    Ok(StagedFile {
        destination: destination.to_path_buf(),
        temporary,
        backup: None,
        existed: destination.exists(),
    })
}

fn commit_staged(mut staged: Vec<StagedFile>) -> Result<(), ReviewV2PublishError> {
    for index in 0..staged.len() {
        if staged[index].existed {
            let backup = staged[index]
                .destination
                .with_extension(format!("v2-backup-{}", std::process::id()));
            if let Err(error) = std::fs::rename(&staged[index].destination, &backup) {
                rollback(&staged, index);
                return Err(ReviewV2PublishError::Io(error.to_string()));
            }
            staged[index].backup = Some(backup);
        }
        if let Err(error) = std::fs::rename(&staged[index].temporary, &staged[index].destination) {
            rollback(&staged, index + 1);
            return Err(ReviewV2PublishError::Io(error.to_string()));
        }
    }
    for file in staged {
        if let Some(backup) = file.backup {
            let _ = std::fs::remove_file(backup);
        }
    }
    Ok(())
}

fn rollback(staged: &[StagedFile], committed: usize) {
    for file in staged.iter().take(committed).rev() {
        let _ = std::fs::remove_file(&file.destination);
        if let Some(backup) = &file.backup {
            let _ = std::fs::rename(backup, &file.destination);
        }
    }
    cleanup_staged(staged);
}

fn cleanup_staged(staged: &[StagedFile]) {
    for file in staged {
        let _ = std::fs::remove_file(&file.temporary);
    }
}

fn ensure_siblings(input: &Path, output: &Path) -> Result<(), ReviewV2PublishError> {
    if input == output
        || input.parent().unwrap_or_else(|| Path::new("."))
            != output.parent().unwrap_or_else(|| Path::new("."))
    {
        return Err(ReviewV2PublishError::UnsafeOutput);
    }
    Ok(())
}

fn artifact_path(output: &Path, digest: &str) -> PathBuf {
    sidecar_path(output, &format!("engineering-review-v2.{digest}.json"))
}

fn markdown_path(output: &Path) -> PathBuf {
    sidecar_path(output, "review-v2.md")
}

pub(crate) fn sidecar_path(output: &Path, suffix: &str) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audit");
    output
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}.{suffix}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewV2PublishError {
    UnsafeOutput,
    Verification(String),
    Serialization(String),
    Report(String),
    Io(String),
}

impl fmt::Display for ReviewV2PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ReviewV2PublishError {}

use std::path::{Path, PathBuf};

use crate::{
    publish_engineering_review_v2_with_plan, CaseBudgetLedger, EngineeringReviewV2,
    EngineeringReviewV2Draft, ReviewControl, ReviewPlanOutput, ReviewStage, ReviewV2PlanSidecar,
    ReviewV2PublishRequest, TranscriptRecorder, Transport,
};

use super::load::load_inputs;
use super::qualification::{qualify_plan, QualificationRuntime};
use super::review_case::{
    complete_without_provider, execute_planned_case, CaseExecutionRuntime, CompletedCase,
};
use super::{
    checkpoint, failed, plan_engineering_review, review_plan_path, LoadedPlanInputs,
    ReviewRunOutput, ReviewRunRequest, ReviewServiceError,
};

struct ExecutionContext<'a> {
    plan: &'a ReviewPlanOutput,
    loaded: &'a LoadedPlanInputs,
    qualifications: &'a [crate::CaseQualificationRecord],
    ledger: &'a CaseBudgetLedger,
    request: &'a ReviewRunRequest,
    transport: &'a dyn Transport,
    control: &'a dyn ReviewControl,
    transcript: &'a TranscriptRecorder,
    transcript_enabled: bool,
}

struct PublishContext<'a> {
    request: &'a ReviewRunRequest,
    transcript: &'a TranscriptRecorder,
    transcript_enabled: bool,
    plan: &'a ReviewPlanOutput,
    input: &'a crate::ReviewInput,
}

pub fn run_case_engineering_review(
    request: &ReviewRunRequest,
    transport: &dyn Transport,
    control: &dyn ReviewControl,
) -> Result<ReviewRunOutput, ReviewServiceError> {
    let transcript = TranscriptRecorder::enabled();
    let transcript_enabled = crate::service::transcript_enabled_from_env();
    let result = run_case_engineering_review_inner(
        request,
        transport,
        control,
        &transcript,
        transcript_enabled,
    );
    if result.is_err() && transcript_enabled {
        crate::service::publish_failure_transcript(&transcript, &request.output, None);
    }
    result
}

fn run_case_engineering_review_inner(
    request: &ReviewRunRequest,
    transport: &dyn Transport,
    control: &dyn ReviewControl,
    transcript: &TranscriptRecorder,
    transcript_enabled: bool,
) -> Result<ReviewRunOutput, ReviewServiceError> {
    checkpoint(control, ReviewStage::Preparing)?;
    ensure_siblings(&request.plan.report, &request.output)?;
    checkpoint(control, ReviewStage::Planning)?;
    let plan = plan_engineering_review(&request.plan)?;
    checkpoint(control, ReviewStage::CompilingEvidence)?;
    checkpoint(control, ReviewStage::ResolvingProbes)?;
    let loaded = load_inputs(&request.plan, Some(&plan.input_binding))?;
    let qualifications = qualify_plan(
        &plan,
        QualificationRuntime {
            routes: &loaded.routes,
            prompts: &loaded.prompts,
            transport,
            consent: request.consent,
            control,
            transcript,
            transcript_enabled,
        },
    )?;
    let ledger = CaseBudgetLedger::new(
        loaded.routes.review_budget,
        loaded.routes.case_budgets.clone(),
    )
    .map_err(failed)?;
    let draft = execute_cases(ExecutionContext {
        plan: &plan,
        loaded: &loaded,
        qualifications: &qualifications,
        ledger: &ledger,
        request,
        transport,
        control,
        transcript,
        transcript_enabled,
    })?;
    checkpoint(control, ReviewStage::Comparing)?;
    checkpoint(control, ReviewStage::Finalizing)?;
    let artifact = EngineeringReviewV2::build(&loaded.input, draft)
        .map_err(|error| failed(error.to_string()))?;
    checkpoint(control, ReviewStage::Publishing)?;
    publish_run(
        PublishContext {
            request,
            transcript,
            transcript_enabled,
            plan: &plan,
            input: &loaded.input,
        },
        artifact,
    )
}

fn execute_cases(
    context: ExecutionContext<'_>,
) -> Result<EngineeringReviewV2Draft, ReviewServiceError> {
    let mut draft = EngineeringReviewV2Draft {
        agenda: context.plan.agenda.clone(),
        cases: context
            .plan
            .cases
            .iter()
            .map(|planned| planned.case.clone())
            .collect(),
        evidence_packets: Vec::new(),
        evidence_packet_history: Vec::new(),
        probe_definitions: Vec::new(),
        probe_results: Vec::new(),
        reviewer_runs: Vec::new(),
        ensemble_runs: Vec::new(),
        ensemble_failures: Vec::new(),
        ensemble_evidence_packets: Vec::new(),
        ensemble_challenges: Vec::new(),
        claims: Vec::new(),
        recommendations: Vec::new(),
        challenges: Vec::new(),
        execution_truth: Vec::new(),
        qualification_records: context.qualifications.to_vec(),
        provider_records: Vec::new(),
        egress_manifests: Vec::new(),
        claim_slot_dispositions: Vec::new(),
        recommendation_family_dispositions: Vec::new(),
        challenge_target_dispositions: Vec::new(),
        semantic_revisions: Vec::new(),
        projection_audits: Vec::new(),
    };
    for planned in &context.plan.cases {
        let completed = if matches!(
            planned.routing.selection.mode,
            crate::DeliberationMode::SingleReviewer | crate::DeliberationMode::Ensemble
        ) && planned.routing.closure.is_none()
        {
            execute_planned_case(
                planned,
                CaseExecutionRuntime {
                    loaded: context.loaded,
                    routes: &context.loaded.routes,
                    prompts: &context.loaded.prompts,
                    qualifications: context.qualifications,
                    ledger: context.ledger,
                    transport: context.transport,
                    consent: context.request.consent,
                    control: context.control,
                    transcript: context.transcript,
                    transcript_enabled: context.transcript_enabled,
                },
            )?
        } else {
            complete_without_provider(planned)?
        };
        absorb_case(&mut draft, completed);
    }
    Ok(draft)
}

fn absorb_case(draft: &mut EngineeringReviewV2Draft, completed: CompletedCase) {
    let CompletedCase {
        packet,
        packet_history,
        definitions,
        results,
        review,
        ensemble_run,
        ensemble_failure,
        ensemble_evidence_packets,
        ensemble_challenges,
        challenge,
        finalization,
        calls,
        egress,
        claim_slot_dispositions,
        recommendation_family_dispositions,
        challenge_target_dispositions,
        semantic_revisions,
        projection_audits,
    } = completed;
    draft.evidence_packets.push(packet);
    draft.evidence_packet_history.extend(packet_history);
    draft.probe_definitions.extend(definitions);
    draft.probe_results.extend(results);
    if let Some(review) = review {
        draft.claims.extend(review.claims.clone());
        draft.recommendations.extend(review.recommendations.clone());
        draft.reviewer_runs.push(review);
    }
    draft.ensemble_runs.extend(ensemble_run);
    draft.ensemble_failures.extend(ensemble_failure);
    draft
        .ensemble_evidence_packets
        .extend(ensemble_evidence_packets);
    draft.ensemble_challenges.extend(ensemble_challenges);
    draft.challenges.extend(challenge);
    draft.execution_truth.push(finalization);
    draft.provider_records.extend(calls);
    draft.egress_manifests.extend(egress);
    absorb_task_audits(
        draft,
        CompletedTaskAudits {
            claim_slot_dispositions,
            recommendation_family_dispositions,
            challenge_target_dispositions,
            semantic_revisions,
            projection_audits,
        },
    );
}

struct CompletedTaskAudits {
    claim_slot_dispositions: Vec<crate::TaskDeclineDisposition>,
    recommendation_family_dispositions: Vec<crate::TaskDeclineDisposition>,
    challenge_target_dispositions: Vec<crate::ChallengeTargetDispositionRecord>,
    semantic_revisions: Vec<crate::SemanticRevisionRecord>,
    projection_audits: Vec<crate::ModelProjectionAudit>,
}

fn absorb_task_audits(draft: &mut EngineeringReviewV2Draft, audits: CompletedTaskAudits) {
    draft
        .claim_slot_dispositions
        .extend(audits.claim_slot_dispositions);
    draft
        .recommendation_family_dispositions
        .extend(audits.recommendation_family_dispositions);
    draft
        .challenge_target_dispositions
        .extend(audits.challenge_target_dispositions);
    draft.semantic_revisions.extend(audits.semantic_revisions);
    draft.projection_audits.extend(audits.projection_audits);
}

fn publish_run(
    context: PublishContext<'_>,
    artifact: EngineeringReviewV2,
) -> Result<ReviewRunOutput, ReviewServiceError> {
    let PublishContext {
        request,
        transcript,
        transcript_enabled,
        plan,
        input,
    } = context;
    let input_report_json = std::fs::read_to_string(&request.plan.report)
        .map_err(|error| failed(format!("failed to read input report: {error}")))?;
    let plan_path = review_plan_path(&request.output);
    let plan_json =
        serde_json::to_string_pretty(&plan).map_err(|error| failed(error.to_string()))?;
    let published = publish_engineering_review_v2_with_plan(
        ReviewV2PublishRequest {
            input_report_path: &request.plan.report,
            reviewed_report_path: &request.output,
            input_report_json: &input_report_json,
            input,
            artifact: &artifact,
        },
        ReviewV2PlanSidecar {
            path: &plan_path,
            json: &plan_json,
        },
    )
    .map_err(|error| failed(error.to_string()))?;
    let reviewed_report = std::fs::read_to_string(&published.reviewed_report_path)
        .map_err(|error| failed(format!("failed to read published report: {error}")))?;
    if transcript_enabled {
        let review_id = artifact.review_id.to_string();
        crate::service::publish_transcript(transcript, &request.output, Some(&review_id))?;
    }
    Ok(run_output(artifact, published, plan_path, reviewed_report))
}

fn run_output(
    artifact: EngineeringReviewV2,
    published: crate::ReviewV2PublishOutput,
    plan_path: PathBuf,
    reviewed_report: String,
) -> ReviewRunOutput {
    let counts = crate::case_review::publish::case_outcome_counts(&artifact);
    ReviewRunOutput {
        schema_version: 2,
        review_id: artifact.review_id,
        reviewed_report_path: published.reviewed_report_path,
        reviewed_report_digest: published.reviewed_report_digest,
        review_artifact_path: published.artifact_path,
        review_artifact_digest: published.artifact_digest,
        review_plan_path: plan_path,
        case_count: artifact.cases.len(),
        accepted_recommendation_count: artifact.portfolio.accepted_recommendation_ids.len(),
        blocked_case_count: counts.blocked,
        contested_case_count: counts.contested,
        challenge_response_count: artifact.coverage.challenge_response_count,
        reviewed_report,
    }
}

fn ensure_siblings(input: &Path, output: &Path) -> Result<(), ReviewServiceError> {
    if input == output
        || input.parent().unwrap_or_else(|| Path::new("."))
            != output.parent().unwrap_or_else(|| Path::new("."))
    {
        return Err(failed(
            "reviewed report must be distinct and beside its input report",
        ));
    }
    Ok(())
}

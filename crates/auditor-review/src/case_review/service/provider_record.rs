use std::time::{SystemTime, UNIX_EPOCH};

use auditor_core::sha256_digest_bytes;

use crate::{
    AssessmentModeRecorded, CaseBudgetLedger, CaseCallContext, CaseProviderCallRecord,
    ContextBlockKind, DeliberationMode, EgressManifest, EgressManifestEntry, ProviderCallRecord,
    ProviderFailureClass, ProviderRequest, RepairContext, ReviewRole, ReviewStage, RouteTarget,
    TrustLabel, Usage,
};

use super::{
    failed,
    provider_call::{CaseCallOutput, CaseCallRequest},
    ReviewServiceError,
};

pub(super) struct AttemptRecordInput<'attempt, 'case_data> {
    pub provider: &'attempt crate::ProviderConfig,
    pub target: &'attempt RouteTarget,
    pub ledger: &'attempt CaseBudgetLedger,
    pub case_request: &'attempt CaseCallRequest<'case_data>,
    pub provider_request: &'attempt ProviderRequest,
    pub assessment_mode: AssessmentModeRecorded,
    pub repair_attempts: u8,
    pub input_bytes: usize,
    pub reserved_output_tokens: usize,
    pub started: u128,
    pub finished: u128,
}

pub(super) struct AttemptAvailability {
    pub retry: bool,
    pub repair: bool,
}

pub(super) enum FailedAttempt {
    Retry,
    Repair(RepairContext),
    Stop,
}

pub(super) fn complete_successful_attempt(
    response: crate::ProviderResponse,
    attempt: AttemptRecordInput<'_, '_>,
    calls: &mut Vec<CaseProviderCallRecord>,
    egress: &mut Vec<EgressManifest>,
) -> Result<CaseCallOutput, ReviewServiceError> {
    let actual_output_tokens = if response.usage == Usage::default() {
        attempt.reserved_output_tokens
    } else {
        response.usage.output_tokens
    };
    attempt
        .ledger
        .settle_output(
            &attempt.case_request.case.case_id,
            attempt.reserved_output_tokens,
            actual_output_tokens,
        )
        .map_err(|error| failed(error.to_string()))?;
    let output_bytes = serde_json::to_vec(&response.structured_output)
        .map_err(|error| failed(error.to_string()))?;
    let call = provider_call_record(CallRecordInput {
        provider: attempt.provider,
        model: &attempt.target.model,
        request: attempt.provider_request,
        started: attempt.started,
        finished: attempt.finished,
        output: Some(&output_bytes),
        finish_reason: response.finish_reason.clone(),
        usage: response.usage,
        failure_class: None,
        discarded: false,
        assessment_mode: attempt.assessment_mode,
        repair_attempts: attempt.repair_attempts,
        input_bytes: attempt.input_bytes,
    });
    calls.push(CaseProviderCallRecord::from_call(
        call,
        call_context(attempt.case_request),
    ));
    Ok(CaseCallOutput {
        value: response.structured_output,
        target: attempt.target.clone(),
        calls: std::mem::take(calls),
        egress: std::mem::take(egress),
    })
}

pub(super) fn complete_failed_attempt(
    failure: crate::ProviderFailure,
    attempt: AttemptRecordInput<'_, '_>,
    availability: AttemptAvailability,
    calls: &mut Vec<CaseProviderCallRecord>,
    failures: &mut Vec<String>,
) -> Result<FailedAttempt, ReviewServiceError> {
    let retry = availability.retry && failure.retryable_without_model_output;
    let repair = availability.repair
        && failure.class == ProviderFailureClass::InvalidOutput
        && failure.prior_assistant_output.is_some();
    let settled_output_tokens = if failure.retryable_without_model_output {
        Some(0)
    } else if failure.usage != Usage::default() {
        Some(failure.usage.output_tokens)
    } else {
        None
    };
    let failure_detail = failure.message.clone();
    calls.push(
        CaseProviderCallRecord::from_call(
            provider_call_record(CallRecordInput {
                provider: attempt.provider,
                model: &attempt.target.model,
                request: attempt.provider_request,
                started: attempt.started,
                finished: attempt.finished,
                output: None,
                finish_reason: None,
                usage: failure.usage,
                failure_class: Some(failure.class),
                discarded: true,
                assessment_mode: attempt.assessment_mode,
                repair_attempts: attempt.repair_attempts,
                input_bytes: attempt.input_bytes,
            }),
            call_context(attempt.case_request),
        )
        .with_failure_detail(failure_detail),
    );
    failures.push(format!(
        "{}:{}: {}",
        attempt.target.provider_id, attempt.target.model, failure.message
    ));
    settle_failed_output(&attempt, settled_output_tokens)?;
    if retry {
        return Ok(FailedAttempt::Retry);
    }
    if repair {
        return Ok(FailedAttempt::Repair(RepairContext {
            stage: attempt.case_request.stage_name.to_string(),
            prior_assistant_output: failure
                .prior_assistant_output
                .expect("repair eligibility requires prior output")
                .into_string(),
            validation_error: failure.message,
            schema: attempt.case_request.output_schema.clone(),
        }));
    }
    Ok(FailedAttempt::Stop)
}

fn settle_failed_output(
    attempt: &AttemptRecordInput<'_, '_>,
    actual_output_tokens: Option<usize>,
) -> Result<(), ReviewServiceError> {
    let Some(actual_output_tokens) = actual_output_tokens else {
        return Ok(());
    };
    attempt
        .ledger
        .settle_output(
            &attempt.case_request.case.case_id,
            attempt.reserved_output_tokens,
            actual_output_tokens,
        )
        .map_err(|error| failed(error.to_string()))
}

pub(super) fn egress_manifest(request: &CaseCallRequest<'_>, byte_count: usize) -> EgressManifest {
    let block = EgressManifestEntry {
        kind: ContextBlockKind::CaseEvidencePacket,
        provenance: format!(
            "case-provider-context:{}:{}",
            request.case.case_id, request.stage_name
        ),
        digest: sha256_digest_bytes(
            serde_json::to_string(&request.context)
                .expect("provider context is serializable")
                .as_bytes(),
        ),
        trust: if request.role == ReviewRole::Challenger || request.stage == ReviewStage::Responding
        {
            TrustLabel::UntrustedProviderOutput
        } else {
            TrustLabel::DeterministicEvidence
        },
        byte_count,
    };
    EgressManifest {
        schema_version: 1,
        block_count: 1,
        byte_count,
        blocks: vec![block],
    }
}

pub(super) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn call_context(request: &CaseCallRequest<'_>) -> CaseCallContext {
    CaseCallContext {
        packet_digest: request.packet.packet_digest.clone(),
        role: request.role,
        case_id: request.case.case_id.clone(),
        pass: request.pass,
        mode: DeliberationMode::SingleReviewer,
    }
}

struct CallRecordInput<'a> {
    provider: &'a crate::ProviderConfig,
    model: &'a str,
    request: &'a ProviderRequest,
    started: u128,
    finished: u128,
    output: Option<&'a [u8]>,
    finish_reason: Option<String>,
    usage: Usage,
    failure_class: Option<ProviderFailureClass>,
    discarded: bool,
    assessment_mode: AssessmentModeRecorded,
    repair_attempts: u8,
    input_bytes: usize,
}

fn provider_call_record(input: CallRecordInput<'_>) -> ProviderCallRecord {
    ProviderCallRecord {
        request_id: input.request.request_id.clone(),
        provider_id: input.provider.id.clone(),
        provider_kind: input.provider.kind,
        model: input.model.to_string(),
        stage: input.request.stage.clone(),
        started_at_unix_millis: input.started,
        finished_at_unix_millis: input.finished,
        context_digest: input.request.context_digest(),
        output_digest: input.output.map(sha256_digest_bytes),
        input_bytes: input.input_bytes,
        output_bytes: input.output.map_or(0, <[u8]>::len),
        finish_reason: input.finish_reason,
        usage: input.usage,
        failure_class: input.failure_class,
        discarded: input.discarded,
        assessment_mode: input.assessment_mode,
        repair_attempts: input.repair_attempts,
    }
}

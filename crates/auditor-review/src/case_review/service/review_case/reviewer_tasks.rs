use crate::{NativeToolCall, ReviewerTaskLedger, TaskCallProvenance};

use super::{
    optional_string_field, recommendation_replacement, string_array_field, string_field,
    task_application::{
        apply_native_task_calls, apply_revision, apply_typed_transition, checked_revision,
        LedgerTransition, TaskApplication, TransitionKind,
    },
    task_error, validate_probe_answer_arguments, CaseEvidenceState, EngineeringCase,
    ProviderProbeDefinition,
};

struct ReviewerDispatch<'a> {
    probe_contracts: &'a [ProviderProbeDefinition],
    request_id: &'a str,
}

pub(super) struct ReviewerInput<'a> {
    pub(super) calls: &'a [NativeToolCall],
    pub(super) case: &'a EngineeringCase,
    pub(super) state: &'a CaseEvidenceState,
    pub(super) probe_contracts: &'a [ProviderProbeDefinition],
    pub(super) request_id: &'a str,
}

pub(super) fn apply_reviewer_task_calls(
    ledger: &mut ReviewerTaskLedger,
    request: ReviewerInput<'_>,
) -> TaskApplication {
    let ReviewerInput {
        calls,
        case,
        state,
        probe_contracts,
        request_id,
    } = request;
    let mut application = TaskApplication::default();
    let dispatch = ReviewerDispatch {
        probe_contracts,
        request_id,
    };
    apply_native_task_calls(
        &mut application,
        calls,
        "The task call was recorded.",
        |index, call| apply_task_call(ledger, call, index + 1 == calls.len(), &dispatch),
    );
    application.validate_evidence_requests(case, state);
    application
}

fn apply_task_call(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    is_last: bool,
    dispatch: &ReviewerDispatch<'_>,
) -> Result<(), String> {
    if call.name == crate::TOOL_COMPLETE_CASE_REVIEW {
        return complete_review(ledger, call, is_last);
    }
    if let Some((kind, reopen)) = transition_kind(&call.name) {
        return apply_typed_transition(ledger, call, dispatch.request_id, kind, reopen);
    }
    match call.name.as_str() {
        crate::TOOL_ANSWER_CASE_PROBE => answer_probe(ledger, call, dispatch),
        crate::TOOL_RECORD_CASE_CLAIM => record_claim(ledger, call, dispatch.request_id),
        crate::TOOL_RECORD_CASE_RECOMMENDATION => {
            record_recommendation(ledger, call, dispatch.request_id)
        }
        crate::TOOL_REVISE_CASE_PROBE_ANSWER => revise_probe(ledger, call, dispatch),
        crate::TOOL_REVISE_CASE_CLAIM => apply_revision(
            ledger,
            call,
            dispatch.request_id,
            checked_revision(call, "claim_ref", Ok)?,
            ReviewerTaskLedger::revise_claim,
        ),
        crate::TOOL_REVISE_CASE_RECOMMENDATION => apply_revision(
            ledger,
            call,
            dispatch.request_id,
            checked_revision(call, "recommendation_ref", recommendation_replacement)?,
            ReviewerTaskLedger::revise_recommendation,
        ),
        other => Err(format!("unknown reviewer task tool `{other}`")),
    }
}

fn complete_review(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    is_last: bool,
) -> Result<(), String> {
    if !is_last {
        return Err("complete_case_review must be the last tool call".to_string());
    }
    let preferred = optional_string_field(&call.arguments, "preferred_recommendation_ref");
    let gaps = string_array_field(&call.arguments, "unresolved_gaps")?;
    ledger.complete(preferred, gaps).map_err(task_error)
}

fn answer_probe(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    dispatch: &ReviewerDispatch<'_>,
) -> Result<(), String> {
    validate_probe_answer_arguments(&call.arguments, dispatch.probe_contracts)?;
    let probe_id = string_field(&call.arguments, "probe_id")?;
    ledger
        .answer_probe(
            probe_id,
            call.arguments.clone(),
            provenance(dispatch.request_id, call),
        )
        .map_err(task_error)
}

fn record_claim(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    request_id: &str,
) -> Result<(), String> {
    let reference = string_field(&call.arguments, "claim_ref")?;
    let slot = string_field(&call.arguments, "slot")?;
    ledger
        .record_claim(
            reference,
            slot,
            call.arguments.clone(),
            provenance(request_id, call),
        )
        .map_err(task_error)
}

fn record_recommendation(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    request_id: &str,
) -> Result<(), String> {
    let arguments = recommendation_replacement(call.arguments.clone())?;
    let reference = string_field(&arguments, "recommendation_ref")?;
    let family = string_field(&arguments, "family")?;
    ledger
        .record_recommendation(
            reference,
            family,
            arguments.clone(),
            provenance(request_id, call),
        )
        .map_err(task_error)
}

fn revise_probe(
    ledger: &mut ReviewerTaskLedger,
    call: &NativeToolCall,
    dispatch: &ReviewerDispatch<'_>,
) -> Result<(), String> {
    apply_revision(
        ledger,
        call,
        dispatch.request_id,
        checked_revision(call, "probe_id", |replacement| {
            validate_probe_answer_arguments(&replacement, dispatch.probe_contracts)?;
            Ok(replacement)
        })?,
        ReviewerTaskLedger::revise_probe_answer,
    )
}

fn provenance<'a>(request_id: &'a str, call: &'a NativeToolCall) -> TaskCallProvenance<'a> {
    TaskCallProvenance::new(request_id, &call.call_id)
}

#[derive(Clone, Copy)]
enum ReviewerRecordKind {
    Claim,
    Recommendation,
}

impl TransitionKind<ReviewerTaskLedger> for ReviewerRecordKind {
    fn reference_field(self) -> &'static str {
        match self {
            Self::Claim => "claim_ref",
            Self::Recommendation => "recommendation_ref",
        }
    }

    fn transition(self, reopen: bool) -> LedgerTransition<ReviewerTaskLedger> {
        match (self, reopen) {
            (Self::Claim, true) => ReviewerTaskLedger::reopen_claim,
            (Self::Claim, false) => ReviewerTaskLedger::withdraw_claim,
            (Self::Recommendation, true) => ReviewerTaskLedger::reopen_recommendation,
            (Self::Recommendation, false) => ReviewerTaskLedger::withdraw_recommendation,
        }
    }
}

fn transition_kind(tool_name: &str) -> Option<(ReviewerRecordKind, bool)> {
    match tool_name {
        crate::TOOL_WITHDRAW_CASE_CLAIM => Some((ReviewerRecordKind::Claim, false)),
        crate::TOOL_REOPEN_CASE_CLAIM => Some((ReviewerRecordKind::Claim, true)),
        crate::TOOL_WITHDRAW_CASE_RECOMMENDATION => {
            Some((ReviewerRecordKind::Recommendation, false))
        }
        crate::TOOL_REOPEN_CASE_RECOMMENDATION => Some((ReviewerRecordKind::Recommendation, true)),
        _ => None,
    }
}

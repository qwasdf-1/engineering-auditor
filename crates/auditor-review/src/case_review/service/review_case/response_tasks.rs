use crate::{NativeConversationMessage, NativeToolCall, ResponseTaskLedger, TaskCallProvenance};

use super::{
    recommendation_replacement, string_field,
    task_application::{
        apply_revision, apply_typed_transition, checked_revision, record_native_value,
        LedgerTransition, TransitionKind,
    },
    task_error, task_tool_result,
};

pub(super) struct ResponseTaskApplication {
    pub(super) tool_results: Vec<NativeConversationMessage>,
    pub(super) errors: Vec<String>,
}

pub(super) fn apply_response_task_calls(
    ledger: &mut ResponseTaskLedger,
    calls: &[NativeToolCall],
    request_id: &str,
) -> ResponseTaskApplication {
    let mut application = ResponseTaskApplication {
        tool_results: Vec::new(),
        errors: Vec::new(),
    };
    for (index, call) in calls.iter().enumerate() {
        let result = apply_call(ledger, call, index + 1 == calls.len(), request_id);
        record_result(&mut application, call, result);
    }
    application
}

fn apply_call(
    ledger: &mut ResponseTaskLedger,
    call: &NativeToolCall,
    is_last: bool,
    request_id: &str,
) -> Result<(), String> {
    if call.name == crate::TOOL_COMPLETE_CHALLENGE_RESPONSE {
        return complete_response(ledger, is_last);
    }
    if let Some((kind, reopen)) = transition_kind(&call.name) {
        return apply_typed_transition(ledger, call, request_id, kind, reopen);
    }
    match call.name.as_str() {
        crate::TOOL_RECORD_REPLACEMENT_RECOMMENDATION => {
            record_replacement(ledger, call, request_id)
        }
        crate::TOOL_RECORD_OBJECTION_RESPONSE => record_native_value(
            ledger,
            call,
            request_id,
            "objection_id",
            ResponseTaskLedger::record_response,
        ),
        crate::TOOL_REVISE_REPLACEMENT_RECOMMENDATION => apply_revision(
            ledger,
            call,
            request_id,
            checked_revision(call, "recommendation_ref", recommendation_replacement)?,
            ResponseTaskLedger::revise_replacement_recommendation,
        ),
        crate::TOOL_REVISE_OBJECTION_RESPONSE => apply_revision(
            ledger,
            call,
            request_id,
            checked_revision(call, "objection_id", Ok)?,
            ResponseTaskLedger::revise_response,
        ),
        other => Err(format!("unknown challenge-response task tool `{other}`")),
    }
}

fn complete_response(ledger: &mut ResponseTaskLedger, is_last: bool) -> Result<(), String> {
    if !is_last {
        return Err("complete_challenge_response must be the last tool call".to_string());
    }
    ledger.complete().map_err(task_error)
}

fn record_replacement(
    ledger: &mut ResponseTaskLedger,
    call: &NativeToolCall,
    request_id: &str,
) -> Result<(), String> {
    let arguments = recommendation_replacement(call.arguments.clone())?;
    let reference = string_field(&arguments, "recommendation_ref")?;
    ledger
        .record_replacement_recommendation(
            reference,
            arguments.clone(),
            provenance(request_id, call),
        )
        .map_err(task_error)
}

#[derive(Clone, Copy)]
enum ResponseRecordKind {
    Replacement,
    Response,
}

impl TransitionKind<ResponseTaskLedger> for ResponseRecordKind {
    fn reference_field(self) -> &'static str {
        match self {
            Self::Replacement => "recommendation_ref",
            Self::Response => "objection_id",
        }
    }

    fn transition(self, reopen: bool) -> LedgerTransition<ResponseTaskLedger> {
        match (self, reopen) {
            (Self::Replacement, true) => ResponseTaskLedger::reopen_replacement_recommendation,
            (Self::Replacement, false) => ResponseTaskLedger::withdraw_replacement_recommendation,
            (Self::Response, true) => ResponseTaskLedger::reopen_response,
            (Self::Response, false) => ResponseTaskLedger::withdraw_response,
        }
    }
}

fn provenance<'a>(request_id: &'a str, call: &'a NativeToolCall) -> TaskCallProvenance<'a> {
    TaskCallProvenance::new(request_id, &call.call_id)
}

fn record_result(
    application: &mut ResponseTaskApplication,
    call: &NativeToolCall,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => application.tool_results.push(task_tool_result(
            call,
            "Accepted",
            "The challenge-response task call was recorded.",
        )),
        Err(error) => {
            application
                .errors
                .push(format!("{}.{}: {error}", call.name, call.call_id));
            application
                .tool_results
                .push(task_tool_result(call, "Rejected", &error));
        }
    }
}

fn transition_kind(tool_name: &str) -> Option<(ResponseRecordKind, bool)> {
    match tool_name {
        crate::TOOL_WITHDRAW_REPLACEMENT_RECOMMENDATION => {
            Some((ResponseRecordKind::Replacement, false))
        }
        crate::TOOL_REOPEN_REPLACEMENT_RECOMMENDATION => {
            Some((ResponseRecordKind::Replacement, true))
        }
        crate::TOOL_WITHDRAW_OBJECTION_RESPONSE => Some((ResponseRecordKind::Response, false)),
        crate::TOOL_REOPEN_OBJECTION_RESPONSE => Some((ResponseRecordKind::Response, true)),
        _ => None,
    }
}

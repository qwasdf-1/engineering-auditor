use crate::{
    ChallengeTaskLedger, EngineeringCase, NativeToolCall, TaskCallProvenance, TaskLedgerError,
};
use serde_json::Value;

use super::{
    revision_arguments, string_field,
    task_application::{
        apply_native_task_calls, apply_typed_transition, record_native_value, LedgerTransition,
        TaskApplication, TransitionKind,
    },
    task_error, CaseEvidenceState,
};

pub(super) fn apply_challenge_task_calls(
    ledger: &mut ChallengeTaskLedger,
    calls: &[NativeToolCall],
    case: &EngineeringCase,
    state: &CaseEvidenceState,
    request_id: &str,
) -> TaskApplication {
    let mut application = TaskApplication::default();
    apply_native_task_calls(
        &mut application,
        calls,
        "The challenge task call was recorded.",
        |index, call| apply_task_call(ledger, call, index + 1 == calls.len(), request_id),
    );
    application.validate_evidence_requests(case, state);
    application
}

fn apply_task_call(
    ledger: &mut ChallengeTaskLedger,
    call: &NativeToolCall,
    is_last: bool,
    request_id: &str,
) -> Result<(), String> {
    if call.name == crate::TOOL_COMPLETE_CASE_CHALLENGE {
        return complete_challenge(ledger, is_last);
    }
    if let Some((kind, reopen)) = transition_kind(&call.name) {
        return apply_typed_transition(ledger, call, request_id, kind, reopen);
    }
    match call.name.as_str() {
        crate::TOOL_RECORD_CASE_OBJECTION => record_objection(ledger, call, request_id),
        crate::TOOL_RECORD_COMMON_CONCERN => record_native_value(
            ledger,
            call,
            request_id,
            "concern_ref",
            ChallengeTaskLedger::record_common_concern,
        ),
        crate::TOOL_REVISE_CASE_OBJECTION => {
            revise_record(ledger, call, request_id, ChallengeRecordKind::Objection)
        }
        crate::TOOL_REVISE_COMMON_CONCERN => {
            revise_record(ledger, call, request_id, ChallengeRecordKind::Concern)
        }
        other => Err(format!("unknown challenge task tool `{other}`")),
    }
}

fn complete_challenge(ledger: &mut ChallengeTaskLedger, is_last: bool) -> Result<(), String> {
    if !is_last {
        return Err("complete_case_challenge must be the last tool call".to_string());
    }
    ledger.complete().map_err(task_error)
}

fn record_objection(
    ledger: &mut ChallengeTaskLedger,
    call: &NativeToolCall,
    request_id: &str,
) -> Result<(), String> {
    let reference = string_field(&call.arguments, "objection_ref")?;
    let target_type = string_field(&call.arguments, "target_type")?;
    let target_id = string_field(&call.arguments, "target_id")?;
    ledger
        .record_objection(
            reference,
            target_type,
            target_id,
            call.arguments.clone(),
            provenance(request_id, call),
        )
        .map_err(task_error)
}

fn revise_record(
    ledger: &mut ChallengeTaskLedger,
    call: &NativeToolCall,
    request_id: &str,
    kind: ChallengeRecordKind,
) -> Result<(), String> {
    let revision = revision_arguments(&call.arguments, kind.reference_field())?;
    kind.revision()(
        ledger,
        &revision.reference,
        revision.expected_revision,
        &revision.reason,
        revision.replacement,
        provenance(request_id, call),
    )
    .map_err(task_error)
}

#[derive(Clone, Copy)]
enum ChallengeRecordKind {
    Objection,
    Concern,
}

type ChallengeRevision = for<'a> fn(
    &mut ChallengeTaskLedger,
    &str,
    u32,
    &str,
    Value,
    TaskCallProvenance<'a>,
) -> Result<(), TaskLedgerError>;

impl TransitionKind<ChallengeTaskLedger> for ChallengeRecordKind {
    fn reference_field(self) -> &'static str {
        match self {
            Self::Objection => "objection_ref",
            Self::Concern => "concern_ref",
        }
    }

    fn transition(self, reopen: bool) -> LedgerTransition<ChallengeTaskLedger> {
        match (self, reopen) {
            (Self::Objection, true) => ChallengeTaskLedger::reopen_objection,
            (Self::Objection, false) => ChallengeTaskLedger::withdraw_objection,
            (Self::Concern, true) => ChallengeTaskLedger::reopen_common_concern,
            (Self::Concern, false) => ChallengeTaskLedger::withdraw_common_concern,
        }
    }
}

impl ChallengeRecordKind {
    fn revision(self) -> ChallengeRevision {
        match self {
            Self::Objection => ChallengeTaskLedger::revise_objection,
            Self::Concern => ChallengeTaskLedger::revise_common_concern,
        }
    }
}

fn provenance<'a>(request_id: &'a str, call: &'a NativeToolCall) -> TaskCallProvenance<'a> {
    TaskCallProvenance::new(request_id, &call.call_id)
}

fn transition_kind(tool_name: &str) -> Option<(ChallengeRecordKind, bool)> {
    match tool_name {
        crate::TOOL_WITHDRAW_CASE_OBJECTION => Some((ChallengeRecordKind::Objection, false)),
        crate::TOOL_REOPEN_CASE_OBJECTION => Some((ChallengeRecordKind::Objection, true)),
        crate::TOOL_WITHDRAW_COMMON_CONCERN => Some((ChallengeRecordKind::Concern, false)),
        crate::TOOL_REOPEN_COMMON_CONCERN => Some((ChallengeRecordKind::Concern, true)),
        _ => None,
    }
}

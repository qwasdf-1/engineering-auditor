use std::collections::BTreeSet;

use crate::{
    CaseEvidenceRequest, CaseProviderTurn, CaseToolBoundary, EngineeringCase,
    NativeConversationMessage, NativeToolCall, TaskCallProvenance, TaskLedgerError,
};
use serde_json::Value;

use super::task_support::RevisionArguments;

use super::{
    probe_ids, revision_arguments, string_field, task_error, task_tool_result,
    transition_arguments, CaseEvidenceState,
};

#[derive(Default)]
pub(super) struct TaskApplication {
    pub(super) evidence_requests: Vec<CaseEvidenceRequest>,
    pub(super) evidence_calls: Vec<NativeToolCall>,
    pub(super) tool_results: Vec<NativeConversationMessage>,
    pub(super) errors: Vec<String>,
}

pub(super) fn apply_native_task_calls(
    application: &mut TaskApplication,
    calls: &[NativeToolCall],
    accepted_detail: &str,
    mut apply_task: impl FnMut(usize, &NativeToolCall) -> Result<(), String>,
) {
    let mixed_evidence = application.mixes_evidence_with_tasks(calls);
    for (index, call) in calls.iter().enumerate() {
        let result = if let Some(error) = &call.validation_error {
            Err(format!("arguments failed schema validation: {error}"))
        } else if mixed_evidence {
            Err("evidence requests must be the only task-call kind in their turn".to_string())
        } else if call.name == crate::TOOL_REQUEST_CASE_EVIDENCE {
            application.record_evidence_request(call)
        } else {
            apply_task(index, call)
        };
        application.record_result(call, result, accepted_detail);
    }
}

pub(super) type LedgerTransition<L> =
    for<'a> fn(&mut L, &str, u32, &str, TaskCallProvenance<'a>) -> Result<(), TaskLedgerError>;

pub(super) trait TransitionKind<L>: Copy {
    fn reference_field(self) -> &'static str;
    fn transition(self, reopen: bool) -> LedgerTransition<L>;
}

pub(super) fn apply_typed_transition<L, K: TransitionKind<L>>(
    ledger: &mut L,
    call: &NativeToolCall,
    request_id: &str,
    kind: K,
    reopen: bool,
) -> Result<(), String> {
    apply_transition(
        ledger,
        call,
        request_id,
        kind.reference_field(),
        kind.transition(reopen),
    )
}

pub(super) type LedgerRevision<L> = for<'a> fn(
    &mut L,
    &str,
    u32,
    &str,
    Value,
    TaskCallProvenance<'a>,
) -> Result<(), TaskLedgerError>;

/// A call's revision arguments; `replacement` checks or rebuilds the
/// replacement value.
pub(super) fn checked_revision(
    call: &NativeToolCall,
    reference_field: &str,
    replacement: impl FnOnce(Value) -> Result<Value, String>,
) -> Result<RevisionArguments, String> {
    let mut revision = revision_arguments(&call.arguments, reference_field)?;
    revision.replacement = replacement(revision.replacement)?;
    Ok(revision)
}

/// Applies a checked revision to the ledger.
pub(super) fn apply_revision<L>(
    ledger: &mut L,
    call: &NativeToolCall,
    request_id: &str,
    revision: RevisionArguments,
    revise: LedgerRevision<L>,
) -> Result<(), String> {
    revise(
        ledger,
        &revision.reference,
        revision.expected_revision,
        &revision.reason,
        revision.replacement,
        TaskCallProvenance::new(request_id, &call.call_id),
    )
    .map_err(task_error)
}

pub(super) fn record_native_value<L>(
    ledger: &mut L,
    call: &NativeToolCall,
    request_id: &str,
    reference_field: &str,
    record: for<'a> fn(&mut L, &str, Value, TaskCallProvenance<'a>) -> Result<(), TaskLedgerError>,
) -> Result<(), String> {
    let reference = string_field(&call.arguments, reference_field)?;
    record(
        ledger,
        reference,
        call.arguments.clone(),
        TaskCallProvenance::new(request_id, &call.call_id),
    )
    .map_err(task_error)
}

fn apply_transition<L>(
    ledger: &mut L,
    call: &NativeToolCall,
    request_id: &str,
    reference_field: &str,
    transition: for<'a> fn(
        &mut L,
        &str,
        u32,
        &str,
        TaskCallProvenance<'a>,
    ) -> Result<(), TaskLedgerError>,
) -> Result<(), String> {
    let arguments = transition_arguments(&call.arguments, reference_field)?;
    transition(
        ledger,
        &arguments.reference,
        arguments.expected_revision,
        &arguments.reason,
        TaskCallProvenance::new(request_id, &call.call_id),
    )
    .map_err(task_error)
}

impl TaskApplication {
    pub(super) fn mixes_evidence_with_tasks(&self, calls: &[NativeToolCall]) -> bool {
        let has_evidence = calls
            .iter()
            .any(|call| call.name == crate::TOOL_REQUEST_CASE_EVIDENCE);
        has_evidence
            && calls
                .iter()
                .any(|call| call.name != crate::TOOL_REQUEST_CASE_EVIDENCE)
    }

    pub(super) fn record_evidence_request(&mut self, call: &NativeToolCall) -> Result<(), String> {
        serde_json::from_value::<CaseEvidenceRequest>(call.arguments.clone())
            .map(|request| {
                self.evidence_requests.push(request);
                self.evidence_calls.push(call.clone());
            })
            .map_err(|error| format!("request_case_evidence arguments: {error}"))
    }

    pub(super) fn record_result(
        &mut self,
        call: &NativeToolCall,
        result: Result<(), String>,
        accepted_detail: &str,
    ) {
        match result {
            Ok(()) if call.name == crate::TOOL_REQUEST_CASE_EVIDENCE => {}
            Ok(()) => self
                .tool_results
                .push(task_tool_result(call, "Accepted", accepted_detail)),
            Err(error) => {
                self.errors
                    .push(format!("{}.{}: {error}", call.name, call.call_id));
                self.tool_results
                    .push(task_tool_result(call, "Rejected", &error));
            }
        }
    }

    pub(super) fn validate_evidence_requests(
        &mut self,
        case: &EngineeringCase,
        state: &CaseEvidenceState,
    ) {
        if self.evidence_requests.is_empty() {
            return;
        }
        let boundary = CaseToolBoundary {
            case_id: &case.case_id,
            probe_ids: &probe_ids(&state.definitions),
            target_ids: &target_ids(state),
        };
        let turn = CaseProviderTurn {
            evidence_requests: self.evidence_requests.clone(),
            case_review: None,
            challenge: None,
        };
        if let Err(error) = turn.validate(&boundary) {
            self.reject_evidence_requests(&error.to_string());
        }
    }

    fn reject_evidence_requests(&mut self, error: &str) {
        self.errors.push(error.to_string());
        self.tool_results.extend(
            self.evidence_calls
                .iter()
                .map(|call| task_tool_result(call, "Rejected", error)),
        );
        self.evidence_requests.clear();
        self.evidence_calls.clear();
    }
}

fn target_ids(state: &CaseEvidenceState) -> BTreeSet<String> {
    state
        .packet
        .allowed_neighborhood
        .map_symbol_ids
        .iter()
        .cloned()
        .chain(
            state
                .packet
                .allowed_neighborhood
                .finding_ids
                .iter()
                .map(|id| format!("finding:{id}")),
        )
        .collect()
}

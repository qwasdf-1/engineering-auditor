use super::*;

pub(super) fn reconstruct_recommendation_absence(arguments: &Value) -> Result<Value, String> {
    let mut reconstructed = arguments.clone();
    let object = reconstructed
        .as_object_mut()
        .ok_or_else(|| "recommendation arguments must be an object".to_string())?;
    object
        .entry("no_alternative_reason".to_string())
        .or_insert(Value::Null);
    Ok(reconstructed)
}

/// A recommendation replacement with its absent fields restored, checked
/// against the benefit-argument rules.
pub(super) fn recommendation_replacement(replacement: Value) -> Result<Value, String> {
    let replacement = reconstruct_recommendation_absence(&replacement)?;
    validate_recommendation_benefit_arguments(&replacement).map_err(|errors| errors.join("; "))?;
    Ok(replacement)
}

pub(super) fn enum_name<T: Serialize>(value: &T) -> Result<String, ReviewServiceError> {
    serde_json::to_value(value)
        .map_err(|error| failed(error.to_string()))?
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| failed("task identifier enum did not serialize as text"))
}

pub(super) fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{field} must be a non-empty string"))
}

pub(super) fn optional_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub(super) fn string_array_field(value: &Value, field: &str) -> Result<Vec<String>, String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{field} must be an array"))?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .filter(|entry| !entry.trim().is_empty())
                .map(ToString::to_string)
                .ok_or_else(|| format!("{field} entries must be non-empty strings"))
        })
        .collect()
}

pub(super) struct TransitionArguments {
    pub(super) reference: String,
    pub(super) expected_revision: u32,
    pub(super) reason: String,
}

pub(super) struct RevisionArguments {
    pub(super) reference: String,
    pub(super) expected_revision: u32,
    pub(super) reason: String,
    pub(super) replacement: Value,
}

pub(super) fn transition_arguments(
    value: &Value,
    reference_field: &str,
) -> Result<TransitionArguments, String> {
    let reference = string_field(value, reference_field)?.to_string();
    let expected_revision = value
        .get("expected_revision")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "expected_revision must be a positive integer".to_string())?;
    let reason = string_field(value, "reason")?.to_string();
    Ok(TransitionArguments {
        reference,
        expected_revision,
        reason,
    })
}

pub(super) fn revision_arguments(
    value: &Value,
    reference_field: &str,
) -> Result<RevisionArguments, String> {
    let transition = transition_arguments(value, reference_field)?;
    let replacement = value
        .get("replacement")
        .filter(|replacement| replacement.is_object())
        .cloned()
        .ok_or_else(|| "replacement must be a complete object".to_string())?;
    if replacement.get(reference_field).and_then(Value::as_str)
        != Some(transition.reference.as_str())
    {
        return Err(format!(
            "replacement.{reference_field} must match {reference_field}"
        ));
    }
    Ok(RevisionArguments {
        reference: transition.reference,
        expected_revision: transition.expected_revision,
        reason: transition.reason,
        replacement,
    })
}

pub(super) fn task_error(error: TaskLedgerError) -> String {
    error.to_string()
}

pub(super) fn record_reviewer_task_audit(log: &mut CallLog, ledger: &ReviewerTaskLedger) {
    log.claim_slot_dispositions
        .extend(ledger.claim_slot_disposition_records());
    log.recommendation_family_dispositions
        .extend(ledger.recommendation_family_disposition_records());
    log.semantic_revisions
        .extend_from_slice(ledger.probe_answers().revisions());
    log.semantic_revisions
        .extend_from_slice(ledger.claims().revisions());
    log.semantic_revisions
        .extend_from_slice(ledger.recommendations().revisions());
}

pub(super) fn record_challenge_task_audit(
    log: &mut CallLog,
    ledger: &ChallengeTaskLedger,
    reviewer_review_id: &StableId,
    challenger_run_id: &StableId,
) {
    log.challenge_target_dispositions
        .extend(ledger.disposition_records(reviewer_review_id, challenger_run_id));
    log.semantic_revisions
        .extend_from_slice(ledger.objections().revisions());
    log.semantic_revisions
        .extend_from_slice(ledger.common_concerns().revisions());
}

pub(super) fn record_response_task_audit(log: &mut CallLog, ledger: &ResponseTaskLedger) {
    log.semantic_revisions
        .extend_from_slice(ledger.responses().revisions());
    log.semantic_revisions
        .extend_from_slice(ledger.replacement_recommendations().revisions());
}

pub(super) fn task_tool_result(
    call: &NativeToolCall,
    heading: &str,
    detail: &str,
) -> NativeConversationMessage {
    NativeConversationMessage::ToolResult {
        call_id: call.call_id.clone(),
        tool_name: call.name.clone(),
        content_markdown: format!("## {heading}\n\n{detail}"),
    }
}

pub(super) fn render_task_correction(title: &str, errors: &[String]) -> String {
    let mut markdown = format!("# {title}\n\n");
    for error in errors {
        markdown.push_str("- ");
        markdown.push_str(error);
        markdown.push('\n');
    }
    markdown.push_str(
        "\nUse the declared native tools. Create calls cannot revise existing IDs; use a revision tool when it is offered. Finish with the completion tool.",
    );
    markdown
}

pub(super) fn mark_last_call_host_rejected(
    log: &mut CallLog,
    transcript: &TranscriptRecorder,
    validation_error: &str,
    discarded: bool,
) {
    if let Some(record) = log.calls.last_mut() {
        transcript.mark_host_rejected(&record.call.request_id, validation_error, discarded);
        record.failure_class = Some(crate::ProviderFailureClass::InvalidOutput);
        record.failure_detail = Some(validation_error.to_string());
        record.call.failure_class = Some(crate::ProviderFailureClass::InvalidOutput);
        record.call.discarded = discarded;
    }
}

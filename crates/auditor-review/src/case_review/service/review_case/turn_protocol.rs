use super::*;

pub(super) fn record_native_assistant_turn(
    value: Value,
    conversation: &mut Vec<NativeConversationMessage>,
    stage: &str,
) -> Result<NativeToolTurn, ReviewServiceError> {
    let turn = serde_json::from_value::<NativeToolTurn>(value)
        .map_err(|error| failed(format!("invalid native {stage} turn: {error}")))?;
    conversation.push(NativeConversationMessage::Assistant {
        text: turn.text.clone(),
        tool_calls: turn.tool_calls.clone(),
    });
    Ok(turn)
}

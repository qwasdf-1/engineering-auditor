use serde_json::Value;

use crate::{NativeConversationMessage, NativeToolCall, ToolSpec};

pub(super) enum NativeOutputRepair {
    Replay(Vec<NativeConversationMessage>),
    FreshCorrection(NativeConversationMessage),
}

pub(super) fn native_output_repair(
    assistant: NativeConversationMessage,
    error: &str,
    tools: Option<&[ToolSpec]>,
) -> NativeOutputRepair {
    let malformed_calls = malformed_native_calls(&assistant);
    if malformed_calls.is_empty() {
        return NativeOutputRepair::Replay(replay_messages(assistant, error));
    }
    let markdown = malformed_correction(&malformed_calls, error, tools);
    NativeOutputRepair::FreshCorrection(NativeConversationMessage::UserCorrection {
        content_markdown: markdown,
    })
}

fn malformed_native_calls(assistant: &NativeConversationMessage) -> Vec<(&NativeToolCall, String)> {
    match assistant {
        NativeConversationMessage::Assistant { tool_calls, .. } => tool_calls
            .iter()
            .filter_map(|call| {
                serde_json::from_str::<Value>(&call.raw_arguments)
                    .err()
                    .map(|error| (call, error.to_string()))
            })
            .collect(),
        NativeConversationMessage::ToolResult { .. }
        | NativeConversationMessage::UserCorrection { .. }
        | NativeConversationMessage::UserContinuation { .. } => Vec::new(),
    }
}

fn replay_messages(
    assistant: NativeConversationMessage,
    error: &str,
) -> Vec<NativeConversationMessage> {
    let rejected_calls = rejected_tool_results(&assistant);
    let mut messages = Vec::with_capacity(rejected_calls.len() + 2);
    messages.push(assistant);
    messages.extend(rejected_calls);
    messages.push(NativeConversationMessage::UserCorrection {
        content_markdown: general_correction(error),
    });
    messages
}

fn rejected_tool_results(assistant: &NativeConversationMessage) -> Vec<NativeConversationMessage> {
    match assistant {
        NativeConversationMessage::Assistant { tool_calls, .. } => tool_calls
            .iter()
            .map(|call| NativeConversationMessage::ToolResult {
                call_id: call.call_id.clone(),
                tool_name: call.name.clone(),
                content_markdown: "## Rejected\n\nThis assistant turn failed native task-call schema validation. The call was not recorded; follow the correction in the next user message."
                    .to_string(),
            })
            .collect(),
        NativeConversationMessage::ToolResult { .. }
        | NativeConversationMessage::UserCorrection { .. }
        | NativeConversationMessage::UserContinuation { .. } => Vec::new(),
    }
}

fn general_correction(error: &str) -> String {
    let mut markdown = String::from(
        "# Native tool-call correction\n\nThe previous native task-call turn was not admitted. No call from that assistant turn was recorded.\n\n## Error\n\n",
    );
    append_error_list(&mut markdown, error);
    markdown.push_str(
        "\nUse only the declared native tools. Do not return a JSON document or a prose-only final answer.",
    );
    markdown
}

fn malformed_correction(
    malformed_calls: &[(&NativeToolCall, String)],
    error: &str,
    tools: Option<&[ToolSpec]>,
) -> String {
    let mut markdown = String::from(
        "# Malformed native tool-call correction\n\n\
         Repair mode: `malformed-native-arguments`. The previous assistant turn was not admitted, and no call from that turn was recorded. Its exact response remains in the audit transcript, but syntactically invalid native arguments cannot be replayed to the provider.\n\n\
         ## Calls requiring correction\n",
    );
    for (call, parse_error) in malformed_calls {
        append_call_error(&mut markdown, call, parse_error, tools);
    }
    markdown.push_str("\n## Whole-turn resubmission\n\nResubmit every intended call from the rejected assistant turn, including calls whose individual arguments were valid. Use only the declared native tools and the evidence IDs in the original case context. Do not return a JSON document or a prose-only final answer.\n\n## Host validation error\n");
    append_error_list(&mut markdown, error);
    markdown
}

fn append_call_error(
    markdown: &mut String,
    call: &NativeToolCall,
    parse_error: &str,
    tools: Option<&[ToolSpec]>,
) {
    markdown.push_str("\n### `");
    markdown.push_str(&call.name);
    markdown.push_str("` — call `");
    markdown.push_str(&call.call_id);
    markdown.push_str("`\n\n- Argument encoding error: ");
    markdown.push_str(parse_error);
    markdown.push('\n');
    if let Some(tool) = tools.and_then(|items| items.iter().find(|item| item.name == call.name)) {
        append_required_fields(markdown, &tool.parameters);
        append_allowed_values(markdown, &tool.parameters);
    }
}

fn append_required_fields(markdown: &mut String, parameters: &Value) {
    let Some(required) = parameters.get("required").and_then(Value::as_array) else {
        return;
    };
    let fields = required
        .iter()
        .filter_map(Value::as_str)
        .map(|field| format!("`{field}`"))
        .collect::<Vec<_>>();
    if !fields.is_empty() {
        markdown.push_str("- Required argument fields: ");
        markdown.push_str(&fields.join(", "));
        markdown.push('\n');
    }
}

fn append_allowed_values(markdown: &mut String, parameters: &Value) {
    let Some(properties) = parameters.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (field, property) in properties {
        let values = property
            .get("enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|value| format!("`{value}`"))
            .collect::<Vec<_>>();
        if !values.is_empty() {
            markdown.push_str("- Allowed values for `");
            markdown.push_str(field);
            markdown.push_str("`: ");
            markdown.push_str(&values.join(", "));
            markdown.push('\n');
        }
    }
}

fn append_error_list(markdown: &mut String, error: &str) {
    for line in error.lines() {
        markdown.push_str("- ");
        markdown.push_str(line);
        markdown.push('\n');
    }
}

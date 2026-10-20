use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ProviderProtocol;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub raw_arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeToolTurn {
    pub text: Option<String>,
    pub tool_calls: Vec<NativeToolCall>,
}

impl NativeToolCall {
    pub fn new(
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
        raw_arguments: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            name: name.into(),
            arguments,
            raw_arguments: raw_arguments.into(),
            validation_error: None,
        }
    }

    pub(crate) fn with_validation_error(mut self, error: Option<String>) -> Self {
        self.validation_error = error;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum NativeConversationMessage {
    Assistant {
        text: Option<String>,
        tool_calls: Vec<NativeToolCall>,
    },
    ToolResult {
        call_id: String,
        tool_name: String,
        content_markdown: String,
    },
    UserCorrection {
        content_markdown: String,
    },
    UserContinuation {
        content_markdown: String,
    },
}

pub fn render_native_conversation(
    protocol: ProviderProtocol,
    system_instruction: &str,
    initial_user_markdown: &str,
    history: &[NativeConversationMessage],
) -> Value {
    match protocol {
        ProviderProtocol::ChatCompletions => Value::Array(
            [
                initial_system_user_messages(system_instruction, initial_user_markdown),
                history.iter().map(render_chat_message).collect(),
            ]
            .concat(),
        ),
        ProviderProtocol::Messages => Value::Array(
            [
                vec![json!({"role": "user", "content": initial_user_markdown})],
                render_messages_history(history),
            ]
            .concat(),
        ),
        ProviderProtocol::Responses => Value::Array(
            [
                initial_system_user_messages(system_instruction, initial_user_markdown),
                history.iter().flat_map(render_responses_messages).collect(),
            ]
            .concat(),
        ),
    }
}

fn initial_system_user_messages(system_instruction: &str, user_markdown: &str) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": system_instruction}),
        json!({"role": "user", "content": user_markdown}),
    ]
}

pub(crate) fn parse_native_assistant_output(
    protocol: ProviderProtocol,
    raw: &str,
) -> Option<NativeConversationMessage> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let (text, tool_calls) = match protocol {
        ProviderProtocol::ChatCompletions => (
            value
                .get("content")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            value
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let raw_arguments = entry["function"]["arguments"].as_str()?.to_string();
                    Some(NativeToolCall::new(
                        entry["id"].as_str()?,
                        entry["function"]["name"].as_str()?,
                        serde_json::from_str(&raw_arguments).unwrap_or(Value::Null),
                        raw_arguments,
                    ))
                })
                .collect(),
        ),
        ProviderProtocol::Messages => {
            let blocks = value.as_array()?;
            let text = blocks
                .iter()
                .filter(|block| block["type"].as_str() == Some("text"))
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            let tool_calls = blocks
                .iter()
                .filter(|block| block["type"].as_str() == Some("tool_use"))
                .filter_map(|block| {
                    let arguments = block["input"].clone();
                    Some(NativeToolCall::new(
                        block["id"].as_str()?,
                        block["name"].as_str()?,
                        arguments.clone(),
                        serde_json::to_string(&arguments).ok()?,
                    ))
                })
                .collect();
            ((!text.is_empty()).then_some(text), tool_calls)
        }
        ProviderProtocol::Responses => {
            let entries = value.as_array()?;
            let text = entries
                .iter()
                .filter_map(|entry| {
                    (entry["type"].as_str() == Some("message"))
                        .then(|| entry["content"].as_array())
                        .flatten()
                })
                .flatten()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            let tool_calls = entries
                .iter()
                .filter(|entry| {
                    matches!(entry["type"].as_str(), Some("tool_call" | "function_call"))
                })
                .filter_map(|entry| {
                    let raw_arguments = entry["arguments"].as_str()?.to_string();
                    Some(NativeToolCall::new(
                        entry["call_id"].as_str()?,
                        entry["name"].as_str()?,
                        serde_json::from_str(&raw_arguments).unwrap_or(Value::Null),
                        raw_arguments,
                    ))
                })
                .collect();
            ((!text.is_empty()).then_some(text), tool_calls)
        }
    };
    Some(NativeConversationMessage::Assistant { text, tool_calls })
}

fn render_chat_message(message: &NativeConversationMessage) -> Value {
    match message {
        NativeConversationMessage::Assistant { text, tool_calls } => json!({
            "role": "assistant",
            "content": text,
            "tool_calls": tool_calls.iter().map(|call| json!({
                "id": call.call_id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": call.raw_arguments,
                }
            })).collect::<Vec<_>>()
        }),
        NativeConversationMessage::ToolResult {
            call_id,
            content_markdown,
            ..
        } => json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content_markdown,
        }),
        NativeConversationMessage::UserCorrection { content_markdown }
        | NativeConversationMessage::UserContinuation { content_markdown } => {
            json!({"role": "user", "content": content_markdown})
        }
    }
}

fn render_messages_history(history: &[NativeConversationMessage]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut pending_user_content = Vec::new();
    let flush_user = |messages: &mut Vec<Value>, pending: &mut Vec<Value>| {
        if !pending.is_empty() {
            messages.push(json!({
                "role": "user",
                "content": std::mem::take(pending),
            }));
        }
    };
    for message in history {
        match message {
            NativeConversationMessage::Assistant { text, tool_calls } => {
                flush_user(&mut messages, &mut pending_user_content);
                let content = render_messages_assistant_content(text.as_deref(), tool_calls);
                messages.push(json!({"role": "assistant", "content": content}));
            }
            NativeConversationMessage::ToolResult {
                call_id,
                content_markdown,
                ..
            } => pending_user_content.push(json!({
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": content_markdown,
            })),
            NativeConversationMessage::UserCorrection { content_markdown }
            | NativeConversationMessage::UserContinuation { content_markdown } => {
                pending_user_content.push(json!({
                    "type": "text",
                    "text": content_markdown,
                }));
            }
        }
    }
    flush_user(&mut messages, &mut pending_user_content);
    messages
}

fn render_messages_assistant_content(
    text: Option<&str>,
    tool_calls: &[NativeToolCall],
) -> Vec<Value> {
    let mut content = text
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({"type": "text", "text": text})])
        .unwrap_or_default();
    content.extend(tool_calls.iter().map(|call| {
        json!({
            "type": "tool_use",
            "id": call.call_id,
            "name": call.name,
            "input": call.arguments,
        })
    }));
    content
}

fn render_responses_messages(message: &NativeConversationMessage) -> Vec<Value> {
    match message {
        NativeConversationMessage::Assistant { text, tool_calls } => {
            let mut entries = Vec::new();
            if let Some(text) = text.as_ref().filter(|text| !text.is_empty()) {
                entries.push(json!({"role": "assistant", "content": text}));
            }
            entries.extend(tool_calls.iter().map(|call| {
                json!({
                    "type": "function_call",
                    "call_id": call.call_id,
                    "name": call.name,
                    "arguments": call.raw_arguments,
                })
            }));
            entries
        }
        NativeConversationMessage::ToolResult {
            call_id,
            content_markdown,
            ..
        } => vec![json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": content_markdown,
        })],
        NativeConversationMessage::UserCorrection { content_markdown }
        | NativeConversationMessage::UserContinuation { content_markdown } => {
            vec![json!({"role": "user", "content": content_markdown})]
        }
    }
}

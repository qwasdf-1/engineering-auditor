//! Provider-native round-trip repair: when a stage fails to parse or validate,
//! the engine may issue one follow-up request that includes the model's prior
//! broken output and the validation error. The adapter renders that turn in
//! the protocol-native shape (assistant message + user feedback), and the
//! model is asked to re-emit a valid output.
//!
//! Bounds:
//!
//! - The repair call is bounded at **one** per stage. A second failure
//!   propagates as `ReviewError::InvalidProviderOutput` and (for the
//!   sharded assessment stage) the shard is recorded as `partial_assessment`.
//! - The repair call shares the original `RouteBudget`, including the same
//!   per-request output allowance as the initial call. Earlier output never
//!   shrinks a repair call's response allowance.
//! - The repair call's `request_id` carries the `:repair-1` suffix so the
//!   transcript sidecar can sequence the two calls per stage.

use serde_json::{json, Value};

use super::native_tool_adapter::{render_tool_request_body, ToolRequestBody};
use super::types::ProviderProtocol;

/// Prior turn + validation context the engine passes to the adapter when it
/// needs to ask the model to re-emit a broken stage output.
#[derive(Clone, Debug)]
pub struct RepairContext {
    /// Stage name (matches `ProviderRequest.stage`).
    pub stage: String,
    /// Complete contract-bearing assistant output from the failing call. Tool
    /// calls and ordinary output text are preserved; protocol-native reasoning
    /// blocks and response-envelope metadata remain in the transcript but are
    /// excluded from the repair prompt because they duplicate no contract
    /// input and cannot satisfy the output schema.
    pub prior_assistant_output: String,
    /// Validation error message surfaced back to the model (e.g. the
    /// `validate_schema` or `parse_output` failure text).
    pub validation_error: String,
    /// Schema used by the host to validate the replacement native tool call.
    /// It is not serialized into a model-authored JSON response request.
    pub schema: Value,
}

impl RepairContext {
    /// Render the user-turn repair message that comes after the model's
    /// broken assistant output. The text is identical across protocols; the
    /// adapter inserts it as the trailing user message in the rendered body.
    pub fn render_repair_user_message(&self) -> String {
        format!(
            "Your previous response for stage '{stage}' did not satisfy the required schema.\n\
             Validation error: {validation_error}\n\
             Re-emit the complete stage output conforming to the declared schema. Do not include commentary.",
            stage = self.stage,
            validation_error = self.validation_error,
        )
    }
}

/// Render a repair request that preserves the original provider-native tool
/// message and keeps the declared tool vocabulary active. The prior assistant
/// value is stored as compact JSON by the adapter; malformed/non-tool values
/// safely fall back to an ordinary assistant text turn.
pub fn render_tool_repair_request_body(
    protocol: ProviderProtocol,
    body: ToolRequestBody<'_>,
    repair: &RepairContext,
) -> Option<Value> {
    let mut body = render_tool_request_body(protocol, body)?;
    let repair_message = format!(
        "{} Use only the declared tools for the replacement response.",
        repair.render_repair_user_message()
    );
    match protocol {
        ProviderProtocol::ChatCompletions => {
            let messages = body.get_mut("messages")?.as_array_mut()?;
            messages.extend(chat_tool_repair_messages(&repair.prior_assistant_output));
            messages.push(json!({"role": "user", "content": repair_message}));
        }
        ProviderProtocol::Messages => {
            let messages = body.get_mut("messages")?.as_array_mut()?;
            let assistant = messages_assistant_message(&repair.prior_assistant_output);
            let mut result_content = messages_tool_repair_results(&assistant);
            messages.push(assistant);
            if result_content.is_empty() {
                messages.push(json!({"role": "user", "content": repair_message}));
            } else {
                result_content.push(json!({"type": "text", "text": repair_message}));
                messages.push(json!({"role": "user", "content": result_content}));
            }
        }
        ProviderProtocol::Responses => {
            let input = body.get_mut("input")?.as_array_mut()?;
            let entries = match serde_json::from_str::<Value>(&repair.prior_assistant_output) {
                Ok(Value::Array(entries)) => entries,
                Ok(entry @ Value::Object(_)) => vec![entry],
                _ => vec![json!({
                    "role": "assistant",
                    "content": repair.prior_assistant_output
                })],
            };
            let results = responses_tool_repair_results(&entries);
            input.extend(entries);
            input.extend(results);
            input.push(json!({"role": "user", "content": repair_message}));
        }
    }
    Some(body)
}

const HOST_REJECTED_TOOL_RESULT: &str =
    "## Host rejection\n\nThis tool call was not admitted. Follow the correction in the next user message.";

fn chat_tool_repair_messages(prior: &str) -> Vec<Value> {
    let assistant = chat_assistant_message(prior);
    let results = assistant["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|call| call.get("id").and_then(Value::as_str))
        .map(|call_id| {
            json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": HOST_REJECTED_TOOL_RESULT
            })
        })
        .collect::<Vec<_>>();
    std::iter::once(assistant).chain(results).collect()
}

fn chat_assistant_message(prior: &str) -> Value {
    match serde_json::from_str::<Value>(prior) {
        Ok(Value::Object(mut message)) => {
            message.insert("role".to_string(), json!("assistant"));
            Value::Object(message)
        }
        _ => json!({"role": "assistant", "content": prior}),
    }
}

fn messages_tool_repair_results(assistant: &Value) -> Vec<Value> {
    assistant["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "tool_use")
        .filter_map(|block| block.get("id").and_then(Value::as_str))
        .map(|tool_use_id| {
            json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": HOST_REJECTED_TOOL_RESULT,
                "is_error": true
            })
        })
        .collect()
}

fn messages_assistant_message(prior: &str) -> Value {
    match serde_json::from_str::<Value>(prior) {
        Ok(content @ Value::Array(_)) => json!({"role": "assistant", "content": content}),
        _ => json!({"role": "assistant", "content": prior}),
    }
}

fn responses_tool_repair_results(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .filter(|entry| entry["type"] == "function_call")
        .filter_map(|entry| entry.get("call_id").and_then(Value::as_str))
        .map(|call_id| {
            json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": HOST_REJECTED_TOOL_RESULT
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_repair() -> RepairContext {
        RepairContext {
            stage: "atomic_assessment".to_string(),
            prior_assistant_output: "{\"assessments\":[{".to_string(),
            validation_error: "expected value at line 1 column 17".to_string(),
            schema: json!({"type": "object", "required": ["assessments"]}),
        }
    }

    #[test]
    fn render_repair_user_message_references_native_schema_without_duplication() {
        let repair = sample_repair();
        let message = repair.render_repair_user_message();
        assert!(message.contains("stage 'atomic_assessment'"));
        assert!(message.contains("expected value at line 1 column 17"));
        assert!(message.contains("Re-emit the complete stage output"));
        assert!(message.contains("declared schema"));
        assert!(!message.contains("\"required\""));
        assert!(!message.contains("```json"));
    }

    #[test]
    fn native_tool_repairs_close_every_protocol_call_before_correction() {
        let chat = chat_tool_repair_messages(
            r#"{"content":null,"tool_calls":[{"id":"chat-call","type":"function","function":{"name":"answer","arguments":"{}"}}]}"#,
        );
        assert_eq!(chat[0]["role"], "assistant");
        assert_eq!(chat[1]["role"], "tool");
        assert_eq!(chat[1]["tool_call_id"], "chat-call");

        let messages_assistant = json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "messages-call",
                "name": "answer",
                "input": {}
            }]
        });
        let messages = messages_tool_repair_results(&messages_assistant);
        assert_eq!(messages[0]["type"], "tool_result");
        assert_eq!(messages[0]["tool_use_id"], "messages-call");
        assert_eq!(messages[0]["is_error"], true);

        let responses = responses_tool_repair_results(&[json!({
            "type": "function_call",
            "call_id": "responses-call",
            "name": "answer",
            "arguments": "{}"
        })]);
        assert_eq!(responses[0]["type"], "function_call_output");
        assert_eq!(responses[0]["call_id"], "responses-call");
    }
}

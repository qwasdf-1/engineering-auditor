//! Provider-native tool request rendering and response parsing.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::native_tool::{translate, ToolChoice, ToolSpec, TOOL_NOOP};
use super::types::ProviderProtocol;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AssessmentMode {
    #[default]
    #[serde(rename = "provider_native_tools_v2")]
    NativeTools,
}

impl AssessmentMode {
    pub const fn as_str(self) -> &'static str {
        "provider_native_tools_v2"
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AssessmentModeRecorded {
    #[default]
    #[serde(rename = "provider_native_tools_v2")]
    NativeTools,
}

impl AssessmentModeRecorded {
    pub const fn as_str(self) -> &'static str {
        "provider_native_tools_v2"
    }

    pub const fn from_mode(_: AssessmentMode) -> Self {
        Self::NativeTools
    }
}

#[derive(Clone, Debug)]
pub struct ParsedToolCall {
    pub tool_name: String,
    pub tool_call_id: String,
    pub arguments: Value,
    pub raw_arguments: String,
}

#[derive(Clone, Debug)]
pub struct ParsedToolOutput {
    pub calls: Vec<ParsedToolCall>,
    pub stop_reason: ToolStopReason,
    pub raw_text: String,
}

impl ParsedToolOutput {
    fn unparseable(raw_text: String) -> Self {
        Self {
            calls: Vec::new(),
            stop_reason: ToolStopReason::Unparseable,
            raw_text,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolStopReason {
    NoOp,
    ImplicitStop,
    Truncated,
    Unparseable,
}

pub struct ToolRequestBody<'a> {
    model: &'a str,
    request: &'a crate::provider::types::ProviderRequest,
    user_content: &'a str,
    specs: &'a [ToolSpec],
    choice: ToolChoice,
    history: &'a [super::NativeConversationMessage],
}

impl<'a> ToolRequestBody<'a> {
    pub fn new(
        model: &'a str,
        request: &'a crate::provider::types::ProviderRequest,
        user_content: &'a str,
    ) -> Self {
        Self {
            model,
            request,
            user_content,
            specs: &[],
            choice: ToolChoice::Auto,
            history: &[],
        }
    }

    pub fn with_tools(mut self, specs: &'a [ToolSpec], choice: ToolChoice) -> Self {
        self.specs = specs;
        self.choice = choice;
        self
    }

    pub fn with_history(mut self, history: &'a [super::NativeConversationMessage]) -> Self {
        self.history = history;
        self
    }
}

pub fn render_tool_request_body(
    protocol: ProviderProtocol,
    body: ToolRequestBody<'_>,
) -> Option<Value> {
    let declaration = translate(body.specs, body.choice, protocol)?;
    match protocol {
        ProviderProtocol::ChatCompletions => Some(json!({
            "max_tokens": body.request.max_output_tokens,
            "messages": super::render_native_conversation(
                protocol,
                &body.request.system_instruction,
                body.user_content,
                body.history,
            ),
            "model": body.model,
            "tools": declaration.tool_block,
            "tool_choice": declaration.choice_block
        })),
        ProviderProtocol::Messages => Some(json!({
            "max_tokens": body.request.max_output_tokens,
            "messages": super::render_native_conversation(
                protocol,
                &body.request.system_instruction,
                body.user_content,
                body.history,
            ),
            "model": body.model,
            "system": body.request.system_instruction,
            "tools": declaration.tool_block,
            "tool_choice": declaration.choice_block
        })),
        ProviderProtocol::Responses => Some(json!({
            "input": super::render_native_conversation(
                protocol,
                &body.request.system_instruction,
                body.user_content,
                body.history,
            ),
            "max_output_tokens": body.request.max_output_tokens,
            "model": body.model,
            "store": false,
            "tools": declaration.tool_block,
            "tool_choice": declaration.choice_block
        })),
    }
}

pub fn parse_tool_response(protocol: ProviderProtocol, value: &Value) -> ParsedToolOutput {
    match protocol {
        ProviderProtocol::ChatCompletions => parse_chat_completions(value),
        ProviderProtocol::Messages => parse_messages(value),
        ProviderProtocol::Responses => parse_responses(value),
    }
}

fn parse_chat_completions(value: &Value) -> ParsedToolOutput {
    let finish_reason = value["choices"][0]["finish_reason"]
        .as_str()
        .or_else(|| value["finish_reason"].as_str())
        .unwrap_or("stop");
    let truncated = matches!(finish_reason, "length" | "max_tokens" | "incomplete");
    let raw_text = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let Some(tool_calls) = value["choices"][0]["message"]["tool_calls"].as_array() else {
        let mut output = ParsedToolOutput::unparseable(raw_text);
        if truncated {
            output.stop_reason = ToolStopReason::Truncated;
        }
        return output;
    };
    let mut calls = Vec::new();
    let mut explicit_noop = false;
    for entry in tool_calls {
        let raw_arguments = entry["function"]["arguments"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let name = entry["function"]["name"].as_str().unwrap_or("").to_string();
        calls.push(parsed_tool_call(
            entry["id"].as_str().unwrap_or("").to_string(),
            name.clone(),
            serde_json::from_str(&raw_arguments).unwrap_or(Value::Null),
            raw_arguments,
        ));
        if name == TOOL_NOOP {
            explicit_noop = true;
            break;
        }
    }
    ParsedToolOutput {
        stop_reason: if explicit_noop {
            ToolStopReason::NoOp
        } else if truncated {
            ToolStopReason::Truncated
        } else if calls.is_empty() {
            ToolStopReason::Unparseable
        } else {
            ToolStopReason::ImplicitStop
        },
        calls,
        raw_text,
    }
}

fn parse_messages(value: &Value) -> ParsedToolOutput {
    let Some(content) = value["content"].as_array() else {
        return ParsedToolOutput::unparseable(String::new());
    };
    let mut calls = Vec::new();
    let mut raw_text = String::new();
    for block in content {
        if block["type"].as_str() == Some("text") {
            raw_text.push_str(block["text"].as_str().unwrap_or(""));
        }
        if block["type"].as_str() != Some("tool_use") {
            continue;
        }
        let arguments = block["input"].clone();
        let name = block["name"].as_str().unwrap_or("").to_string();
        calls.push(parsed_tool_call(
            block["id"].as_str().unwrap_or("").to_string(),
            name.clone(),
            arguments.clone(),
            serde_json::to_string(&arguments).unwrap_or_default(),
        ));
        if name == TOOL_NOOP {
            return ParsedToolOutput {
                calls,
                stop_reason: ToolStopReason::NoOp,
                raw_text,
            };
        }
    }
    let stop_reason = if value["stop_reason"].as_str() == Some("max_tokens") {
        ToolStopReason::Truncated
    } else if calls.is_empty() {
        ToolStopReason::Unparseable
    } else {
        ToolStopReason::ImplicitStop
    };
    ParsedToolOutput {
        calls,
        stop_reason,
        raw_text,
    }
}

fn parse_responses(value: &Value) -> ParsedToolOutput {
    let Some(entries) = value["output"].as_array() else {
        return ParsedToolOutput::unparseable(String::new());
    };
    let mut calls = Vec::new();
    let mut raw_text = String::new();
    for entry in entries {
        if let Some(text) = response_entry_text(entry) {
            raw_text.push_str(text);
        }
        let Some(call) = response_tool_call(entry) else {
            continue;
        };
        let noop = call.tool_name == TOOL_NOOP;
        calls.push(call);
        if noop {
            return ParsedToolOutput {
                calls,
                stop_reason: ToolStopReason::NoOp,
                raw_text,
            };
        }
    }
    let stop_reason = if calls.is_empty() {
        ToolStopReason::Unparseable
    } else if matches!(value["status"].as_str(), Some("incomplete" | "failed")) {
        ToolStopReason::Truncated
    } else {
        ToolStopReason::ImplicitStop
    };
    ParsedToolOutput {
        calls,
        stop_reason,
        raw_text,
    }
}

fn parsed_tool_call(
    tool_call_id: String,
    tool_name: String,
    arguments: Value,
    raw_arguments: String,
) -> ParsedToolCall {
    ParsedToolCall {
        tool_name,
        tool_call_id,
        arguments,
        raw_arguments,
    }
}

fn response_entry_text(entry: &Value) -> Option<&str> {
    (entry["type"].as_str() == Some("message"))
        .then(|| entry["content"].as_array())
        .flatten()?
        .iter()
        .find(|block| block["type"].as_str() == Some("output_text"))?["text"]
        .as_str()
}

fn response_tool_call(entry: &Value) -> Option<ParsedToolCall> {
    matches!(entry["type"].as_str(), Some("tool_call" | "function_call")).then(|| {
        let raw_arguments = entry["arguments"].as_str().unwrap_or("").to_string();
        parsed_tool_call(
            entry["call_id"].as_str().unwrap_or("").to_string(),
            entry["name"].as_str().unwrap_or("").to_string(),
            serde_json::from_str(&raw_arguments).unwrap_or(Value::Null),
            raw_arguments,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_native_arguments_remain_a_structured_invalid_call() {
        let value = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {"tool_calls": [{
                    "id": "call-1",
                    "function": {"name": "answer_case_probe", "arguments": "{broken"}
                }]}
            }]
        });
        let parsed = parse_tool_response(ProviderProtocol::ChatCompletions, &value);
        assert_eq!(parsed.calls.len(), 1);
        assert!(parsed.calls[0].arguments.is_null());
        assert_eq!(parsed.calls[0].raw_arguments, "{broken");
    }
}

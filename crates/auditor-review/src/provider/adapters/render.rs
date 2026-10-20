use auditor_core::sha256_digest_bytes;
use serde_json::Value;

use super::{CallDispatch, ProviderProtocol, ProviderRequest};
use crate::provider::native_tool_adapter::{render_tool_request_body, ToolRequestBody};
use crate::provider::repair::render_tool_repair_request_body;

pub(super) fn render_dispatch_body(
    protocol: ProviderProtocol,
    model: &str,
    request: &ProviderRequest,
    user_content: &str,
    dispatch: &CallDispatch<'_>,
) -> Value {
    let tool_body = || {
        ToolRequestBody::new(model, request, user_content)
            .with_tools(&dispatch.tools, dispatch.tool_choice)
            .with_history(&dispatch.conversation)
    };
    if let Some(repair) = dispatch.repair {
        render_tool_repair_request_body(protocol, tool_body(), repair)
            .expect("native tool repair rendering requires a non-empty tool declaration")
    } else {
        render_tool_request_body(protocol, tool_body())
            .expect("native tool rendering requires a non-empty tool declaration")
    }
}

/// Mistral prefix caching: requests that share a `prompt_cache_key` are routed
/// to a cache holding their common prompt prefix (cached tokens bill at 10% of
/// input). A case conversation keeps its system instruction and initial user
/// context fixed and only appends turns, so a digest of that fixed opening is a
/// stable per-conversation key that carries no identifiers or secrets.
pub(super) fn apply_prompt_cache_key(
    kind: super::ProviderKind,
    body: &mut Value,
    request: &ProviderRequest,
    user_content: &str,
) {
    if kind != super::ProviderKind::MistralChat {
        return;
    }
    let opening = format!("{}\0{user_content}", request.system_instruction);
    let digest = sha256_digest_bytes(opening.as_bytes());
    let digest = digest.trim_start_matches("sha256:");
    body["prompt_cache_key"] = Value::String(format!("ea-{}", &digest[..32]));
}

/// Schema keywords a provider's strict tool mode rejects, removed from the
/// declared tool parameters on the wire. Host admission still validates every
/// call against the complete schema, so lowering never widens what is
/// admitted. Mistral's strict mode rejects `uniqueItems` (HTTP 400, "Invalid
/// structured output syntax").
pub(super) fn lower_tool_schemas(kind: super::ProviderKind, body: &mut Value) {
    let unsupported: &[&str] = match kind {
        super::ProviderKind::MistralChat => &["uniqueItems"],
        _ => return,
    };
    for tool in body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(parameters) = tool.pointer_mut("/function/parameters") {
            strip_keywords(parameters, unsupported);
        }
    }
}

fn strip_keywords(schema: &mut Value, keywords: &[&str]) {
    match schema {
        Value::Object(object) => {
            object.retain(|key, _| !keywords.contains(&key.as_str()));
            for value in object.values_mut() {
                strip_keywords(value, keywords);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_keywords(item, keywords);
            }
        }
        _ => {}
    }
}

pub(super) fn tool_assistant_output(protocol: ProviderProtocol, value: &Value) -> String {
    let assistant = match protocol {
        ProviderProtocol::ChatCompletions => {
            let mut message = value["choices"][0]["message"].clone();
            if let Some(object) = message.as_object_mut() {
                object.retain(|field, _| !field.starts_with("reasoning"));
            }
            message
        }
        ProviderProtocol::Messages => {
            let mut content = value["content"].as_array().cloned().unwrap_or_default();
            content.retain(|block| {
                !matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                )
            });
            Value::Array(content)
        }
        ProviderProtocol::Responses => {
            let mut output = value["output"].as_array().cloned().unwrap_or_default();
            output.retain(|entry| entry.get("type").and_then(Value::as_str) != Some("reasoning"));
            Value::Array(output)
        }
    };
    serde_json::to_string(&assistant).unwrap_or_default()
}

pub(super) fn api_path(base: &str, resource: &str) -> String {
    if base.ends_with("/v1") {
        format!("/{resource}")
    } else {
        format!("/v1/{resource}")
    }
}

#[cfg(test)]
mod assistant_output_tests {
    use super::*;
    use serde_json::json;

    fn request(system: &str) -> ProviderRequest {
        ProviderRequest {
            request_id: "case:1".to_string(),
            stage: "case_reviewing".to_string(),
            system_instruction: system.to_string(),
            context: json!({}),
            context_markdown: Some("# Case".to_string()),
            output_schema_name: "tasks".to_string(),
            output_schema: json!({}),
            max_input_bytes: 1,
            max_output_tokens: 1,
            timeout_ms: 1,
        }
    }

    #[test]
    fn mistral_cache_key_is_a_stable_digest_of_the_fixed_conversation_opening() {
        let key = |kind, system: &str, history: Value| {
            let mut body = json!({"messages": history});
            apply_prompt_cache_key(kind, &mut body, &request(system), "# Case");
            body.get("prompt_cache_key").cloned()
        };
        let first = key(super::super::ProviderKind::MistralChat, "base", json!([1]));
        let later = key(
            super::super::ProviderKind::MistralChat,
            "base",
            json!([1, 2, 3]),
        );
        let other = key(super::super::ProviderKind::MistralChat, "other", json!([1]));
        let first = first.expect("Mistral requests carry a cache key");
        assert_eq!(Some(first.clone()), later);
        assert_ne!(Some(first.clone()), other);
        let first = first.as_str().expect("string key");
        assert!(first.starts_with("ea-") && first.len() == 35);
        assert!(!first.contains("case:1"));
        assert_eq!(
            key(
                super::super::ProviderKind::OpenAiCompatible,
                "base",
                json!([1])
            ),
            None
        );
    }

    #[test]
    fn mistral_tool_schemas_drop_only_the_keywords_its_strict_mode_rejects() {
        let rendered = || {
            json!({
                "messages": [],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "answer_case_probe",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "limitations": {
                                    "type": "array",
                                    "items": {"type": "string", "enum": ["a", "b"]},
                                    "uniqueItems": true,
                                    "minItems": 2
                                }
                            }
                        },
                        "strict": true
                    }
                }]
            })
        };
        let mut mistral = rendered();
        lower_tool_schemas(super::super::ProviderKind::MistralChat, &mut mistral);
        let limitations =
            &mistral["tools"][0]["function"]["parameters"]["properties"]["limitations"];
        assert!(limitations.get("uniqueItems").is_none());
        assert_eq!(limitations["minItems"], 2);
        assert_eq!(mistral["tools"][0]["function"]["strict"], true);
        let mut other = rendered();
        lower_tool_schemas(super::super::ProviderKind::OpenAiCompatible, &mut other);
        assert_eq!(other, rendered());
    }

    #[test]
    fn repair_projection_preserves_complete_contract_output_without_reasoning_blocks() {
        let tool_call = json!({
            "id": "call_case",
            "type": "function",
            "function": {
                "name": "emit_case_review",
                "arguments": "{\"review\":{\"case_position\":\"substantiated\"}}"
            }
        });
        let chat = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning": "private chain",
                    "reasoning_details": [{"text": "private chain"}],
                    "tool_calls": [tool_call.clone()]
                }
            }]
        });
        let projected = serde_json::from_str::<Value>(&tool_assistant_output(
            ProviderProtocol::ChatCompletions,
            &chat,
        ))
        .unwrap();
        assert_eq!(projected["tool_calls"], json!([tool_call.clone()]));
        assert!(projected.get("reasoning").is_none());
        assert!(projected.get("reasoning_details").is_none());

        let messages = json!({
            "content": [
                {"type": "thinking", "thinking": "private chain"},
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "tool_use", "id": "call_case", "name": "emit_case_review",
                 "input": {"review": {"case_position": "substantiated"}}}
            ]
        });
        let projected = serde_json::from_str::<Value>(&tool_assistant_output(
            ProviderProtocol::Messages,
            &messages,
        ))
        .unwrap();
        assert_eq!(projected.as_array().unwrap().len(), 1);
        assert_eq!(projected[0]["type"], "tool_use");

        let responses = json!({
            "output": [
                {"type": "reasoning", "summary": []},
                {"type": "function_call", "call_id": "call_case",
                 "name": "emit_case_review",
                 "arguments": "{\"review\":{\"case_position\":\"substantiated\"}}"}
            ]
        });
        let projected = serde_json::from_str::<Value>(&tool_assistant_output(
            ProviderProtocol::Responses,
            &responses,
        ))
        .unwrap();
        assert_eq!(projected.as_array().unwrap().len(), 1);
        assert_eq!(projected[0]["type"], "function_call");
        assert_eq!(
            projected[0]["arguments"],
            "{\"review\":{\"case_position\":\"substantiated\"}}"
        );
    }
}

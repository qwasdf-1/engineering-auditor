use auditor_core::{
    render_typed_markdown, MarkdownDocument, MarkdownField, MarkdownSection, MarkdownValue,
};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TranscriptMarkdownOptions {
    pub source: String,
    pub model_input_only: bool,
    pub stage_prefixes: Vec<String>,
}

pub fn render_transcript_markdown(
    transcript_json: &str,
    options: &TranscriptMarkdownOptions,
) -> Result<String, String> {
    let document: Value = serde_json::from_str(transcript_json)
        .map_err(|error| format!("invalid transcript JSON: {error}"))?;
    let root = document
        .as_object()
        .ok_or_else(|| "transcript root must be an object".to_string())?;
    let events = root
        .get("transcript")
        .and_then(Value::as_array)
        .ok_or_else(|| "transcript.transcript must be an array".to_string())?;
    let selected = events
        .iter()
        .filter(|event| {
            options.stage_prefixes.is_empty()
                || options.stage_prefixes.iter().any(|prefix| {
                    event["stage"]
                        .as_str()
                        .is_some_and(|stage| stage.starts_with(prefix))
                })
        })
        .collect::<Vec<_>>();
    let mut sections = vec![MarkdownSection::new(
        "Transcript",
        vec![
            text_field("Source", &options.source),
            text_field(
                "Transcript schema version",
                scalar_text(root.get("transcript_schema_version")),
            ),
            text_field("Review ID", scalar_text(root.get("review_id"))),
            text_field("Selected events", selected.len()),
            text_field(
                "View",
                if options.model_input_only {
                    "provider-visible input"
                } else {
                    "complete ordered transcript"
                },
            ),
            text_field(
                "Reasoning availability",
                "Only reasoning text returned by the provider can be rendered; hidden reasoning is not reconstructable.",
            ),
        ],
    )];
    for (index, event) in selected.into_iter().enumerate() {
        sections.push(event_section(index + 1, event, options.model_input_only)?);
    }
    render_typed_markdown(&MarkdownDocument::new(
        if options.model_input_only {
            "Engineering Auditor Model-Input View"
        } else {
            "Engineering Auditor Provider Transcript"
        },
        sections,
    ))
    .map_err(|error| error.to_string())
}

fn event_section(
    index: usize,
    event: &Value,
    model_input_only: bool,
) -> Result<MarkdownSection, String> {
    let event = event
        .as_object()
        .ok_or_else(|| format!("transcript event {index} must be an object"))?;
    let stage = event
        .get("stage")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut fields = vec![
        text_field("Request ID", scalar_text(event.get("request_id"))),
        text_field("Provider", scalar_text(event.get("provider_id"))),
        text_field("Provider kind", scalar_text(event.get("provider_kind"))),
        text_field("Model", scalar_text(event.get("model"))),
    ];
    let request_body = event
        .get("transport_request")
        .and_then(Value::as_object)
        .and_then(|request| request.get("body_text"))
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    if let Some(request) = request_body.as_ref().and_then(Value::as_object) {
        fields.extend(provider_request_fields(request));
    } else {
        fields.push(text_field(
            "System instruction",
            scalar_text(event.get("system_instruction")),
        ));
        if let Some(context) = event.get("context") {
            fields.push(MarkdownField::new(
                "Canonical request context",
                crate::model_value_from_json(context),
            ));
        }
    }
    if !model_input_only {
        let (provider_fields, provider_body_present) = provider_return_fields(event);
        fields.extend(provider_fields);
        fields.extend([
            text_field(
                "Parsed text",
                if !provider_body_present {
                    scalar_text(event.get("parsed_text"))
                } else {
                    Default::default()
                },
            ),
            MarkdownField::new(
                "Structured provider output",
                (!provider_body_present)
                    .then(|| event.get("structured_output"))
                    .flatten()
                    .map(normalized_wire_value)
                    .map(|value| crate::model_value_from_json(&value))
                    .unwrap_or_else(|| MarkdownValue::text("")),
            ),
            text_field("Finish reason", scalar_text(event.get("finish_reason"))),
            text_field("Failure class", scalar_text(event.get("failure_class"))),
            text_field("Failure message", scalar_text(event.get("failure_message"))),
            text_field("Discarded", scalar_text(event.get("discarded"))),
            MarkdownField::new(
                "Usage",
                event
                    .get("usage")
                    .map(crate::model_value_from_json)
                    .unwrap_or_else(|| MarkdownValue::text("")),
            ),
            MarkdownField::new(
                "Applied fixes",
                event
                    .get("applied_fixes")
                    .map(crate::model_value_from_json)
                    .unwrap_or_else(|| MarkdownValue::text("")),
            ),
        ]);
    }
    Ok(MarkdownSection::new(
        format!("Event {index}: {stage}"),
        fields,
    ))
}

fn provider_request_fields(request: &Map<String, Value>) -> Vec<MarkdownField> {
    let controls = [
        "model",
        "max_tokens",
        "max_output_tokens",
        "temperature",
        "top_p",
        "tool_choice",
        "response_format",
    ]
    .into_iter()
    .filter_map(|key| {
        request
            .get(key)
            .map(|value| MarkdownField::new(key, crate::model_value_from_json(value)))
    })
    .collect::<Vec<_>>();
    let messages = request
        .get("messages")
        .or_else(|| request.get("input"))
        .or_else(|| request.get("contents"))
        .map(normalized_wire_value)
        .map(|value| crate::model_value_from_json(&value))
        .unwrap_or_else(|| MarkdownValue::text(""));
    let tools = request
        .get("tools")
        .map(normalized_wire_value)
        .map(|value| crate::model_value_from_json(&value))
        .unwrap_or_else(|| MarkdownValue::text(""));
    vec![
        MarkdownField::new("Request controls", MarkdownValue::fields(controls)),
        MarkdownField::new("Ordered messages", messages),
        MarkdownField::new("Native tool contracts", tools),
    ]
}

fn provider_return_fields(event: &Map<String, Value>) -> (Vec<MarkdownField>, bool) {
    let response = event.get("transport_response").and_then(Value::as_object);
    let status = response.and_then(|response| response.get("status"));
    let body = response
        .and_then(|response| response.get("body_text"))
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let visible = body.as_ref().map(provider_visible_return);
    (
        vec![
            text_field("Provider status", scalar_text(status)),
            MarkdownField::new(
                "Provider return",
                visible
                    .as_ref()
                    .map(crate::model_value_from_json)
                    .unwrap_or_else(|| MarkdownValue::text("")),
            ),
        ],
        body.is_some(),
    )
}

fn provider_visible_return(body: &Value) -> Value {
    let normalized = normalized_wire_value(body);
    let Some(object) = normalized.as_object() else {
        return normalized;
    };
    for key in ["choices", "output", "candidates", "content"] {
        if let Some(value) = object.get(key) {
            return value.clone();
        }
    }
    if let Some(error) = object.get("error") {
        return Value::Object(Map::from_iter([("error".to_string(), error.clone())]));
    }
    normalized
}

fn normalized_wire_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(normalized_wire_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    if key == "raw_arguments" && object.contains_key("arguments") {
                        return None;
                    }
                    let output_key = if key == "raw_arguments" {
                        "arguments".to_string()
                    } else {
                        key.clone()
                    };
                    let value = if matches!(key.as_str(), "arguments" | "raw_arguments" | "input") {
                        value
                            .as_str()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .map(|value| normalized_wire_value(&value))
                            .unwrap_or_else(|| normalized_wire_value(value))
                    } else {
                        normalized_wire_value(value)
                    };
                    Some((output_key, value))
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn scalar_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn text_field(label: &str, value: impl ToString) -> MarkdownField {
    MarkdownField::new(label, MarkdownValue::text(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transcript_projection_is_ordered_typed_and_transport_free() {
        let request = json!({
            "model": "mock-model",
            "messages": [
                {"role": "system", "content": "Use native tools."},
                {"role": "user", "content": "# Case\n\nComplete source."},
                {"role": "assistant", "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": {"name": "answer_question", "arguments": "{\"answer\":\"yes\"}"}
                }]}
            ],
            "tools": [{"type": "function", "function": {
                "name": "answer_question",
                "description": "Answer one question.",
                "parameters": {"type": "object", "properties": {"answer": {"enum": ["yes", "no"]}}}
            }}]
        });
        let response =
            json!({"choices": [{"message": {"content": "Reason returned.", "tool_calls": []}}]});
        let transcript = json!({
            "transcript_schema_version": 1,
            "review_id": "review:1",
            "transcript": [{
                "request_id": "request:1",
                "stage": "case_reviewing",
                "provider_id": "mock",
                "provider_kind": "openai_compatible",
                "model": "mock-model",
                "transport_request": {"body_text": request.to_string()},
                "transport_response": {"status": 200, "body_text": response.to_string()},
                "structured_output": {"tool_calls": []},
                "parsed_text": "Reason returned.",
                "finish_reason": "tool_calls",
                "failure_class": null,
                "failure_message": null,
                "discarded": false,
                "usage": {"input_tokens": 10, "output_tokens": 2},
                "applied_fixes": []
            }]
        });
        let markdown = render_transcript_markdown(
            &transcript.to_string(),
            &TranscriptMarkdownOptions {
                source: "mock.json".to_string(),
                model_input_only: false,
                stage_prefixes: vec!["case_".to_string()],
            },
        )
        .unwrap();
        assert!(markdown.contains("## Event 1: case_reviewing"));
        assert!(markdown.contains("Use native tools."));
        assert!(markdown.contains("**answer:** yes"));
        assert!(markdown.contains("Reason returned."));
        assert!(!markdown.contains("transport_request"));
        assert!(!markdown.contains("{\"answer\""));
    }
}

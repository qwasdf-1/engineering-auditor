//! Streamed chat completions: the request asks for a server-sent event
//! stream, and the stream's chunks are put back together as the completion
//! the provider would have returned unstreamed, so the rest of the adapter
//! (and the transcript) reads one response either way.

use serde_json::{json, Map, Value};

use crate::provider::ProviderKind;

/// Asks for a stream. An OpenAI-compatible stream reports usage only when
/// asked; Mistral's last chunk always carries it.
pub(super) fn request_stream(kind: ProviderKind, body: &mut Value) {
    body["stream"] = Value::Bool(true);
    if kind != ProviderKind::MistralChat {
        body["stream_options"] = json!({"include_usage": true});
    }
}

/// The completion a stream's chunks add up to; `None` when the body is
/// not an event stream. A chunk carrying an error is returned as it is,
/// so the error is classed like an unstreamed one.
pub(super) fn assemble(body: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(body).ok()?;
    let mut completion = Completion::default();
    let mut chunks = 0;
    for data in text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .filter(|data| *data != "[DONE]")
    {
        let chunk: Value = serde_json::from_str(data).ok()?;
        if chunk.get("error").is_some() {
            return Some(chunk);
        }
        completion.add(&chunk);
        chunks += 1;
    }
    (chunks > 0).then(|| completion.into_value())
}

#[derive(Default)]
struct Completion {
    head: Map<String, Value>,
    message: Map<String, Value>,
    tool_calls: Vec<Value>,
    finish_reason: Value,
    usage: Value,
}

impl Completion {
    fn add(&mut self, chunk: &Value) {
        for key in ["id", "model", "created"] {
            if let Some(value) = chunk.get(key).filter(|value| !value.is_null()) {
                self.head.entry(key).or_insert_with(|| value.clone());
            }
        }
        if chunk.get("usage").is_some_and(|usage| !usage.is_null()) {
            self.usage = chunk["usage"].clone();
        }
        let choice = &chunk["choices"][0];
        if !choice["finish_reason"].is_null() {
            self.finish_reason = choice["finish_reason"].clone();
        }
        let Some(delta) = choice["delta"].as_object() else {
            return;
        };
        for (key, value) in delta {
            match (key.as_str(), value) {
                ("tool_calls", Value::Array(calls)) => {
                    calls.iter().for_each(|call| self.call(call))
                }
                ("role" | "index", _) | (_, Value::Null) => {}
                ("content", _) => {
                    append_content(self.message.entry(key).or_insert_with(|| json!("")), value);
                }
                (_, Value::String(text)) => append_text(self.message.entry(key), text),
                _ => {
                    self.message.insert(key.clone(), value.clone());
                }
            }
        }
    }

    /// One tool-call fragment, joined to the call of its index, or with no
    /// index to the last call; a fragment naming a new call id starts a
    /// call (some providers send each whole call at index 0).
    fn call(&mut self, fragment: &Value) {
        let calls = self.tool_calls.len();
        let mut position = match fragment["index"].as_u64() {
            Some(index) => usize::try_from(index).map_or(calls, |index| index.min(calls)),
            None => calls.saturating_sub(1),
        };
        let new_id = |call: &Value| {
            fragment["id"].is_string() && call["id"].is_string() && call["id"] != fragment["id"]
        };
        if self.tool_calls.get(position).is_none_or(new_id) {
            position = calls;
            self.tool_calls
                .push(json!({"type": "function", "function": {"name": "", "arguments": ""}}));
        }
        let call = &mut self.tool_calls[position];
        for key in ["id", "type"] {
            if let Some(value) = fragment.get(key).filter(|value| value.is_string()) {
                call[key] = value.clone();
            }
        }
        let function = &fragment["function"];
        if let Some(name) = function["name"].as_str().filter(|name| !name.is_empty()) {
            if call["function"]["name"].as_str() != Some(name) {
                append_text(as_entry(&mut call["function"], "name"), name);
            }
        }
        if let Some(arguments) = function["arguments"].as_str() {
            append_text(as_entry(&mut call["function"], "arguments"), arguments);
        }
    }

    fn into_value(mut self) -> Value {
        self.message.insert("role".to_string(), json!("assistant"));
        if !self.tool_calls.is_empty() {
            self.message
                .insert("tool_calls".to_string(), Value::Array(self.tool_calls));
        }
        let mut value = self.head;
        value.insert("object".to_string(), json!("chat.completion"));
        value.insert(
            "choices".to_string(),
            json!([{"index": 0, "message": self.message, "finish_reason": self.finish_reason}]),
        );
        value.insert("usage".to_string(), self.usage);
        Value::Object(value)
    }
}

fn as_entry<'v>(value: &'v mut Value, key: &str) -> serde_json::map::Entry<'v> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("made an object").entry(key)
}

fn append_text(entry: serde_json::map::Entry<'_>, text: &str) {
    let value = entry.or_insert_with(|| json!(""));
    match value {
        Value::String(existing) => existing.push_str(text),
        other => *other = json!(text),
    }
}

/// Content as text, or once a delta brings parts (Mistral's `thinking` and
/// `text` chunks, between which it sends empty text) as parts.
fn append_content(content: &mut Value, delta: &Value) {
    match (&mut *content, delta) {
        (_, Value::String(text)) if text.is_empty() => {}
        (Value::String(existing), Value::String(text)) => existing.push_str(text),
        (Value::Array(parts), Value::String(text)) => {
            append_parts(parts, &[json!({"type": "text", "text": text})]);
        }
        (_, Value::Array(more)) => {
            if let Value::String(text) = &*content {
                *content = if text.is_empty() {
                    json!([])
                } else {
                    json!([{"type": "text", "text": text}])
                };
            }
            if let Value::Array(parts) = content {
                append_parts(parts, more);
            }
        }
        _ => {}
    }
}

/// Content parts (Mistral's `thinking` and `text` chunks), with a part of
/// the same kind as the last joined to it.
fn append_parts(content: &mut Vec<Value>, parts: &[Value]) {
    for part in parts {
        let same_kind = content
            .last()
            .is_some_and(|last| last["type"] == part["type"] && last["type"].is_string());
        match (same_kind, part["type"].as_str()) {
            (true, Some("text")) => {
                if let (Some(last), Some(text)) = (content.last_mut(), part["text"].as_str()) {
                    append_text(as_entry(last, "text"), text);
                }
            }
            (true, Some("thinking")) => {
                if let (Some(Value::Array(inner)), Some(more)) = (
                    content.last_mut().and_then(|last| last.get_mut("thinking")),
                    part["thinking"].as_array(),
                ) {
                    append_parts(inner, more);
                }
            }
            _ => content.push(part.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(chunks: &[Value]) -> Vec<u8> {
        let mut body = String::new();
        for chunk in chunks {
            body.push_str(&format!("data: {chunk}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");
        body.into_bytes()
    }

    #[test]
    fn an_openai_compatible_stream_adds_up_to_its_completion() {
        let body = stream(&[
            json!({"id": "c1", "model": "m", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]}),
            json!({"id": "c1", "choices": [{"delta": {"reasoning_content": "think "}}]}),
            json!({"id": "c1", "choices": [{"delta": {"reasoning_content": "more"}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "t1", "type": "function", "function": {"name": "read_source", "arguments": "{\"pa"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "th\": \"a\"}"}},
                {"index": 1, "id": "t2", "function": {"name": "history", "arguments": "{}"}}]}}]}),
            json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 4}}),
        ]);
        let value = assemble(&body).unwrap();
        let message = &value["choices"][0]["message"];
        assert_eq!(value["id"], "c1");
        assert_eq!(message["reasoning_content"], "think more");
        assert_eq!(message["content"], "");
        assert_eq!(message["tool_calls"][0]["id"], "t1");
        assert_eq!(message["tool_calls"][0]["function"]["name"], "read_source");
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            "{\"path\": \"a\"}"
        );
        assert_eq!(message["tool_calls"][1]["function"]["name"], "history");
        assert_eq!(value["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(value["usage"]["prompt_tokens"], 10);
    }

    #[test]
    fn mistral_thinking_parts_and_whole_tool_calls_join_up() {
        let thinking = |text: &str| json!([{"type": "thinking", "thinking": [{"type": "text", "text": text}]}]);
        let body = stream(&[
            json!({"id": "m1", "choices": [{"delta": {"role": "assistant", "content": thinking("Let ")}}]}),
            json!({"choices": [{"delta": {"content": thinking("me look.")}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"id": "a", "function": {"name": "findings_in", "arguments": "{\"scope\": \"file:7\"}"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"id": "b", "function": {"name": "findings_in", "arguments": "{\"scope\": \"file:8\"}"}}]}}],
                "usage": {"prompt_tokens": 7, "completion_tokens": 2}}),
            json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
        ]);
        let value = assemble(&body).unwrap();
        let message = &value["choices"][0]["message"];
        assert_eq!(
            message["content"],
            json!([{"type": "thinking", "thinking": [{"type": "text", "text": "Let me look."}]}])
        );
        let calls = message["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["id"], "b");
        assert_eq!(calls[1]["function"]["arguments"], "{\"scope\": \"file:8\"}");
        assert_eq!(value["usage"]["completion_tokens"], 2);
    }

    #[test]
    fn mistral_empty_text_between_thinking_parts_keeps_them() {
        // The chunk shapes of a live Mistral stream: an `index` in the
        // delta, empty text between thinking parts, and continuation
        // fragments with an empty name.
        let delta = |delta: Value| json!({"id": "x", "choices": [{"index": 0, "delta": delta, "finish_reason": null}]});
        let thought = |text: &str| json!([{"type": "thinking", "thinking": [{"type": "text", "text": text}], "closed": true}]);
        let body = stream(&[
            delta(json!({"role": "assistant", "content": ""})),
            delta(json!({"index": 0, "content": thought("The")})),
            delta(json!({"index": 0, "content": ""})),
            delta(json!({"index": 0, "content": thought(" user")})),
            delta(json!({"index": 0, "content": "", "tool_calls": [
                {"id": "t0", "type": "function", "function": {"name": "lookup", "arguments": "{\"name\": "}, "index": 0}]})),
            delta(json!({"index": 0, "content": "", "tool_calls": [
                {"function": {"name": "", "arguments": "\"a\"}"}, "index": 0}]})),
            json!({"id": "x", "choices": [{"index": 0, "delta": {"index": 0, "content": ""}, "finish_reason": "tool_calls"}],
                "usage": {"prompt_tokens": 174, "completion_tokens": 88}}),
        ]);
        let value = assemble(&body).unwrap();
        let message = &value["choices"][0]["message"];
        assert_eq!(
            message["content"],
            json!([{"type": "thinking", "thinking": [{"type": "text", "text": "The user"}], "closed": true}])
        );
        assert!(message.get("index").is_none());
        assert_eq!(
            message["tool_calls"][0]["function"],
            json!({"name": "lookup", "arguments": "{\"name\": \"a\"}"})
        );
        assert_eq!(value["usage"]["prompt_tokens"], 174);
    }

    #[test]
    fn an_error_chunk_is_returned_and_a_plain_body_is_not_a_stream() {
        let body = stream(&[
            json!({"choices": [{"delta": {"content": "a"}}]}),
            json!({"error": {"message": "Not enough capacity available", "code": "3505"}}),
        ]);
        assert_eq!(assemble(&body).unwrap()["error"]["code"], "3505");
        assert!(assemble(br#"{"choices": []}"#).is_none());
        assert!(assemble(b"data: {broken\n\n").is_none());
    }

    #[test]
    fn only_an_openai_compatible_stream_asks_for_usage() {
        let mut body = json!({});
        request_stream(ProviderKind::OpenAiCompatible, &mut body);
        assert_eq!(
            body,
            json!({"stream": true, "stream_options": {"include_usage": true}})
        );
        let mut body = json!({});
        request_stream(ProviderKind::MistralChat, &mut body);
        assert_eq!(body, json!({"stream": true}));
    }
}

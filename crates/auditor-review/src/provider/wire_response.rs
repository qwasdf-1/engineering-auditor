use serde_json::Value;

pub(crate) use super::schema_validation::validate_schema;
use super::types::{
    classify_http_status, ProviderFailure, ProviderFailureClass, ProviderProtocol, Usage,
};

pub(super) fn extract_usage(protocol: ProviderProtocol, value: &Value) -> Usage {
    match protocol {
        ProviderProtocol::ChatCompletions => Usage {
            input_tokens: usize_value(&value["usage"]["prompt_tokens"]),
            output_tokens: usize_value(&value["usage"]["completion_tokens"]),
            cached_input_tokens: usize_value(
                &value["usage"]["prompt_tokens_details"]["cached_tokens"],
            ),
        },
        ProviderProtocol::Messages => Usage {
            input_tokens: usize_value(&value["usage"]["input_tokens"]),
            output_tokens: usize_value(&value["usage"]["output_tokens"]),
            cached_input_tokens: usize_value(&value["usage"]["cache_read_input_tokens"]),
        },
        ProviderProtocol::Responses => Usage {
            input_tokens: usize_value(&value["usage"]["input_tokens"]),
            output_tokens: usize_value(&value["usage"]["output_tokens"]),
            cached_input_tokens: usize_value(
                &value["usage"]["input_tokens_details"]["cached_tokens"],
            ),
        },
    }
}

pub(super) fn extract_request_id(value: &Value) -> Option<String> {
    optional_string(&value["id"])
}

pub(super) fn extract_finish_reason(protocol: ProviderProtocol, value: &Value) -> Option<String> {
    match protocol {
        ProviderProtocol::ChatCompletions => optional_string(&value["choices"][0]["finish_reason"]),
        ProviderProtocol::Messages => optional_string(&value["stop_reason"]),
        ProviderProtocol::Responses => optional_string(&value["status"]),
    }
}

fn optional_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn usize_value(value: &Value) -> usize {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

pub(super) fn classify_transport_failure(message: &str) -> ProviderFailureClass {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("timed out") || normalized.contains("timeout") {
        ProviderFailureClass::Timeout
    } else {
        ProviderFailureClass::Transport
    }
}

pub(super) fn http_failure(provider: &str, model: &str, status: u16) -> ProviderFailure {
    let class = classify_http_status(status);
    ProviderFailure::new(
        provider,
        model,
        class,
        retryable_class(class),
        format!("provider returned HTTP status {status}"),
    )
}

pub(super) fn http_response_failure(
    provider: &str,
    model: &str,
    status: u16,
    value: Option<&Value>,
) -> ProviderFailure {
    let mut failure = value
        .and_then(|value| error_envelope_failure(provider, model, value))
        .unwrap_or_else(|| http_failure(provider, model, status));
    let status_class = classify_http_status(status);
    if !matches!(
        status_class,
        ProviderFailureClass::Configuration | ProviderFailureClass::Refusal
    ) {
        failure.class = status_class;
        failure.retryable_without_model_output = retryable_class(status_class);
    }
    failure
}

pub(super) fn error_envelope_failure(
    provider: &str,
    model: &str,
    value: &Value,
) -> Option<ProviderFailure> {
    let error = value.get("error")?.as_object()?;
    let description = error_description(error);
    let class = classify_error_description(&description);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(|message| format!("provider error: {message}"))
        .unwrap_or_else(|| "provider returned an error envelope without model output".to_string());
    Some(ProviderFailure::new(
        provider,
        model,
        class,
        retryable_class(class),
        message,
    ))
}

fn error_description(error: &serde_json::Map<String, Value>) -> String {
    ["type", "code", "message"]
        .into_iter()
        .filter_map(|field| error.get(field).and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn classify_error_description(description: &str) -> ProviderFailureClass {
    const RULES: &[(&[&str], bool, ProviderFailureClass)] = &[
        (
            &["rate", "too many", "quota"],
            false,
            ProviderFailureClass::RateLimit,
        ),
        (
            &["unauthorized", "authentication"],
            false,
            ProviderFailureClass::Authentication,
        ),
        (
            &["forbidden", "permission"],
            false,
            ProviderFailureClass::Authorization,
        ),
        (
            &["context", "length"],
            true,
            ProviderFailureClass::ContextOverflow,
        ),
        (
            &[
                "invalid_request",
                "request is invalid",
                "bad request",
                "does not support",
                "unsupported",
                "grammar error",
                "unimplemented keys",
            ],
            false,
            ProviderFailureClass::Configuration,
        ),
        (
            &["server_error", "upstream", "unavailable", "failover"],
            false,
            ProviderFailureClass::Unavailable,
        ),
        (
            &["refusal", "safety", "content policy"],
            false,
            ProviderFailureClass::Refusal,
        ),
    ];
    RULES
        .iter()
        .find_map(|(markers, require_all, class)| {
            let matches = if *require_all {
                markers.iter().all(|marker| description.contains(marker))
            } else {
                markers.iter().any(|marker| description.contains(marker))
            };
            matches.then_some(*class)
        })
        .unwrap_or(ProviderFailureClass::InvalidOutput)
}

fn retryable_class(class: ProviderFailureClass) -> bool {
    matches!(
        class,
        ProviderFailureClass::RateLimit | ProviderFailureClass::Unavailable
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn cached_prompt_tokens_are_read_from_each_protocol_usage_shape() {
        let chat = json!({"usage": {"prompt_tokens": 1013, "completion_tokens": 30,
            "prompt_tokens_details": {"cached_tokens": 1008}}});
        let usage = extract_usage(ProviderProtocol::ChatCompletions, &chat);
        assert_eq!(
            (usage.input_tokens, usage.cached_input_tokens),
            (1013, 1008)
        );
        let messages = json!({"usage": {"input_tokens": 50, "output_tokens": 5,
            "cache_read_input_tokens": 40}});
        assert_eq!(
            extract_usage(ProviderProtocol::Messages, &messages).cached_input_tokens,
            40
        );
        let responses = json!({"usage": {"input_tokens": 70, "output_tokens": 5,
            "input_tokens_details": {"cached_tokens": 64}}});
        assert_eq!(
            extract_usage(ProviderProtocol::Responses, &responses).cached_input_tokens,
            64
        );
        let uncached = json!({"usage": {"prompt_tokens": 9, "completion_tokens": 1}});
        assert_eq!(
            extract_usage(ProviderProtocol::ChatCompletions, &uncached).cached_input_tokens,
            0
        );
    }

    #[test]
    fn wrapped_upstream_grammar_error_is_configuration() {
        let response = json!({
            "error": {
                "type": "provider_error",
                "message": "Upstream request failed: The request is invalid: bad request: Grammar error: Unimplemented keys: [\"uniqueItems\"]"
            }
        });

        let failure = error_envelope_failure("provider", "model", &response)
            .expect("error envelope should produce a failure");

        assert_eq!(failure.class, ProviderFailureClass::Configuration);
        assert!(!failure.retryable_without_model_output);
    }

    #[test]
    fn genuine_upstream_unavailability_remains_retryable() {
        let response = json!({
            "error": {
                "type": "provider_error",
                "message": "Upstream request failed: Endpoint is unavailable."
            }
        });

        let failure = error_envelope_failure("provider", "model", &response)
            .expect("error envelope should produce a failure");

        assert_eq!(failure.class, ProviderFailureClass::Unavailable);
        assert!(failure.retryable_without_model_output);
    }
}

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{
    types::{
        classify_http_status, DiscoveryRecord, HostedConsent, ProviderConfig, ProviderFailureClass,
        ProviderKind,
    },
    Transport, TransportHeader, TransportRequest,
};

const DISCOVERY_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub fn discover_provider(
    provider: &ProviderConfig,
    transport: &dyn Transport,
    consent: HostedConsent,
    timeout_ms: u64,
) -> DiscoveryRecord {
    let failure = |class| DiscoveryRecord {
        provider_id: provider.id.clone(),
        provider_kind: provider.kind,
        origin: provider.endpoint_base.clone(),
        discovered_models: Vec::new(),
        configured_missing: provider.models.clone(),
        discovered_unconfigured: Vec::new(),
        failure_class: Some(class),
        observed_at_unix_seconds: now_seconds(),
    };
    if provider.validate().is_err() {
        return failure(ProviderFailureClass::Configuration);
    }
    if !consent.allow_hosted || (provider.kind.is_anonymous() && !consent.allow_anonymous) {
        return failure(ProviderFailureClass::Policy);
    }
    let path = if provider.endpoint_base.ends_with("/v1") {
        "/models"
    } else {
        "/v1/models"
    };
    let response = match transport.get(TransportRequest {
        url: format!("{}{path}", provider.endpoint_base),
        headers: discovery_headers(provider),
        body: Vec::new(),
        timeout_ms,
        max_response_bytes: DISCOVERY_RESPONSE_BYTES,
    }) {
        Ok(response) if (200..300).contains(&response.status) => response,
        Ok(response) => return failure(classify_http_status(response.status)),
        Err(message) => return failure(classify_discovery_transport(&message)),
    };
    let value: Value = match serde_json::from_slice(&response.body) {
        Ok(value) => value,
        Err(_) => return failure(ProviderFailureClass::InvalidOutput),
    };
    let discovered_models = discovered_models(&value);
    let configured = provider.models.iter().cloned().collect::<BTreeSet<_>>();
    let discovered = discovered_models.iter().cloned().collect::<BTreeSet<_>>();
    DiscoveryRecord {
        provider_id: provider.id.clone(),
        provider_kind: provider.kind,
        origin: provider.endpoint_base.clone(),
        configured_missing: configured.difference(&discovered).cloned().collect(),
        discovered_unconfigured: discovered.difference(&configured).cloned().collect(),
        discovered_models,
        failure_class: None,
        observed_at_unix_seconds: now_seconds(),
    }
}

fn discovery_headers(provider: &ProviderConfig) -> Vec<TransportHeader> {
    match provider.kind {
        ProviderKind::OpenCodeAnonymous => vec![TransportHeader::Literal {
            name: "authorization".to_string(),
            value: "Bearer public".to_string(),
        }],
        ProviderKind::AnthropicMessages => {
            let mut headers = vec![TransportHeader::Literal {
                name: "anthropic-version".to_string(),
                value: "2023-06-01".to_string(),
            }];
            if let Some(variable) = &provider.api_key_env {
                headers.push(TransportHeader::Environment {
                    name: "x-api-key".to_string(),
                    variable: variable.clone(),
                    prefix: String::new(),
                });
            }
            headers
        }
        _ => provider
            .api_key_env
            .iter()
            .map(|variable| TransportHeader::Environment {
                name: "authorization".to_string(),
                variable: variable.clone(),
                prefix: "Bearer ".to_string(),
            })
            .collect(),
    }
}

fn discovered_models(value: &Value) -> Vec<String> {
    let mut models = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| model.as_str())
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn classify_discovery_transport(message: &str) -> ProviderFailureClass {
    if message.to_ascii_lowercase().contains("timed out") {
        ProviderFailureClass::Timeout
    } else {
        ProviderFailureClass::Transport
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

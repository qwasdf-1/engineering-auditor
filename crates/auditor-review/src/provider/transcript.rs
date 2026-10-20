use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::native_tool_adapter::{AssessmentMode, AssessmentModeRecorded};
use super::transport::TransportHeader;
use super::types::{
    ProviderConfig, ProviderFailure, ProviderFailureClass, ProviderKind, ProviderRequest,
    ProviderResponse, Usage,
};

pub const TRANSCRIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
pub struct TranscriptEvent {
    pub request_id: String,
    pub stage: String,
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub model: String,
    pub started_at_unix_millis: u128,
    pub finished_at_unix_millis: u128,
    pub system_instruction: String,
    pub context: serde_json::Value,
    pub transport_request: TranscriptTransportRequest,
    pub transport_response: Option<TranscriptTransportResponse>,
    pub parsed_text: Option<String>,
    pub structured_output: Option<serde_json::Value>,
    pub finish_reason: Option<String>,
    pub usage: TranscriptUsage,
    pub failure_class: Option<ProviderFailureClass>,
    pub failure_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applied_fixes: Vec<String>,
    pub assessment_mode: AssessmentModeRecorded,
    pub repair_attempts: u8,
    pub discarded: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TranscriptTransportRequest {
    pub url: String,
    pub body_size: usize,
    pub body_text: Option<String>,
    pub headers_redacted: Vec<TranscriptHeader>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptHeader {
    Literal { name: String, value: String },
    Redacted { name: String, variable: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct TranscriptTransportResponse {
    pub status: u16,
    pub body_size: usize,
    pub body_text: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TranscriptUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub cached_input_tokens: usize,
}

impl From<super::types::Usage> for TranscriptUsage {
    fn from(usage: super::types::Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
        }
    }
}

#[derive(Debug, Default)]
pub struct TranscriptRecorder {
    events: Mutex<Vec<TranscriptEvent>>,
}

pub struct TranscriptStart {
    request_id: String,
    stage: String,
    provider_id: String,
    provider_kind: ProviderKind,
    model: String,
    system_instruction: String,
    context: serde_json::Value,
    transport_request: TranscriptTransportRequest,
}

impl TranscriptStart {
    pub fn provider_call(
        request: &ProviderRequest,
        provider: &ProviderConfig,
        model: &str,
        transport_request: TranscriptTransportRequest,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            stage: request.stage.clone(),
            provider_id: provider.id.clone(),
            provider_kind: provider.kind,
            model: model.to_string(),
            system_instruction: request.system_instruction.clone(),
            context: request.context.clone(),
            transport_request,
        }
    }
}

#[derive(Default)]
pub struct TranscriptFailureDetails {
    transport_response: Option<TranscriptTransportResponse>,
    parsed_text: Option<String>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    applied_fixes: Vec<String>,
    discarded: bool,
}

impl TranscriptFailureDetails {
    pub fn with_transport_response(
        mut self,
        response: Option<TranscriptTransportResponse>,
    ) -> Self {
        self.transport_response = response;
        self
    }

    pub fn with_output(
        mut self,
        parsed_text: Option<String>,
        finish_reason: Option<String>,
        usage: Option<Usage>,
    ) -> Self {
        self.parsed_text = parsed_text;
        self.finish_reason = finish_reason;
        self.usage = usage;
        self
    }

    pub fn with_fixes(mut self, applied_fixes: Vec<String>) -> Self {
        self.applied_fixes = applied_fixes;
        self
    }

    pub fn discarded(mut self) -> Self {
        self.discarded = true;
        self
    }
}

#[derive(Debug)]
pub struct CallRecorder<'a> {
    recorder: Option<&'a TranscriptRecorder>,
    request_id: String,
    stage: String,
    provider_id: String,
    provider_kind: ProviderKind,
    model: String,
    started_at_unix_millis: u128,
    system_instruction: String,
    context: serde_json::Value,
    transport_request: TranscriptTransportRequest,
    assessment_mode: AssessmentModeRecorded,
    repair_attempts: u8,
}

impl TranscriptRecorder {
    pub fn null() -> Self {
        Self::default()
    }

    pub fn enabled() -> Self {
        Self::default()
    }

    pub fn is_enabled(&self) -> bool {
        true
    }

    pub fn begin(&self, enabled: bool, start: TranscriptStart) -> CallRecorder<'_> {
        let started_at_unix_millis = now_millis();
        let repair_attempts = u8::from(start.request_id.ends_with(":repair-1"));
        CallRecorder {
            recorder: enabled.then_some(self),
            request_id: start.request_id,
            stage: start.stage,
            provider_id: start.provider_id,
            provider_kind: start.provider_kind,
            model: start.model,
            started_at_unix_millis,
            system_instruction: start.system_instruction,
            context: start.context,
            transport_request: start.transport_request,
            assessment_mode: AssessmentModeRecorded::NativeTools,
            repair_attempts,
        }
    }

    pub fn events(&self) -> Vec<TranscriptEvent> {
        self.events.lock().expect("transcript mutex").clone()
    }

    pub fn to_json(&self, review_id: Option<&str>) -> serde_json::Value {
        let events = self.events.lock().expect("transcript mutex");
        serde_json::json!({
            "transcript_schema_version": TRANSCRIPT_SCHEMA_VERSION,
            "generated_at_unix_seconds": now_seconds(),
            "review_id": review_id,
            "transcript": events.clone(),
        })
    }

    pub fn mark_host_rejected(
        &self,
        request_id: &str,
        validation_error: &str,
        discarded: bool,
    ) -> bool {
        let mut events = self.events.lock().expect("transcript mutex");
        let Some(event) = events
            .iter_mut()
            .rfind(|event| event.request_id == request_id)
        else {
            return false;
        };
        event.failure_class = Some(ProviderFailureClass::InvalidOutput);
        event.failure_message = Some(format!(
            "host admission {} provider output: {validation_error}",
            if discarded {
                "rejected"
            } else {
                "partially rejected"
            }
        ));
        event.discarded = discarded;
        true
    }

    fn record(&self, event: TranscriptEvent) {
        self.events.lock().expect("transcript mutex").push(event);
    }
}

impl CallRecorder<'_> {
    pub fn with_dispatch(mut self, mode: AssessmentMode, repair: bool) -> Self {
        self.assessment_mode = AssessmentModeRecorded::from_mode(mode);
        self.repair_attempts = u8::from(repair);
        self
    }

    pub fn finish_ok(
        self,
        response: &ProviderResponse,
        transport_response: Option<TranscriptTransportResponse>,
        parsed_text: Option<String>,
        applied_fixes: Vec<String>,
        discarded: bool,
    ) {
        if let Some(recorder) = self.recorder {
            let event = TranscriptEvent {
                request_id: self.request_id,
                stage: self.stage,
                provider_id: self.provider_id,
                provider_kind: self.provider_kind,
                model: self.model,
                started_at_unix_millis: self.started_at_unix_millis,
                finished_at_unix_millis: now_millis().max(self.started_at_unix_millis),
                system_instruction: self.system_instruction,
                context: self.context,
                transport_request: self.transport_request,
                transport_response,
                parsed_text,
                structured_output: Some(response.structured_output.clone()),
                finish_reason: response.finish_reason.clone(),
                usage: TranscriptUsage::from(response.usage),
                failure_class: None,
                failure_message: None,
                applied_fixes,
                assessment_mode: self.assessment_mode,
                repair_attempts: self.repair_attempts,
                discarded,
            };
            recorder.record(event);
        }
    }

    pub fn finish_failure(self, failure: &ProviderFailure, details: TranscriptFailureDetails) {
        if let Some(recorder) = self.recorder {
            let usage = TranscriptUsage::from(details.usage.unwrap_or_default());
            let event = TranscriptEvent {
                request_id: self.request_id,
                stage: self.stage,
                provider_id: self.provider_id,
                provider_kind: self.provider_kind,
                model: self.model,
                started_at_unix_millis: self.started_at_unix_millis,
                finished_at_unix_millis: now_millis().max(self.started_at_unix_millis),
                system_instruction: self.system_instruction,
                context: self.context,
                transport_request: self.transport_request,
                transport_response: details.transport_response,
                parsed_text: details.parsed_text,
                structured_output: None,
                finish_reason: details.finish_reason,
                usage,
                failure_class: Some(failure.class),
                failure_message: Some(failure.message.clone()),
                applied_fixes: details.applied_fixes,
                assessment_mode: self.assessment_mode,
                repair_attempts: self.repair_attempts,
                discarded: details.discarded,
            };
            recorder.record(event);
        }
    }
}

pub fn redact_headers(headers: &[TransportHeader]) -> Vec<TranscriptHeader> {
    headers
        .iter()
        .map(|header| match header {
            TransportHeader::Literal { name, value } => TranscriptHeader::Literal {
                name: name.clone(),
                value: value.clone(),
            },
            TransportHeader::Environment { name, variable, .. } => TranscriptHeader::Redacted {
                name: name.clone(),
                variable: variable.clone(),
            },
        })
        .collect()
}

pub fn decode_body(body: &[u8]) -> Option<String> {
    Some(String::from_utf8_lossy(body).into_owned())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::transport::TransportRequest;
    use super::*;

    const INVALID_UTF8: [u8; 3] = [0xFF, 0xFE, 0x00];
    const RESPONSE_STATUS: u16 = 200;
    const RESPONSE_BODY_SIZE: usize = 32;
    const INPUT_TOKENS: usize = 16;
    const OUTPUT_TOKENS: usize = 8;

    fn build_request() -> TransportRequest {
        TransportRequest {
            url: "https://example.test/v1/chat/completions".to_string(),
            headers: vec![
                TransportHeader::Literal {
                    name: "content-type".to_string(),
                    value: "application/json".to_string(),
                },
                TransportHeader::Environment {
                    name: "authorization".to_string(),
                    variable: "OPENAI_API_KEY".to_string(),
                    prefix: "Bearer ".to_string(),
                },
            ],
            body: br#"{"model":"hy3-free"}"#.to_vec(),
            timeout_ms: 30_000,
            max_response_bytes: 64 * 1024,
        }
    }

    fn transcript_start(
        request_id: &str,
        stage: &str,
        context: serde_json::Value,
        transport_request: TranscriptTransportRequest,
    ) -> TranscriptStart {
        TranscriptStart {
            request_id: request_id.to_string(),
            stage: stage.to_string(),
            provider_id: "opencode_anonymous".to_string(),
            provider_kind: ProviderKind::OpenCodeAnonymous,
            model: "hy3-free".to_string(),
            system_instruction: "system prompt".to_string(),
            context,
            transport_request,
        }
    }

    #[test]
    fn redact_headers_keeps_literal_and_redacts_environment_variable_value() {
        let redacted = redact_headers(&build_request().headers);
        assert_eq!(redacted.len(), 2);
        match &redacted[0] {
            TranscriptHeader::Literal { name, value } => {
                assert_eq!(name, "content-type");
                assert_eq!(value, "application/json");
            }
            other => panic!("expected Literal, got {other:?}"),
        }
        match &redacted[1] {
            TranscriptHeader::Redacted { name, variable } => {
                assert_eq!(name, "authorization");
                assert_eq!(variable, "OPENAI_API_KEY");
            }
            other => panic!("expected Redacted, got {other:?}"),
        }
    }

    #[test]
    fn decode_body_handles_utf8_with_replacement_without_truncation() {
        let small = b"hello";
        assert_eq!(decode_body(small).as_deref(), Some("hello"));

        assert!(decode_body(&INVALID_UTF8).is_some());

        let big = vec![b'x'; 128 * 1024];
        let decoded = decode_body(&big).expect("decoded");
        assert_eq!(decoded.len(), big.len());
    }

    #[test]
    fn records_success_and_failure_events_without_leaking_credentials() {
        let recorder = TranscriptRecorder::enabled();
        let transport = build_request();
        let redacted = redact_headers(&transport.headers);
        let request = TranscriptTransportRequest {
            url: transport.url.clone(),
            body_size: transport.body.len(),
            body_text: decode_body(&transport.body),
            headers_redacted: redacted,
        };

        let ok_call = recorder.begin(
            true,
            transcript_start(
                "req-1",
                "atomic_assessment",
                serde_json::json!({"context": "ok"}),
                request.clone(),
            ),
        );
        let response = ProviderResponse {
            provider_id: "opencode_anonymous".to_string(),
            model: "hy3-free".to_string(),
            provider_request_id: Some("gen-1".to_string()),
            finish_reason: Some("stop".to_string()),
            structured_output: serde_json::json!({"nonce": "x"}),
            usage: super::super::types::Usage {
                input_tokens: INPUT_TOKENS,
                output_tokens: OUTPUT_TOKENS,
                cached_input_tokens: 0,
            },
            capability_observations: vec!["provider_native_tools_v2".to_string()],
            native_assistant_output: None,
        };
        ok_call.finish_ok(
            &response,
            Some(TranscriptTransportResponse {
                status: RESPONSE_STATUS,
                body_size: RESPONSE_BODY_SIZE,
                body_text: Some("{\"choices\":[{}]}".to_string()),
            }),
            Some("{\"nonce\":\"x\"}".to_string()),
            Vec::new(),
            false,
        );

        let failure_call = recorder.begin(
            true,
            transcript_start(
                "req-2",
                "evaluation",
                serde_json::json!({"context": "fail"}),
                request,
            ),
        );
        let failure = ProviderFailure::new(
            "opencode_anonymous",
            "hy3-free",
            ProviderFailureClass::Transport,
            true,
            "HTTP transport request failed: connection timed out",
        );
        failure_call.finish_failure(&failure, TranscriptFailureDetails::default().discarded());

        let events = recorder.events();
        assert_eq!(events.len(), 2);

        let ok_event = &events[0];
        assert_eq!(ok_event.stage, "atomic_assessment");
        assert_eq!(ok_event.model, "hy3-free");
        assert_eq!(ok_event.failure_class, None);
        assert_eq!(ok_event.structured_output.as_ref().unwrap()["nonce"], "x");
        assert!(ok_event
            .transport_request
            .body_text
            .as_deref()
            .unwrap()
            .contains("hy3-free"));
        assert_eq!(ok_event.transport_request.headers_redacted.len(), 2);
        assert!(matches!(
            ok_event.transport_request.headers_redacted[1],
            TranscriptHeader::Redacted { .. }
        ));

        let fail_event = &events[1];
        assert!(ok_event.finished_at_unix_millis >= ok_event.started_at_unix_millis);
        assert!(fail_event.finished_at_unix_millis >= fail_event.started_at_unix_millis);
        assert_eq!(
            fail_event.failure_class,
            Some(ProviderFailureClass::Transport)
        );
        assert_eq!(
            fail_event.failure_message.as_deref(),
            Some("HTTP transport request failed: connection timed out")
        );
        assert!(fail_event.discarded);
        assert!(fail_event.structured_output.is_none());

        assert!(recorder.mark_host_rejected("req-1", "InvalidClosure", true));
        let annotated = recorder.events();
        assert_eq!(
            annotated[0].failure_class,
            Some(ProviderFailureClass::InvalidOutput)
        );
        assert_eq!(
            annotated[0].failure_message.as_deref(),
            Some("host admission rejected provider output: InvalidClosure")
        );
        assert!(annotated[0].discarded);
        assert!(annotated[0].structured_output.is_some());
    }

    #[test]
    fn json_envelope_carries_schema_version_and_review_id() {
        let recorder = TranscriptRecorder::enabled();
        let payload = recorder.to_json(Some("review:abc"));
        assert_eq!(
            payload["transcript_schema_version"],
            TRANSCRIPT_SCHEMA_VERSION
        );
        assert_eq!(payload["review_id"], "review:abc");
        assert!(payload["transcript"].is_array());
    }

    #[test]
    fn transcript_event_records_tool_mode_and_repair_attempt() {
        let recorder = TranscriptRecorder::enabled();
        let transport = build_request();
        let call = recorder.begin(
            true,
            transcript_start(
                "request:atomic_assessment:digest:repair-1",
                "atomic_assessment",
                serde_json::json!({"context": "repair"}),
                TranscriptTransportRequest {
                    url: transport.url,
                    body_size: transport.body.len(),
                    body_text: decode_body(&transport.body),
                    headers_redacted: redact_headers(&transport.headers),
                },
            ),
        );
        let response = ProviderResponse {
            provider_id: "opencode_anonymous".to_string(),
            model: "hy3-free".to_string(),
            provider_request_id: Some("tool-repair".to_string()),
            finish_reason: Some("tool_calls".to_string()),
            structured_output: serde_json::json!({"assessments": [], "source_requests": []}),
            usage: super::super::types::Usage::default(),
            capability_observations: vec![
                "provider_native_tools_v2".to_string(),
                "repair_v1".to_string(),
            ],
            native_assistant_output: None,
        };
        call.finish_ok(
            &response,
            None,
            Some("tool call".to_string()),
            Vec::new(),
            false,
        );

        let event = serde_json::to_value(&recorder.events()[0]).unwrap();
        assert_eq!(event["assessment_mode"], "provider_native_tools_v2");
        assert_eq!(event["repair_attempts"], 1);
    }
}

use serde_json::{json, Value};

use super::dispatch::CallDispatch;
use super::native_tool::TOOL_NOOP;
use super::native_tool_adapter::{
    parse_tool_response, AssessmentModeRecorded, ParsedToolOutput, ToolStopReason,
};
use super::transcript::{
    decode_body, redact_headers, CallRecorder, TranscriptFailureDetails, TranscriptRecorder,
    TranscriptStart, TranscriptTransportRequest, TranscriptTransportResponse,
};
use super::transport::{Transport, TransportHeader, TransportRequest};
use super::types::{
    HostedConsent, ProviderConfig, ProviderFailure, ProviderFailureClass, ProviderKind,
    ProviderProtocol, ProviderRequest, ProviderResponse, Usage,
};
use super::wire_response::{
    classify_transport_failure, error_envelope_failure, extract_finish_reason, extract_request_id,
    extract_usage, http_response_failure, validate_schema,
};

mod case_tools;
mod render;

use render::{
    api_path, apply_prompt_cache_key, lower_tool_schemas, render_dispatch_body,
    tool_assistant_output,
};

pub struct BuiltinAdapter<'a> {
    config: &'a ProviderConfig,
    transport: &'a dyn Transport,
    consent: HostedConsent,
}

struct CallCompletion<'call, 'request, 'dispatch> {
    model: &'request str,
    request: &'request ProviderRequest,
    dispatch: &'request CallDispatch<'dispatch>,
    call: CallRecorder<'call>,
    transcript_response: Option<TranscriptTransportResponse>,
}

struct ToolMetadata {
    usage: Usage,
    request_id: Option<String>,
    finish_reason: Option<String>,
}

struct FailureCompletion<'call, 'request> {
    model: &'request str,
    call: CallRecorder<'call>,
    transcript_response: Option<TranscriptTransportResponse>,
    finish_reason: Option<String>,
    usage: Usage,
}

pub(super) struct ToolSuccess {
    structured_output: Value,
    raw_text: String,
    capability: &'static str,
    native_assistant_output: Option<String>,
}

enum ToolDispatchKind {
    ProviderProbe,
    NamedProbe,
    NativeCase,
}

impl<'a> BuiltinAdapter<'a> {
    pub fn new(
        config: &'a ProviderConfig,
        transport: &'a dyn Transport,
        consent: HostedConsent,
    ) -> Self {
        Self {
            config,
            transport,
            consent,
        }
    }

    fn finish_truncated_output(
        &self,
        completion: FailureCompletion<'_, '_>,
        message: &str,
        raw_text: String,
    ) -> ProviderFailure {
        let failure = ProviderFailure::new(
            &self.config.id,
            completion.model,
            ProviderFailureClass::Truncated,
            false,
            message,
        );
        completion.call.finish_failure(
            &failure,
            TranscriptFailureDetails::default()
                .with_transport_response(completion.transcript_response)
                .with_output(
                    Some(raw_text),
                    completion.finish_reason,
                    Some(completion.usage),
                ),
        );
        failure
    }

    /// Execute one provider-native tool turn. The adapter never serializes the
    /// case context as JSON and never falls back to text or strict-JSON output.
    pub fn execute_dispatched(
        &self,
        model: &str,
        request: &ProviderRequest,
        dispatch: &CallDispatch<'_>,
        recorder: &TranscriptRecorder,
        transcript_enabled: bool,
    ) -> Result<ProviderResponse, ProviderFailure> {
        self.config.validate().map_err(|message| {
            ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Configuration,
                false,
                message,
            )
        })?;
        self.validate_consent(model)?;
        let visible_context_bytes = request
            .context_markdown
            .as_ref()
            .map_or(0, String::len)
            .saturating_add(request.system_instruction.len());
        if visible_context_bytes > request.max_input_bytes {
            return Err(ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::ContextOverflow,
                false,
                "provider context exceeds the configured input budget",
            ));
        }
        let transport_request = self.transport_request_dispatched(model, request, dispatch)?;
        let transcript_request = TranscriptTransportRequest {
            url: transport_request.url.clone(),
            body_size: transport_request.body.len(),
            body_text: decode_body(&transport_request.body),
            headers_redacted: redact_headers(&transport_request.headers),
        };
        let call = recorder
            .begin(
                transcript_enabled,
                TranscriptStart::provider_call(request, self.config, model, transcript_request),
            )
            .with_dispatch(dispatch.assessment_mode, dispatch.repair.is_some());
        let transport_response = self.transport.post(transport_request);
        match transport_response {
            Ok(response) => {
                let transcript_response = TranscriptTransportResponse {
                    status: response.status,
                    body_size: response.body.len(),
                    body_text: decode_body(&response.body),
                };
                self.complete_call_dispatched(
                    response,
                    CallCompletion {
                        model,
                        request,
                        dispatch,
                        call,
                        transcript_response: Some(transcript_response),
                    },
                )
            }
            Err(message) => {
                let failure = ProviderFailure::new(
                    &self.config.id,
                    model,
                    classify_transport_failure(&message),
                    true,
                    message,
                );
                call.finish_failure(&failure, TranscriptFailureDetails::default());
                Err(failure)
            }
        }
    }

    fn complete_call_dispatched(
        &self,
        response: super::transport::TransportResponse,
        completion: CallCompletion<'_, '_, '_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let CallCompletion {
            model,
            request,
            dispatch,
            call,
            transcript_response,
        } = completion;
        let parsed_value = serde_json::from_slice::<Value>(&response.body);
        if !(200..300).contains(&response.status) {
            let failure = http_response_failure(
                &self.config.id,
                model,
                response.status,
                parsed_value.as_ref().ok(),
            );
            call.finish_failure(
                &failure,
                TranscriptFailureDetails::default().with_transport_response(transcript_response),
            );
            return Err(failure);
        }
        let value: Value = match parsed_value {
            Ok(value) => value,
            Err(_) => {
                let failure = ProviderFailure::new(
                    &self.config.id,
                    model,
                    ProviderFailureClass::InvalidOutput,
                    false,
                    "provider response was not valid JSON",
                );
                call.finish_failure(
                    &failure,
                    TranscriptFailureDetails::default()
                        .with_transport_response(transcript_response),
                );
                return Err(failure);
            }
        };
        if let Some(failure) = error_envelope_failure(&self.config.id, model, &value) {
            call.finish_failure(
                &failure,
                TranscriptFailureDetails::default().with_transport_response(transcript_response),
            );
            return Err(failure);
        }
        let protocol = self.config.protocol_for(model);
        let finish_reason = extract_finish_reason(protocol, &value);
        if finish_reason
            .as_deref()
            .is_some_and(|reason| matches!(reason, "error" | "failed"))
        {
            let usage = extract_usage(protocol, &value);
            let failure = ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Unavailable,
                true,
                "provider declared completion failure before producing a usable contract output",
            )
            .with_output_usage(usage);
            call.finish_failure(
                &failure,
                TranscriptFailureDetails::default()
                    .with_transport_response(transcript_response)
                    .with_output(
                        Some(tool_assistant_output(protocol, &value)),
                        finish_reason,
                        Some(usage),
                    ),
            );
            return Err(failure);
        }
        self.complete_tool_call_dispatch(
            protocol,
            value,
            CallCompletion {
                model,
                request,
                dispatch,
                call,
                transcript_response,
            },
        )
    }

    fn complete_tool_call_dispatch(
        &self,
        protocol: ProviderProtocol,
        response_value: Value,
        completion: CallCompletion<'_, '_, '_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let native_assistant_output = tool_assistant_output(protocol, &response_value);
        let parsed = parse_tool_response(protocol, &response_value);
        let metadata = ToolMetadata {
            usage: extract_usage(protocol, &response_value),
            request_id: extract_request_id(&response_value),
            finish_reason: extract_finish_reason(protocol, &response_value),
        };
        let Some(kind) = tool_dispatch_kind(completion.request) else {
            let failure = ProviderFailure::new(
                &self.config.id,
                completion.model,
                ProviderFailureClass::Configuration,
                false,
                "provider-native dispatch has no registered tool result owner for this stage",
            )
            .with_repair_output(native_assistant_output, AssessmentModeRecorded::NativeTools)
            .with_output_usage(metadata.usage);
            completion.call.finish_failure(
                &failure,
                TranscriptFailureDetails::default()
                    .with_transport_response(completion.transcript_response)
                    .with_output(
                        Some(parsed.raw_text),
                        metadata.finish_reason,
                        Some(metadata.usage),
                    ),
            );
            return Err(failure);
        };
        match kind {
            ToolDispatchKind::ProviderProbe => {
                self.complete_tool_probe(parsed, metadata, completion)
            }
            ToolDispatchKind::NamedProbe => {
                self.complete_named_tool_probe(parsed, metadata, completion)
            }
            ToolDispatchKind::NativeCase => self.complete_native_case_task_calls(
                parsed,
                native_assistant_output,
                metadata,
                completion,
            ),
        }
    }

    fn complete_tool_probe(
        &self,
        parsed: ParsedToolOutput,
        metadata: ToolMetadata,
        completion: CallCompletion<'_, '_, '_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let CallCompletion {
            model,
            request,
            call,
            transcript_response,
            ..
        } = completion;
        let ToolMetadata {
            usage,
            request_id,
            finish_reason,
        } = metadata;
        let noop = parsed
            .calls
            .iter()
            .any(|tool_call| tool_call.tool_name == TOOL_NOOP);
        if parsed.stop_reason == ToolStopReason::NoOp && noop {
            let structured_output = json!({"noop": true});
            if validate_schema(&structured_output, &request.output_schema).is_ok() {
                return Ok(self.finish_tool_success(
                    FailureCompletion {
                        model,
                        call,
                        transcript_response,
                        finish_reason,
                        usage,
                    },
                    request_id,
                    ToolSuccess {
                        structured_output,
                        raw_text: parsed.raw_text,
                        capability: "provider_native_tools_v2",
                        native_assistant_output: None,
                    },
                ));
            }
        }
        Err(self.finish_discarded_tool_failure(
            FailureCompletion {
                model,
                call,
                transcript_response,
                finish_reason,
                usage,
            },
            parsed.raw_text,
            "provider did not complete the required noop tool qualification probe",
        ))
    }

    fn validate_consent(&self, model: &str) -> Result<(), ProviderFailure> {
        if !self.consent.allow_hosted {
            return Err(ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Policy,
                false,
                "hosted provider egress requires explicit consent",
            ));
        }
        if self.config.kind.is_anonymous() && !self.consent.allow_anonymous {
            return Err(ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Policy,
                false,
                "anonymous provider egress requires separate explicit consent",
            ));
        }
        Ok(())
    }

    fn transport_request_dispatched(
        &self,
        model: &str,
        request: &ProviderRequest,
        dispatch: &CallDispatch<'_>,
    ) -> Result<TransportRequest, ProviderFailure> {
        let user_content = request
            .context_markdown
            .as_ref()
            .filter(|markdown| !markdown.is_empty())
            .ok_or_else(|| {
                ProviderFailure::new(
                    &self.config.id,
                    model,
                    ProviderFailureClass::Configuration,
                    false,
                    "provider-native requests require a non-empty Markdown context",
                )
            })?
            .clone();
        if dispatch.tools.is_empty() {
            return Err(ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Configuration,
                false,
                "provider-native requests require at least one declared tool",
            ));
        }
        let protocol = self.config.protocol_for(model);
        let mut body = render_dispatch_body(protocol, model, request, &user_content, dispatch);
        lower_tool_schemas(self.config.kind, &mut body);
        apply_prompt_cache_key(self.config.kind, &mut body, request, &user_content);
        let path = self.request_path(model, protocol)?;
        let headers = self.request_headers(model)?;
        Ok(TransportRequest {
            url: format!("{}{path}", self.config.endpoint_base),
            headers,
            body: serde_json::to_vec(&body).expect("provider request JSON"),
            timeout_ms: request.timeout_ms,
            max_response_bytes: request
                .max_output_tokens
                .saturating_mul(16)
                .saturating_add(64 * 1024)
                .min(16 * 1024 * 1024),
        })
    }

    fn request_path(
        &self,
        model: &str,
        protocol: ProviderProtocol,
    ) -> Result<String, ProviderFailure> {
        let path = match protocol {
            ProviderProtocol::Responses => api_path(&self.config.endpoint_base, "responses"),
            ProviderProtocol::Messages => api_path(&self.config.endpoint_base, "messages"),
            ProviderProtocol::ChatCompletions => {
                if self.config.kind == ProviderKind::OpenAiCompatible {
                    self.config.chat_completions_path.clone().ok_or_else(|| {
                        ProviderFailure::new(
                            &self.config.id,
                            model,
                            ProviderFailureClass::Configuration,
                            false,
                            "custom compatible provider has no chat-completions path",
                        )
                    })?
                } else {
                    api_path(&self.config.endpoint_base, "chat/completions")
                }
            }
        };
        Ok(path)
    }

    fn request_headers(&self, model: &str) -> Result<Vec<TransportHeader>, ProviderFailure> {
        let mut headers = vec![TransportHeader::Literal {
            name: "content-type".to_string(),
            value: "application/json".to_string(),
        }];
        match self.config.kind {
            ProviderKind::AnthropicMessages => {
                headers.push(TransportHeader::Literal {
                    name: "anthropic-version".to_string(),
                    value: "2023-06-01".to_string(),
                });
                headers.push(TransportHeader::Environment {
                    name: "x-api-key".to_string(),
                    variable: self.api_key_env(model)?.to_string(),
                    prefix: String::new(),
                });
            }
            ProviderKind::OpenCodeAnonymous => {
                headers.push(TransportHeader::Literal {
                    name: "authorization".to_string(),
                    value: "Bearer public".to_string(),
                });
            }
            ProviderKind::OpenAiCompatible if self.config.api_key_env.is_none() => {}
            _ => {
                headers.push(TransportHeader::Environment {
                    name: "authorization".to_string(),
                    variable: self.api_key_env(model)?.to_string(),
                    prefix: "Bearer ".to_string(),
                });
            }
        }
        Ok(headers)
    }

    fn api_key_env(&self, model: &str) -> Result<&str, ProviderFailure> {
        let variable = self.config.api_key_env.as_deref().ok_or_else(|| {
            ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Configuration,
                false,
                "provider API key environment variable is not configured",
            )
        })?;
        std::env::var_os(variable).map(|_| variable).ok_or_else(|| {
            ProviderFailure::new(
                &self.config.id,
                model,
                ProviderFailureClass::Configuration,
                false,
                format!("provider API key environment variable is unset: {variable}"),
            )
        })
    }
}

fn tool_dispatch_kind(request: &ProviderRequest) -> Option<ToolDispatchKind> {
    if request.stage == "provider_tool_qualification" {
        Some(ToolDispatchKind::ProviderProbe)
    } else if matches!(
        request.stage.as_str(),
        super::CASE_ROLE_TOOL_QUALIFICATION_STAGE | super::CASE_SEMANTIC_QUALIFICATION_STAGE
    ) {
        Some(ToolDispatchKind::NamedProbe)
    } else if matches!(
        request.output_schema_name.as_str(),
        "case_task_calls" | "challenge_response_tasks"
    ) {
        Some(ToolDispatchKind::NativeCase)
    } else {
        None
    }
}

use super::{
    validate_schema, AssessmentModeRecorded, BuiltinAdapter, CallCompletion, FailureCompletion,
    ProviderFailure, ProviderFailureClass, ProviderResponse, ToolMetadata, ToolStopReason,
    ToolSuccess,
};
use crate::{NativeToolCall, NativeToolTurn};

impl BuiltinAdapter<'_> {
    pub(super) fn complete_native_case_task_calls(
        &self,
        parsed: crate::provider::native_tool_adapter::ParsedToolOutput,
        native_assistant_output: String,
        metadata: ToolMetadata,
        completion: CallCompletion<'_, '_, '_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let CallCompletion {
            model,
            request: _,
            dispatch,
            call,
            transcript_response,
        } = completion;
        if parsed.stop_reason == ToolStopReason::Truncated {
            return Err(self.finish_truncated_output(
                FailureCompletion {
                    model,
                    call,
                    transcript_response,
                    finish_reason: metadata.finish_reason,
                    usage: metadata.usage,
                },
                "provider native case-task output was truncated",
                parsed.raw_text,
            ));
        }
        if !parsed.calls.is_empty() {
            let structured_output = native_turn(&parsed, &dispatch.tools);
            let response = ProviderResponse {
                provider_id: self.config.id.clone(),
                model: model.to_string(),
                provider_request_id: metadata.request_id,
                finish_reason: metadata.finish_reason,
                structured_output,
                usage: metadata.usage,
                capability_observations: vec!["native_case_task_calls_v2".to_string()],
                native_assistant_output: Some(native_assistant_output),
            };
            call.finish_ok(
                &response,
                transcript_response,
                Some(parsed.raw_text),
                Vec::new(),
                false,
            );
            return Ok(response);
        }
        let failure = ProviderFailure::new(
            &self.config.id,
            model,
            ProviderFailureClass::InvalidOutput,
            false,
            "provider emitted no native case-task calls",
        )
        .with_repair_output(native_assistant_output, AssessmentModeRecorded::NativeTools)
        .with_output_usage(metadata.usage);
        call.finish_failure(
            &failure,
            crate::provider::transcript::TranscriptFailureDetails::default()
                .with_transport_response(transcript_response)
                .with_output(
                    Some(parsed.raw_text),
                    metadata.finish_reason,
                    Some(metadata.usage),
                )
                .discarded(),
        );
        Err(failure)
    }

    pub(super) fn complete_named_tool_probe(
        &self,
        parsed: crate::provider::native_tool_adapter::ParsedToolOutput,
        metadata: ToolMetadata,
        completion: CallCompletion<'_, '_, '_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let CallCompletion {
            model,
            request,
            dispatch,
            call,
            transcript_response,
            ..
        } = completion;
        let expected_tool_name = dispatch.tools.first().map(|tool| tool.name);
        let output = parsed
            .calls
            .iter()
            .find(|tool_call| Some(tool_call.tool_name.as_str()) == expected_tool_name)
            .map(|tool_call| tool_call.arguments.clone());
        if let Some(structured_output) = output {
            if validate_schema(&structured_output, &request.output_schema).is_ok() {
                let structured_output =
                    if request.stage == crate::provider::CASE_ROLE_TOOL_QUALIFICATION_STAGE {
                        native_turn(&parsed, &dispatch.tools)
                    } else {
                        structured_output
                    };
                return Ok(self.finish_tool_success(
                    FailureCompletion {
                        model,
                        call,
                        transcript_response,
                        finish_reason: metadata.finish_reason,
                        usage: metadata.usage,
                    },
                    metadata.request_id,
                    ToolSuccess {
                        structured_output,
                        raw_text: parsed.raw_text,
                        capability: "case_tool_call_v3",
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
                finish_reason: metadata.finish_reason,
                usage: metadata.usage,
            },
            parsed.raw_text,
            "provider did not complete the bound case-role tool qualification",
        ))
    }

    pub(super) fn finish_discarded_tool_failure(
        &self,
        completion: FailureCompletion<'_, '_>,
        raw_text: String,
        message: &str,
    ) -> ProviderFailure {
        let failure = ProviderFailure::new(
            &self.config.id,
            completion.model,
            ProviderFailureClass::InvalidOutput,
            false,
            message,
        );
        completion.call.finish_failure(
            &failure,
            crate::provider::transcript::TranscriptFailureDetails::default()
                .with_transport_response(completion.transcript_response)
                .with_output(
                    Some(raw_text),
                    completion.finish_reason,
                    Some(completion.usage),
                )
                .discarded(),
        );
        failure
    }

    pub(super) fn finish_tool_success(
        &self,
        completion: FailureCompletion<'_, '_>,
        request_id: Option<String>,
        output: ToolSuccess,
    ) -> ProviderResponse {
        let response = ProviderResponse {
            provider_id: self.config.id.clone(),
            model: completion.model.to_string(),
            provider_request_id: request_id,
            finish_reason: completion.finish_reason,
            structured_output: output.structured_output,
            usage: completion.usage,
            capability_observations: vec![output.capability.to_string()],
            native_assistant_output: output.native_assistant_output,
        };
        completion.call.finish_ok(
            &response,
            completion.transcript_response,
            Some(output.raw_text),
            Vec::new(),
            false,
        );
        response
    }
}

fn native_turn(
    parsed: &crate::provider::native_tool_adapter::ParsedToolOutput,
    tools: &[crate::ToolSpec],
) -> serde_json::Value {
    serde_json::to_value(NativeToolTurn {
        text: (!parsed.raw_text.is_empty()).then(|| parsed.raw_text.clone()),
        tool_calls: parsed
            .calls
            .iter()
            .map(|call| {
                let validation_error = tools
                    .iter()
                    .find(|spec| spec.name == call.tool_name)
                    .map_or_else(
                        || Some("tool is not available in this turn".to_string()),
                        |spec| {
                            validate_schema(&call.arguments, &spec.parameters)
                                .err()
                                .map(|error| error.to_string())
                        },
                    );
                NativeToolCall::new(
                    &call.tool_call_id,
                    &call.tool_name,
                    call.arguments.clone(),
                    &call.raw_arguments,
                )
                .with_validation_error(validation_error)
            })
            .collect(),
    })
    .expect("native tool turn serializes")
}

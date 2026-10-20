use std::time::{SystemTime, UNIX_EPOCH};

use auditor_core::sha256_digest_bytes;
use serde_json::{json, Value};

use super::adapters::BuiltinAdapter;
use super::{
    CallDispatch, CaseQualificationBinding, CaseQualificationRecord, CaseTool, HostedConsent,
    NativeConversationMessage, NativeToolTurn, ProviderConfig, ProviderFailure,
    ProviderFailureClass, ProviderRequest, ProviderResponse, RouteBudget,
    SemanticQualificationCase, ToolChoice, TranscriptRecorder, Transport,
    MAX_NO_MODEL_OUTPUT_RETRIES, REVIEW_QUALIFICATION_V2_SCHEMA_VERSION,
};

pub const CASE_ROLE_TOOL_QUALIFICATION_STAGE: &str = "case_role_tool_qualification";
pub const CASE_SEMANTIC_QUALIFICATION_STAGE: &str = "case_semantic_qualification";
const NATIVE_ARRAY_MARKERS: [&str; 2] = ["native-array-one", "native-array-two"];
const QUALIFICATION_INITIAL_PROBE_ID: &str =
    "case-probe:0000000000000000000000000000000000000000000000000000000000000001";
const QUALIFICATION_CONTINUATION_PROBE_ID: &str =
    "case-probe:0000000000000000000000000000000000000000000000000000000000000003";
const QUALIFICATION_EVIDENCE_ID: &str =
    "source-span:0000000000000000000000000000000000000000000000000000000000000002";
const MAX_QUALIFICATION_PROTOCOL_ATTEMPTS: usize = 3;
#[derive(Clone, Copy)]
pub struct CaseQualificationRuntime<'a> {
    pub transport: &'a dyn Transport,
    pub consent: HostedConsent,
    pub budget: RouteBudget,
    pub transcript: &'a TranscriptRecorder,
    pub transcript_enabled: bool,
}

pub fn qualify_case_target(
    provider: &ProviderConfig,
    binding: CaseQualificationBinding,
    runtime: CaseQualificationRuntime<'_>,
) -> CaseQualificationRecord {
    let nonce = format!("ea-case-qualification:{}", binding.qualification_id());
    let mut passed = Vec::new();
    let mut failure_class = None;
    let mut failure_detail = None;
    let (native_protocol_valid, mut transport_valid) =
        match qualify_case_tool_with_protocol_retries(provider, &binding, &nonce, runtime) {
            Ok(()) => (true, true),
            Err(failure) => {
                failure_class = Some(failure.class);
                failure_detail = Some(failure.message);
                (false, provider_output_was_reached(failure.class))
            }
        };
    if native_protocol_valid {
        for case in SemanticQualificationCase::ALL {
            let expected = json!({
                "classification": case,
                "nonce": nonce,
                "native_array_markers": NATIVE_ARRAY_MARKERS,
            });
            let context = json!({
                "case_kind": binding.case_kind,
                "nonce": nonce,
                "role": binding.role,
                "stimulus": semantic_stimulus(case),
                "native_array_markers": NATIVE_ARRAY_MARKERS,
            });
            let request = ProviderRequest {
                request_id: format!(
                    "{}:semantic-{}",
                    binding.qualification_id(),
                    semantic_case_number(case)
                ),
                stage: CASE_SEMANTIC_QUALIFICATION_STAGE.to_string(),
                system_instruction: semantic_instruction().to_string(),
                context: Value::Null,
                context_markdown: Some(qualification_markdown(
                    "Semantic qualification case",
                    &context,
                )),
                output_schema_name: "case_semantic_qualification".to_string(),
                output_schema: qualification_schema(&nonce),
                max_input_bytes: runtime.budget.max_input_bytes,
                max_output_tokens: runtime.budget.max_output_tokens,
                timeout_ms: runtime.budget.timeout_ms,
            };
            let dispatch = CallDispatch::named_tool_probe(
                CaseTool::EmitQualificationNonce.spec(true),
                ToolChoice::Specific(CaseTool::EmitQualificationNonce.name()),
            );
            let adapter = BuiltinAdapter::new(provider, runtime.transport, runtime.consent);
            let result = execute_qualification_call(
                &adapter,
                &binding.target.model,
                &request,
                &dispatch,
                runtime,
            );
            match result {
                Ok(response) if response.structured_output == expected => passed.push(case),
                Ok(_) => {
                    failure_class = Some(ProviderFailureClass::InvalidOutput);
                    failure_detail = Some(format!(
                        "semantic qualification case `{case:?}` did not return its required classification and native array markers"
                    ));
                    break;
                }
                Err(failure) => {
                    transport_valid = provider_output_was_reached(failure.class);
                    failure_class = Some(failure.class);
                    failure_detail = Some(failure.message);
                    break;
                }
            }
        }
    }
    let semantic_output_valid = passed == SemanticQualificationCase::ALL;
    CaseQualificationRecord {
        schema_version: REVIEW_QUALIFICATION_V2_SCHEMA_VERSION,
        qualification_id: binding.qualification_id(),
        binding,
        transport_valid,
        native_protocol_valid,
        semantic_output_valid,
        semantic_cases_passed: passed,
        failure_class,
        failure_detail,
        qualified_at_unix_seconds: now_seconds(),
    }
}

fn qualify_case_tool_with_protocol_retries(
    provider: &ProviderConfig,
    binding: &CaseQualificationBinding,
    nonce: &str,
    runtime: CaseQualificationRuntime<'_>,
) -> Result<(), ProviderFailure> {
    for attempt in 0..MAX_QUALIFICATION_PROTOCOL_ATTEMPTS {
        match qualify_case_tool(provider, binding, nonce, runtime, attempt) {
            Err(failure)
                if failure.class == ProviderFailureClass::InvalidOutput
                    && attempt + 1 < MAX_QUALIFICATION_PROTOCOL_ATTEMPTS => {}
            result => return result,
        }
    }
    Err(ProviderFailure::new(
        &provider.id,
        &binding.target.model,
        ProviderFailureClass::Configuration,
        false,
        "qualification protocol attempt budget was empty",
    ))
}

/// A dispatch that requires one `answer_case_probe` call with `schema`.
fn answer_probe_dispatch(schema: Value) -> CallDispatch<'static> {
    let mut spec = CaseTool::AnswerCaseProbe.spec(true);
    spec.parameters = schema;
    CallDispatch::named_tool_probe(spec, ToolChoice::Specific(CaseTool::AnswerCaseProbe.name()))
}

fn is_single_probe_answer(calls: &[crate::NativeToolCall]) -> bool {
    matches!(calls, [call] if call.name == CaseTool::AnswerCaseProbe.name())
}

fn invalid_output(
    provider: &ProviderConfig,
    binding: &CaseQualificationBinding,
    detail: &str,
) -> ProviderFailure {
    ProviderFailure::new(
        &provider.id,
        &binding.target.model,
        ProviderFailureClass::InvalidOutput,
        false,
        detail,
    )
}

fn qualify_case_tool(
    provider: &ProviderConfig,
    binding: &CaseQualificationBinding,
    nonce: &str,
    runtime: CaseQualificationRuntime<'_>,
    attempt: usize,
) -> Result<(), ProviderFailure> {
    let request_prefix = if attempt == 0 {
        binding.qualification_id().to_string()
    } else {
        format!("{}:protocol-retry-{attempt}", binding.qualification_id())
    };
    let expected = qualification_probe_output(nonce, QUALIFICATION_INITIAL_PROBE_ID);
    let schema = qualification_probe_schema(nonce, QUALIFICATION_INITIAL_PROBE_ID);
    let request = ProviderRequest {
        request_id: format!("{request_prefix}:tool"),
        stage: CASE_ROLE_TOOL_QUALIFICATION_STAGE.to_string(),
        system_instruction: "For every assistant turn in this qualification conversation, call answer_case_probe exactly once with the fields supplied for that turn. Preserve evidence_ids and limitations as native arrays. Use the native tool call; do not write JSON, encode arrays as strings, or substitute prose."
            .to_string(),
        context: Value::Null,
        context_markdown: Some(qualification_markdown(
            "Production-shaped native tool qualification",
            &expected,
        )),
        output_schema_name: "case_role_tool_qualification".to_string(),
        output_schema: schema.clone(),
        max_input_bytes: runtime.budget.max_input_bytes,
        max_output_tokens: runtime.budget.max_output_tokens,
        timeout_ms: runtime.budget.timeout_ms,
    };
    let dispatch = answer_probe_dispatch(schema.clone());
    let call_context = QualificationCallContext {
        provider,
        model: &binding.target.model,
        runtime,
    };
    let first_turn = run_qualification_turn(
        &call_context,
        &request,
        &dispatch,
        "native qualification did not preserve the assistant tool-call turn",
    )?;
    if !is_single_probe_answer(&first_turn.tool_calls) {
        return Err(invalid_output(
            provider,
            binding,
            "native qualification emitted an unexpected tool-call set",
        ));
    }
    let call = &first_turn.tool_calls[0];
    let continuation_expected =
        qualification_probe_output(nonce, QUALIFICATION_CONTINUATION_PROBE_ID);
    let continuation_schema =
        qualification_probe_schema(nonce, QUALIFICATION_CONTINUATION_PROBE_ID);
    let continuation = ProviderRequest {
        request_id: format!("{request_prefix}:tool-continuation"),
        output_schema: continuation_schema.clone(),
        ..request
    };
    let dispatch = answer_probe_dispatch(continuation_schema).with_conversation(vec![
        NativeConversationMessage::Assistant {
            text: first_turn.text,
            tool_calls: first_turn.tool_calls.clone(),
        },
        NativeConversationMessage::ToolResult {
            call_id: call.call_id.clone(),
            tool_name: call.name.clone(),
            content_markdown: format!(
                "## Accepted\n\nThe first production-shaped native tool call was recorded. Answer the next distinct probe below with `answer_case_probe`.\n\n{}",
                qualification_markdown(
                    "Next production-shaped native tool call",
                    &continuation_expected,
                )
            ),
        },
    ]);
    let second_turn = run_qualification_turn(
        &call_context,
        &continuation,
        &dispatch,
        "native qualification did not preserve the tool-result continuation",
    )?;
    if is_single_probe_answer(&second_turn.tool_calls) {
        Ok(())
    } else {
        Err(invalid_output(
            provider,
            binding,
            "native qualification continuation emitted an unexpected tool-call set",
        ))
    }
}

struct QualificationCallContext<'a> {
    provider: &'a ProviderConfig,
    model: &'a str,
    runtime: CaseQualificationRuntime<'a>,
}

fn run_qualification_turn(
    context: &QualificationCallContext<'_>,
    request: &ProviderRequest,
    dispatch: &CallDispatch,
    invalid_message: &str,
) -> Result<NativeToolTurn, ProviderFailure> {
    let response = execute_qualification_call(
        &BuiltinAdapter::new(
            context.provider,
            context.runtime.transport,
            context.runtime.consent,
        ),
        context.model,
        request,
        dispatch,
        context.runtime,
    )?;
    decode_qualification_turn(
        response.structured_output,
        context.provider,
        context.model,
        invalid_message,
    )
}

fn decode_qualification_turn(
    output: Value,
    provider: &ProviderConfig,
    model: &str,
    message: &str,
) -> Result<NativeToolTurn, ProviderFailure> {
    serde_json::from_value(output).map_err(|_| {
        ProviderFailure::new(
            &provider.id,
            model,
            ProviderFailureClass::InvalidOutput,
            false,
            message,
        )
    })
}

fn qualification_probe_output(nonce: &str, probe_id: &str) -> Value {
    json!({
        "answer": "unknown",
        "evidence_ids": [QUALIFICATION_EVIDENCE_ID],
        "explanation": nonce,
        "limitations": NATIVE_ARRAY_MARKERS,
        "probe_id": probe_id,
    })
}

fn qualification_probe_schema(nonce: &str, probe_id: &str) -> Value {
    let mut schema = CaseTool::AnswerCaseProbe.parameters();
    schema["properties"]["probe_id"] = json!({"const": probe_id, "type": "string"});
    schema["properties"]["answer"] = json!({"const": "unknown", "type": "string"});
    schema["properties"]["evidence_ids"] = json!({
        "const": [QUALIFICATION_EVIDENCE_ID],
        "items": {"const": QUALIFICATION_EVIDENCE_ID, "type": "string"},
        "maxItems": 1,
        "minItems": 1,
        "type": "array"
    });
    schema["properties"]["explanation"] = json!({"const": nonce, "type": "string"});
    schema["properties"]["limitations"] = json!({
        "const": NATIVE_ARRAY_MARKERS,
        "items": {
            "enum": NATIVE_ARRAY_MARKERS,
            "type": "string"
        },
        "maxItems": 2,
        "minItems": 2,
        "type": "array",
        "uniqueItems": true
    });
    schema
}

fn qualification_markdown(title: &str, context: &Value) -> String {
    let digest = sha256_digest_bytes(
        &serde_json::to_vec(context).expect("qualification context serializes"),
    );
    crate::render_structured_model_document(title, context, digest, &[])
        .expect("qualification Markdown rendering is infallible")
        .markdown
}

fn execute_qualification_call(
    adapter: &BuiltinAdapter<'_>,
    model: &str,
    request: &ProviderRequest,
    dispatch: &CallDispatch<'_>,
    runtime: CaseQualificationRuntime<'_>,
) -> Result<ProviderResponse, ProviderFailure> {
    let mut retries_remaining = MAX_NO_MODEL_OUTPUT_RETRIES;
    loop {
        match adapter.execute_dispatched(
            model,
            request,
            dispatch,
            runtime.transcript,
            runtime.transcript_enabled,
        ) {
            Err(failure) if retries_remaining > 0 && failure.retryable_without_model_output => {
                let retry_number = MAX_NO_MODEL_OUTPUT_RETRIES - retries_remaining;
                retries_remaining -= 1;
                super::wait_before_no_model_output_retry(retry_number);
            }
            result => return result,
        }
    }
}

fn qualification_schema(nonce: &str) -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "classification": {
                "enum": SemanticQualificationCase::ALL,
                "type": "string"
            },
            "nonce": {"const": nonce, "type": "string"}
            ,
            "native_array_markers": {
                "const": NATIVE_ARRAY_MARKERS,
                "type": "array"
            }
        },
        "required": ["classification", "nonce", "native_array_markers"],
        "type": "object"
    })
}

const fn semantic_instruction() -> &'static str {
    "Classify the bounded lifecycle stimulus using only its evidence. The stimulus decision contract is closed-world and complete: apply its classification precedence and do not add unlisted requirements. Use emit_qualification_nonce as a native tool call with the supplied nonce, classification, and native_array_markers preserved as an array. Choose positive when the evidence substantiates the proposed in-case action, negative when it disproves it, unknown when required evidence is absent, conflicting_evidence when authoritative evidence disagrees, or forbidden_redesign when the only proposed action crosses the stated case boundary. Do not infer missing facts, propose another action, write JSON, or substitute prose for the tool call."
}

fn semantic_stimulus(case: SemanticQualificationCase) -> Value {
    let mut stimulus = match case {
        SemanticQualificationCase::Positive => json!({
            "qualification_case_id": "semantic-boundary-1",
            "decision_question": "Does the evidence substantiate the bounded shared-lifecycle action?",
            "case_boundary": "Ownership may move only within the existing component; public behavior must remain unchanged.",
            "proposed_action": "Reuse one process-lifetime descriptor for the two construction sites.",
            "unobserved_predicate_ids": [],
            "evidence": [
                {"predicate_id": "boundary_compliance", "source": "component_scope", "fact": "Both construction sites and the proposed process-lifetime owner are inside the same existing component; reuse leaves public behavior unchanged."},
                {"predicate_id": "input_equivalence", "source": "constructor_contract", "fact": "Both construction sites receive byte-equivalent inputs."},
                {"predicate_id": "post_construction_immutability", "source": "mutation_trace", "fact": "The descriptor is immutable after construction."},
                {"predicate_id": "identity_unobservability", "source": "consumer_contract", "fact": "Consumers use value semantics and do not observe instance identity."},
                {"predicate_id": "lifetime_compatibility", "source": "lifecycle_trace", "fact": "Both descriptors span the same process lifetime."}
            ]
        }),
        SemanticQualificationCase::Negative => json!({
            "qualification_case_id": "semantic-boundary-2",
            "decision_question": "Does the evidence substantiate the bounded shared-lifecycle action?",
            "case_boundary": "Decide only whether the two existing in-component construction sites may share one descriptor; no ownership move or API redesign is proposed.",
            "proposed_action": "Reuse one descriptor at the two existing construction sites.",
            "unobserved_predicate_ids": [],
            "evidence": [
                {"predicate_id": "input_equivalence", "source": "constructor_contract", "fact": "Both construction sites initially receive byte-equivalent inputs."},
                {"predicate_id": "post_construction_immutability", "source": "mutation_trace", "fact": "Each descriptor is mutated with construction-site-specific state after construction."},
                {"predicate_id": "identity_unobservability", "source": "consumer_contract", "fact": "Consumers distinguish the two descriptor identities."},
                {"predicate_id": "lifetime_compatibility", "source": "lifecycle_trace", "fact": "Each descriptor ends with its construction site's request."}
            ]
        }),
        SemanticQualificationCase::Unknown => json!({
            "qualification_case_id": "semantic-boundary-3",
            "decision_question": "Does the evidence substantiate the bounded shared-lifecycle action?",
            "case_boundary": "Observable behavior must remain unchanged.",
            "proposed_action": "Reuse one descriptor for both construction sites.",
            "unobserved_predicate_ids": [
                "post_construction_immutability",
                "identity_unobservability",
                "lifetime_compatibility"
            ],
            "evidence": [
                {"predicate_id": "input_equivalence", "source": "cardinality_trace", "fact": "Two descriptors are constructed from byte-equivalent inputs."},
                {"source": "coverage_record", "fact": "Mutation, consumer identity, and lifetime evidence was not collected."}
            ]
        }),
        SemanticQualificationCase::ConflictingEvidence => json!({
            "qualification_case_id": "semantic-boundary-4",
            "decision_question": "Does the evidence substantiate the bounded shared-lifecycle action?",
            "case_boundary": "The proposed in-component reuse is within the design boundary; classify the evidence state before authorizing it.",
            "proposed_action": "Reuse one process-lifetime descriptor.",
            "unobserved_predicate_ids": [
                "input_equivalence",
                "identity_unobservability",
                "lifetime_compatibility"
            ],
            "evidence": [
                {"predicate_id": "post_construction_immutability", "source": "declared_contract", "authority": "authoritative", "fact": "The descriptor is immutable and process-scoped."},
                {"predicate_id": "post_construction_immutability", "source": "instrumented_runtime", "authority": "authoritative", "fact": "The descriptor is mutated and disposed per request."}
            ]
        }),
        SemanticQualificationCase::ForbiddenRedesign => json!({
            "qualification_case_id": "semantic-boundary-5",
            "decision_question": "How should the proposed action be classified under the case boundary?",
            "case_boundary": "Do not change the public API, storage model, or ownership outside the named component.",
            "proposed_action": "Replace the public API and move ownership into a new cross-service registry.",
            "unobserved_predicate_ids": [
                "input_equivalence",
                "post_construction_immutability",
                "identity_unobservability",
                "lifetime_compatibility"
            ],
            "evidence": [
                {"source": "feasibility_probe", "fact": "The proposed savings require the public API and cross-service ownership changes."},
                {"source": "alternative_probe", "fact": "No action confined to the named component has been established."}
            ]
        }),
    };
    stimulus
        .as_object_mut()
        .expect("semantic qualification stimuli are objects")
        .insert(
            "decision_contract".to_string(),
            semantic_decision_contract(),
        );
    stimulus
}

fn semantic_decision_contract() -> Value {
    json!({
        "closed_world": true,
        "completeness": "Only the named lifecycle predicates and case boundary determine this synthetic classification. Unlisted conditions must not be added.",
        "required_predicates": {
            "input_equivalence": "Both construction sites receive equivalent inputs.",
            "post_construction_immutability": "Shared state is not mutated after construction.",
            "identity_unobservability": "Consumers do not depend on distinct instance identity.",
            "lifetime_compatibility": "The existing instances have compatible lifetimes."
        },
        "precedence": [
            "forbidden_redesign",
            "conflicting_evidence",
            "negative",
            "unknown",
            "positive"
        ],
        "classification_rules": {
            "forbidden_redesign": "The proposed action crosses the stated case boundary.",
            "conflicting_evidence": "Authoritative evidence disagrees about a required predicate.",
            "negative": "At least one required predicate is contradicted.",
            "unknown": "No required predicate is contradicted, but at least one is unobserved.",
            "positive": "The action stays inside the boundary and every required predicate is substantiated."
        }
    })
}

const fn semantic_case_number(case: SemanticQualificationCase) -> u8 {
    match case {
        SemanticQualificationCase::Positive => 1,
        SemanticQualificationCase::Negative => 2,
        SemanticQualificationCase::Unknown => 3,
        SemanticQualificationCase::ConflictingEvidence => 4,
        SemanticQualificationCase::ForbiddenRedesign => 5,
    }
}

const fn provider_output_was_reached(failure: ProviderFailureClass) -> bool {
    matches!(
        failure,
        ProviderFailureClass::Refusal
            | ProviderFailureClass::Truncated
            | ProviderFailureClass::InvalidOutput
    )
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_semantic_stimulus_substantiates_its_case_boundary() {
        let stimulus = semantic_stimulus(SemanticQualificationCase::Positive);
        let boundary_fact = stimulus["evidence"]
            .as_array()
            .expect("positive semantic evidence is an array")
            .iter()
            .find(|item| item["predicate_id"] == "boundary_compliance")
            .expect("positive semantic evidence includes boundary compliance");

        assert_eq!(boundary_fact["source"], "component_scope");
        assert!(boundary_fact["fact"]
            .as_str()
            .is_some_and(|fact| fact.contains("same existing component")
                && fact.contains("public behavior unchanged")));
    }
}

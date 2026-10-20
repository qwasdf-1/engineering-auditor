use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use auditor_review::{
    inspect_engineering_case, plan_engineering_review, run_case_engineering_review,
    verify_case_engineering_review, CaseBudgetPolicy, DeliberationMode, EnsembleRouteChains,
    HostedConsent, ProviderConfig, ResolverKind, ReviewPlanRequest, ReviewRoutesV2,
    ReviewRunRequest, ReviewVerifyRequest, RouteBudget, RouteTarget, StableId, Transport,
    TransportRequest, TransportResponse, UncontrolledReview, V2ProviderChain,
    REVIEW_ROUTES_V2_SCHEMA_VERSION, TOOL_EMIT_QUALIFICATION_NONCE,
};

use super::*;

struct ScriptedCaseTransport {
    finding_id: String,
    unresolved_probe_ids: Vec<StableId>,
    claim_slot: auditor_review::ClaimSlot,
    recommendation_premise_probe_id: StableId,
    recommendation_premise_slot: auditor_review::ClaimSlot,
    invalid_model_probe: bool,
    invalid_host_claim_once: bool,
    invalid_tool_schema_once: bool,
    partial_invalid_tool_schema_once: bool,
    invalid_host_correction_schema_once: bool,
    split_valid_progress_once: bool,
    mixed_certainty_boundary_audit_once: bool,
    ensemble_split: bool,
    transient_review_failures: usize,
    calls: AtomicUsize,
    review_turns: AtomicUsize,
    host_correction_turns: AtomicUsize,
    boundary_audit_turns: AtomicUsize,
    recommendation_audit_turns: AtomicUsize,
    correction_errors: Mutex<Vec<String>>,
    offered_case_tools: Mutex<Vec<Vec<String>>>,
    stages: Mutex<Vec<String>>,
    timeouts: Mutex<Vec<u64>>,
    case_contexts: Mutex<Vec<(bool, String)>>,
}

impl ScriptedCaseTransport {
    fn new(plan: &auditor_review::ReviewPlanOutput) -> Self {
        let unresolved_probe_ids: Vec<StableId> = plan.cases[0]
            .deterministic_probe_results
            .iter()
            .filter(|result| result.resolver.kind == ResolverKind::UnknownFallback)
            .map(|result| result.probe_id.clone())
            .collect();
        let (recommendation_premise_probe_id, recommendation_premise_slot) = unresolved_probe_ids
            .iter()
            .find_map(|probe_id| {
                plan.cases[0]
                    .probe_definitions
                    .iter()
                    .find(|definition| definition.probe_id == *probe_id)
                    .and_then(|definition| definition.blocked_claim_slots.first().copied())
                    .map(|slot| (probe_id.clone(), slot))
            })
            .expect("scripted review has a claim-blocking unresolved probe");
        Self {
            finding_id: plan.cases[0].case.finding_ids[0].clone(),
            unresolved_probe_ids,
            claim_slot: auditor_review::ClaimSlot::InputEquivalence,
            recommendation_premise_probe_id,
            recommendation_premise_slot,
            invalid_model_probe: false,
            invalid_host_claim_once: false,
            invalid_tool_schema_once: false,
            partial_invalid_tool_schema_once: false,
            invalid_host_correction_schema_once: false,
            split_valid_progress_once: false,
            mixed_certainty_boundary_audit_once: false,
            ensemble_split: false,
            transient_review_failures: 1,
            calls: AtomicUsize::new(0),
            review_turns: AtomicUsize::new(0),
            host_correction_turns: AtomicUsize::new(0),
            boundary_audit_turns: AtomicUsize::new(0),
            recommendation_audit_turns: AtomicUsize::new(0),
            correction_errors: Mutex::new(Vec::new()),
            offered_case_tools: Mutex::new(Vec::new()),
            stages: Mutex::new(Vec::new()),
            timeouts: Mutex::new(Vec::new()),
            case_contexts: Mutex::new(Vec::new()),
        }
    }

    fn with_invalid_model_probe(mut self) -> Self {
        self.invalid_model_probe = true;
        self
    }

    fn with_invalid_tool_schema_once(mut self) -> Self {
        self.invalid_tool_schema_once = true;
        self
    }

    fn with_partial_invalid_tool_schema_once(mut self) -> Self {
        self.partial_invalid_tool_schema_once = true;
        self
    }

    fn with_invalid_host_claim_once(mut self) -> Self {
        self.invalid_host_claim_once = true;
        self
    }

    fn with_invalid_host_correction_schema_once(mut self) -> Self {
        self.invalid_host_correction_schema_once = true;
        self
    }

    fn with_split_valid_progress_once(mut self) -> Self {
        self.split_valid_progress_once = true;
        self
    }

    fn with_mixed_certainty_boundary_audit_once(mut self) -> Self {
        self.mixed_certainty_boundary_audit_once = true;
        self
    }

    fn with_ensemble_split(mut self) -> Self {
        self.ensemble_split = true;
        self
    }

    fn with_transient_review_failures(mut self, failures: usize) -> Self {
        self.transient_review_failures = failures;
        self
    }

    fn measurement_recommendation_calls(&self) -> Vec<(&'static str, Value)> {
        vec![
            (
                auditor_review::TOOL_RECORD_CASE_CLAIM,
                json!({
                    "claim_ref": "measurement-premise",
                    "slot": self.recommendation_premise_slot,
                    "kind": "recommendation_premise",
                    "proposition": "The unresolved lifecycle boundary requires focused evidence before changing ownership.",
                    "status": "blocked",
                    "supporting_probe_ids": [self.recommendation_premise_probe_id],
                    "counter_probe_ids": [],
                    "dependency_claim_refs": [],
                    "assumptions": [],
                    "confidence": "medium"
                }),
            ),
            (
                auditor_review::TOOL_RECORD_CASE_RECOMMENDATION,
                json!({
                    "recommendation_ref": "measure-before-change",
                    "family": "measurement_before_change",
                    "summary": "Resolve the missing lifecycle evidence before changing ownership.",
                    "scope_ids": [self.finding_id],
                    "premise_claim_refs": ["measurement-premise"],
                    "affected_finding_ids": [self.finding_id],
                    "detector_advice_relationship": "narrows",
                    "preconditions": ["The missing behavior can be inspected or measured."],
                    "expected_benefits": [],
                    "risks": ["The measurement may confirm that the current ownership is required."],
                    "costs": ["Requires focused inspection or instrumentation."],
                    "alternatives": [],
                    "verification_steps": [{
                        "method": "Inspect or trace the unresolved lifecycle behavior.",
                        "expected_observation": "The evidence resolves the blocked lifecycle boundary.",
                        "evidence_probe_ids": [self.recommendation_premise_probe_id]
                    }],
                    "materiality": "low",
                    "materiality_basis": "Material impact is not established by the bounded evidence.",
                    "confidence": "medium"
                }),
            ),
        ]
    }
}

impl Transport for ScriptedCaseTransport {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let body: Value =
            serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
        let messages = body["messages"].as_array().cloned().unwrap_or_default();
        let user_messages = messages
            .iter()
            .filter_map(|message| {
                (message["role"].as_str() == Some("user"))
                    .then(|| message["content"].as_str())
                    .flatten()
            })
            .collect::<Vec<_>>();
        let correction = user_messages.iter().copied().rfind(|content| {
            content.contains("Reviewer task calls need correction")
                || content.contains("completed reviewer ledger failed semantic admission")
        });
        let accepted_progress = user_messages
            .iter()
            .any(|content| content.contains("Reviewer completion is still required"));
        let boundary_audit = user_messages
            .iter()
            .any(|content| content.contains("Reviewer evidence-boundary audit is required"));
        let recommendation_audit = user_messages
            .iter()
            .any(|content| content.contains("Reviewer recommendation coverage audit is required"));
        let host_correction = correction.is_some();
        if let Some(correction) = correction {
            self.correction_errors
                .lock()
                .unwrap()
                .push(correction.to_string());
        }
        self.timeouts.lock().unwrap().push(request.timeout_ms);
        let tool_names = body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect::<Vec<_>>();
        if tool_names.contains(&auditor_review::TOOL_COMPLETE_CASE_CHALLENGE) {
            return Ok(tool_response(
                auditor_review::TOOL_COMPLETE_CASE_CHALLENGE,
                json!({}),
            ));
        }
        if let Some(record_recommendation) =
            body["tools"].as_array().into_iter().flatten().find(|tool| {
                tool["function"]["name"].as_str()
                    == Some(auditor_review::TOOL_RECORD_CASE_RECOMMENDATION)
            })
        {
            let allowed = &record_recommendation["function"]["parameters"]["properties"]
                ["affected_finding_ids"]["items"]["enum"];
            if allowed != &json!([self.finding_id]) {
                return Err(format!(
                    "affected_finding_ids is not bound to the admitted case: {allowed}"
                ));
            }
        }
        let model_context = body["messages"][1]["content"].as_str().unwrap_or_default();
        let production_shaped_qualification =
            model_context.starts_with("# Production-shaped native tool qualification");
        let name = if tool_names.contains(&TOOL_EMIT_QUALIFICATION_NONCE)
            || production_shaped_qualification
        {
            "case_qualification"
        } else if tool_names.contains(&auditor_review::TOOL_ANSWER_CASE_PROBE)
            || tool_names.contains(&auditor_review::TOOL_REQUEST_CASE_EVIDENCE)
        {
            "case_task_calls"
        } else {
            "unknown"
        };
        self.stages.lock().unwrap().push(name.to_string());
        if let Some(response) = scripted_qualification(&body) {
            return response;
        }
        if name == "case_task_calls" {
            let shared_reconsideration = user_messages.iter().any(|content| {
                content.contains("resolved the blind reviewers' allowed evidence union")
            });
            self.case_contexts
                .lock()
                .unwrap()
                .push((shared_reconsideration, model_context.to_string()));
            if shared_reconsideration {
                return Ok(tool_response(
                    auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                    }),
                ));
            }
            if body["messages"][0]["content"]
                .as_str()
                .is_some_and(|instruction| instruction.contains("Continue the same case task"))
            {
                return Err("follow-up notes must not rewrite the system instruction".to_string());
            }
            if accepted_progress
                && user_messages
                    .last()
                    .is_some_and(|note| note.contains("failed host admission"))
            {
                return Err(
                    "accepted-progress continuation was mislabeled as failed admission".to_string(),
                );
            }
            self.offered_case_tools
                .lock()
                .unwrap()
                .push(tool_names.iter().map(|name| (*name).to_string()).collect());
            let review_turn = self.review_turns.fetch_add(1, Ordering::Relaxed);
            let host_correction_turn =
                host_correction.then(|| self.host_correction_turns.fetch_add(1, Ordering::Relaxed));
            if review_turn < self.transient_review_failures {
                return Ok(TransportResponse {
                    status: 500,
                    body: br#"{"error":{"type":"server_error"}}"#.to_vec(),
                });
            }
            if review_turn == 1 && self.invalid_tool_schema_once {
                return Ok(tool_response(
                    auditor_review::TOOL_ANSWER_CASE_PROBE,
                    json!({
                        "probe_id": self.unresolved_probe_ids[0],
                        "answer": "outside_contract",
                        "evidence_ids": [],
                        "explanation": "Invalid schema fixture.",
                        "limitations": []
                    }),
                ));
            }
            if review_turn == 1 && self.partial_invalid_tool_schema_once {
                return Ok(tool_calls_response(vec![
                    (
                        auditor_review::TOOL_ANSWER_CASE_PROBE,
                        json!({
                            "probe_id": self.unresolved_probe_ids[0],
                            "answer": "unknown",
                            "evidence_ids": [],
                            "explanation": "The bounded packet does not establish this probe.",
                            "limitations": ["No admissible deterministic evidence resolved the probe."]
                        }),
                    ),
                    (
                        auditor_review::TOOL_ANSWER_CASE_PROBE,
                        json!({
                            "probe_id": self.unresolved_probe_ids[1],
                            "answer": "outside_contract",
                            "evidence_ids": [],
                            "explanation": "Invalid sibling call fixture.",
                            "limitations": []
                        }),
                    ),
                ]));
            }
            if self.invalid_host_correction_schema_once && host_correction_turn == Some(0) {
                return Ok(tool_response(
                    auditor_review::TOOL_WITHDRAW_CASE_CLAIM,
                    json!({
                        "claim_ref": "invalid-unknown-claim",
                        "expected_revision": "not-an-integer",
                        "reason": "Invalid schema fixture."
                    }),
                ));
            }
            if self.invalid_host_claim_once && host_correction {
                return Ok(tool_calls_response(vec![
                    (
                        auditor_review::TOOL_WITHDRAW_CASE_CLAIM,
                        json!({
                            "claim_ref": "invalid-unknown-claim",
                            "expected_revision": 1,
                            "reason": "Unknown evidence cannot support a negative conclusion."
                        }),
                    ),
                    (
                        auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                        json!({
                            "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                        }),
                    ),
                ]));
            }
            if self.split_valid_progress_once && accepted_progress {
                return Ok(tool_response(
                    auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                    }),
                ));
            }
            if self.mixed_certainty_boundary_audit_once && recommendation_audit {
                self.recommendation_audit_turns
                    .fetch_add(1, Ordering::Relaxed);
                let mut calls = self.measurement_recommendation_calls();
                calls.push((
                    auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                    }),
                ));
                return Ok(tool_calls_response(calls));
            }
            if self.mixed_certainty_boundary_audit_once
                && boundary_audit
                && self.boundary_audit_turns.load(Ordering::Relaxed) == 0
            {
                self.boundary_audit_turns.fetch_add(1, Ordering::Relaxed);
                return Ok(tool_calls_response(vec![
                    (
                        auditor_review::TOOL_REVISE_CASE_PROBE_ANSWER,
                        json!({
                            "probe_id": self.unresolved_probe_ids[0],
                            "expected_revision": 1,
                            "reason": "The earlier definitive answer relied only on possibility, so the authoritative boundary remains unresolved.",
                            "replacement": {
                                "probe_id": self.unresolved_probe_ids[0],
                                "answer": "unknown",
                                "evidence_ids": [],
                                "explanation": "The bounded packet does not establish either probe boundary.",
                                "limitations": ["Capability alone does not establish runtime behavior."]
                            }
                        }),
                    ),
                    (
                        auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                        json!({
                            "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                        }),
                    ),
                ]));
            }
            let unresolved_probe_ids = if self.partial_invalid_tool_schema_once && host_correction {
                &self.unresolved_probe_ids[1..]
            } else {
                self.unresolved_probe_ids.as_slice()
            };
            let mut calls = unresolved_probe_ids
                .iter()
                .enumerate()
                .map(|(index, probe_id)| {
                    (
                        auditor_review::TOOL_ANSWER_CASE_PROBE,
                        json!({
                            "probe_id": probe_id,
                            "answer": if self.mixed_certainty_boundary_audit_once && index == 0 { "yes" } else { "unknown" },
                            "evidence_ids": if self.invalid_model_probe {
                                vec!["source-span:outside-packet"]
                            } else {
                                Vec::new()
                            },
                            "explanation": "The bounded packet does not establish this probe.",
                            "limitations": ["No admissible deterministic evidence resolved the probe."]
                        }),
                    )
                })
                .collect::<Vec<_>>();
            if self.invalid_host_claim_once && review_turn == 1 {
                calls.push((
                    auditor_review::TOOL_RECORD_CASE_CLAIM,
                    json!({
                        "claim_ref": "invalid-unknown-claim",
                        "slot": self.claim_slot,
                        "kind": "root_cause",
                        "proposition": "Unknown evidence was incorrectly treated as negative evidence.",
                        "status": "not_substantiated",
                        "supporting_probe_ids": [self.unresolved_probe_ids[0]],
                        "counter_probe_ids": [],
                        "dependency_claim_refs": [],
                        "assumptions": [],
                        "confidence": "low"
                    }),
                ));
            }
            let reviewer_c_split =
                self.ensemble_split && body["model"].as_str() == Some("free-model-c");
            if !self.mixed_certainty_boundary_audit_once {
                let mut measurement_calls = self.measurement_recommendation_calls();
                if reviewer_c_split {
                    measurement_calls[1].1["summary"] = json!(
                        "Resolve the missing lifecycle evidence with narrow runtime instrumentation."
                    );
                    measurement_calls[1].1["costs"] =
                        json!(["Requires narrow runtime instrumentation."]);
                }
                calls.extend(measurement_calls);
            }
            if !self.split_valid_progress_once {
                calls.push((
                    auditor_review::TOOL_COMPLETE_CASE_REVIEW,
                    json!({
                        "unresolved_gaps": ["Required lifecycle evidence remains unavailable."]
                    }),
                ));
            }
            return Ok(tool_calls_response(calls));
        }
        Err(format!("unexpected scripted output schema {name}"))
    }
}

#[test]
fn accepted_progress_continuation_is_not_recorded_as_a_repair() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_split_valid_progress_once();
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-accepted-progress.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();

    assert_eq!(artifact.provider_records.len(), 3);
    let initial = &artifact.provider_records[1];
    assert_eq!(initial.failure_class, None);
    assert_eq!(initial.repair_count, 0);
    assert_eq!(initial.call.stage, "case_reviewing");

    let continuation = &artifact.provider_records[2];
    assert_eq!(continuation.failure_class, None);
    assert_eq!(continuation.repair_count, 0);
    assert_eq!(continuation.call.stage, "case_review_continuation");
    assert!(continuation.call.request_id.ends_with(":continuation-1"));
    assert!(transport.correction_errors.lock().unwrap().is_empty());
}

#[test]
fn mixed_certainty_probe_ledger_gets_a_boundary_audit_before_admission() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_mixed_certainty_boundary_audit_once();
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-boundary-audit.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();

    assert_eq!(transport.boundary_audit_turns.load(Ordering::Relaxed), 1);
    assert_eq!(
        transport.recommendation_audit_turns.load(Ordering::Relaxed),
        1
    );
    assert!(artifact
        .probe_results
        .iter()
        .filter(|result| result.resolver.kind == ResolverKind::ModelInterpretation)
        .all(|result| result.answer == auditor_review::ProbeAnswer::Unknown));
    assert!(artifact
        .semantic_revisions
        .iter()
        .any(|revision| revision.reference == transport.unresolved_probe_ids[0].to_string()));
    assert!(artifact.provider_records.iter().any(|record| {
        record.call.stage == "case_review_continuation"
            && record.call.request_id.ends_with(":continuation-1")
            && record.failure_class.is_none()
    }));
}

/// The scripted answer to a role-qualification request (the
/// production-shaped native tool probe or the nonce stimulus), if `body` is
/// one.
pub(super) fn scripted_qualification(body: &Value) -> Option<Result<TransportResponse, String>> {
    let model_context = body["messages"][1]["content"].as_str().unwrap_or_default();
    if model_context.starts_with("# Production-shaped native tool qualification") {
        let properties = &body["tools"][0]["function"]["parameters"]["properties"];
        return Some(Ok(tool_response(
            auditor_review::TOOL_ANSWER_CASE_PROBE,
            json!({
                "answer": properties["answer"]["const"].clone(),
                "evidence_ids": properties["evidence_ids"]["const"].clone(),
                "explanation": properties["explanation"]["const"].clone(),
                "limitations": properties["limitations"]["const"].clone(),
                "probe_id": properties["probe_id"]["const"].clone(),
            }),
        )));
    }
    let nonce_tool = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|tool| tool["function"]["name"].as_str() == Some(TOOL_EMIT_QUALIFICATION_NONCE));
    nonce_tool.then(|| nonce_response(model_context))
}

fn nonce_response(model_context: &str) -> Result<TransportResponse, String> {
    let nonce = markdown_field(model_context, "nonce")
        .ok_or_else(|| "qualification nonce is absent".to_string())?;
    let classification = if let Some(stimulus_id) =
        markdown_field(model_context, "qualification_case_id")
    {
        match stimulus_id.as_str() {
            "semantic-boundary-1" => "positive",
            "semantic-boundary-2" => "negative",
            "semantic-boundary-3" => "unknown",
            "semantic-boundary-4" => "conflicting_evidence",
            "semantic-boundary-5" => "forbidden_redesign",
            other => return Err(format!("unknown semantic stimulus {other}")),
        }
        .to_string()
    } else {
        markdown_field(model_context, "classification").unwrap_or_else(|| "positive".to_string())
    };
    Ok(tool_response(
        TOOL_EMIT_QUALIFICATION_NONCE,
        json!({
            "classification": classification,
            "nonce": nonce,
            "native_array_markers": ["native-array-one", "native-array-two"],
        }),
    ))
}

pub(super) fn markdown_field(markdown: &str, field: &str) -> Option<String> {
    let marker = format!("**{field}:** ");
    markdown.lines().find_map(|line| {
        line.find(&marker)
            .map(|offset| line[offset + marker.len()..].trim().to_string())
    })
}

pub(super) fn tool_response(name: &str, arguments: Value) -> TransportResponse {
    tool_calls_response(vec![(name, arguments)])
}

pub(super) fn tool_calls_response(calls: Vec<(&str, Value)>) -> TransportResponse {
    TransportResponse {
        status: 200,
        body: serde_json::to_vec(&json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "tool_calls": calls.into_iter().enumerate().map(|(index, (name, arguments))| json!({
                        "id": format!("spec11-call-{index}"),
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&arguments).unwrap()
                        }
                    })).collect::<Vec<_>>()
                }
            }],
            "id": "spec11-scripted-tool",
            "usage": {"completion_tokens": 4, "prompt_tokens": 8}
        }))
        .unwrap(),
    }
}

fn write_v2_routes(fixture: &Fixture) -> PathBuf {
    let provider = ProviderConfig::opencode_anonymous(vec!["free-model".to_string()]);
    let chain = V2ProviderChain {
        targets: vec![RouteTarget {
            provider_id: provider.id.clone(),
            model: "free-model".to_string(),
        }],
    };
    let review_budget = RouteBudget {
        max_requests: 64,
        max_input_bytes: 8 * 1024 * 1024,
        max_output_tokens: 64 * 1024,
        timeout_ms: 60_000,
        max_cost_microusd: 0,
        allow_unknown_cost: false,
    };
    let routes = ReviewRoutesV2 {
        schema_version: REVIEW_ROUTES_V2_SCHEMA_VERSION,
        providers: vec![provider.clone()],
        single_reviewer: chain.clone(),
        ensemble: None,
        challenger: chain,
        alternative_comparator: None,
        review_budget,
        case_budgets: CaseBudgetPolicy {
            defaults: RouteBudget {
                max_requests: 8,
                max_input_bytes: 2 * 1024 * 1024,
                max_output_tokens: 16 * 1024,
                timeout_ms: 45_000,
                max_cost_microusd: 0,
                allow_unknown_cost: false,
            },
            per_template: Default::default(),
        },
    };
    let path = fixture.root.join("routes-v2.json");
    std::fs::write(path.as_path(), serde_json::to_vec_pretty(&routes).unwrap()).unwrap();
    path
}

fn write_ensemble_routes(fixture: &Fixture) -> PathBuf {
    write_ensemble_routes_for(fixture, "free-model", "ensemble-routes-v2.json")
}

fn write_split_ensemble_routes(fixture: &Fixture) -> PathBuf {
    write_ensemble_routes_for(fixture, "free-model-c", "split-ensemble-routes-v2.json")
}

fn write_ensemble_routes_for(fixture: &Fixture, reviewer_c_model: &str, name: &str) -> PathBuf {
    let mut models = vec!["free-model".to_string(), reviewer_c_model.to_string()];
    models.sort();
    models.dedup();
    let provider = ProviderConfig::opencode_anonymous(models);
    let chain = V2ProviderChain {
        targets: vec![RouteTarget {
            provider_id: provider.id.clone(),
            model: "free-model".to_string(),
        }],
    };
    let review_budget = RouteBudget {
        max_requests: 64,
        max_input_bytes: 8 * 1024 * 1024,
        max_output_tokens: 64 * 1024,
        timeout_ms: 60_000,
        max_cost_microusd: 0,
        allow_unknown_cost: false,
    };
    let routes = ReviewRoutesV2 {
        schema_version: REVIEW_ROUTES_V2_SCHEMA_VERSION,
        providers: vec![provider.clone()],
        single_reviewer: chain.clone(),
        ensemble: Some(EnsembleRouteChains {
            reviewer_a: chain.clone(),
            reviewer_b: chain.clone(),
            reviewer_c: V2ProviderChain {
                targets: vec![RouteTarget {
                    provider_id: provider.id.clone(),
                    model: reviewer_c_model.to_string(),
                }],
            },
        }),
        challenger: chain,
        alternative_comparator: None,
        review_budget,
        case_budgets: CaseBudgetPolicy {
            defaults: RouteBudget {
                max_requests: 16,
                max_input_bytes: 4 * 1024 * 1024,
                max_output_tokens: 16 * 1024,
                timeout_ms: 45_000,
                max_cost_microusd: 0,
                allow_unknown_cost: false,
            },
            per_template: Default::default(),
        },
    };
    let path = fixture.root.join(name);
    std::fs::write(path.as_path(), serde_json::to_vec_pretty(&routes).unwrap()).unwrap();
    path
}

#[test]
fn shadow_ensemble_runs_three_blind_passes_and_shared_reconsideration() {
    let fixture = Fixture::lifecycle_only();
    let mut request = request(&fixture);
    request.routes = write_ensemble_routes(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    assert_eq!(
        plan.cases[0].routing.selection.mode,
        DeliberationMode::Ensemble
    );
    let transport = ScriptedCaseTransport::new(&plan).with_transient_review_failures(0);
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture.report.with_file_name("reviewed-v2-ensemble.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(
        artifact.ensemble_runs.len(),
        1,
        "shadow failures: {:?}",
        artifact.ensemble_failures
    );
    assert!(artifact.ensemble_failures.is_empty());
    assert_eq!(artifact.ensemble_runs[0].pass_a.len(), 3);
    assert_eq!(artifact.ensemble_runs[0].pass_b.len(), 3);
    assert_eq!(
        artifact.ensemble_runs[0].independence,
        auditor_review::EnsembleIndependence::SameModelRepeated
    );
    assert_eq!(artifact.reviewer_runs.len(), 1);
    assert_eq!(
        artifact.execution_truth[0].reviewer_review_id.as_ref(),
        Some(&artifact.reviewer_runs[0].review_id)
    );

    let contexts = transport.case_contexts.lock().unwrap();
    let pass_a = contexts
        .iter()
        .filter(|(shared, _)| !shared)
        .map(|(_, context)| context)
        .collect::<Vec<_>>();
    let pass_b = contexts
        .iter()
        .filter(|(shared, _)| *shared)
        .map(|(_, context)| context)
        .collect::<Vec<_>>();
    assert_eq!(
        pass_a.len(),
        4,
        "one production plus three blind Pass A contexts"
    );
    assert!(pass_a.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(pass_b.len(), 3);
    assert!(pass_b.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn comparison_targets_drive_frozen_per_reviewer_shadow_challenges() {
    let fixture = Fixture::lifecycle_only();
    let mut request = request(&fixture);
    request.routes = write_split_ensemble_routes(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan)
        .with_transient_review_failures(0)
        .with_ensemble_split();
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-comparison-challenges.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(
        artifact.ensemble_runs.len(),
        1,
        "shadow failures: {:?}",
        artifact.ensemble_failures
    );
    assert_eq!(artifact.ensemble_challenges.len(), 3);
    assert!(artifact
        .ensemble_challenges
        .iter()
        .all(|execution| execution.outcome.is_some() && execution.failure_code.is_none()));
    assert!(artifact.ensemble_challenges.iter().all(|execution| {
        execution
            .outcome
            .as_ref()
            .is_some_and(|outcome| outcome.response.is_none())
    }));
}

pub(super) fn request(fixture: &Fixture) -> ReviewPlanRequest {
    ReviewPlanRequest {
        report: fixture.report.clone(),
        repo_root: fixture.repo.clone(),
        case_templates: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/auditor/review-case-templates-v1.json"),
        routes: write_v2_routes(fixture),
        prompt_bundle: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/auditor/review-prompt-bundle-v2.json"),
        brief: None,
        evidence: Vec::new(),
    }
}

#[test]
fn production_case_call_recovers_four_consecutive_no_output_failures() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_transient_review_failures(4);
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-two-retries.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(transport.review_turns.load(Ordering::Relaxed), 5);
    assert_eq!(artifact.provider_records.len(), 5);
    assert!(artifact.provider_records[..4].iter().all(|record| {
        record.failure_class == Some(auditor_review::ProviderFailureClass::Unavailable)
            && record
                .failure_detail
                .as_ref()
                .is_some_and(|detail| !detail.trim().is_empty())
    }));
    assert_eq!(artifact.provider_records[4].failure_class, None);
    assert_eq!(artifact.provider_records[4].failure_detail, None);
}

#[test]
fn production_case_call_stops_after_four_no_output_retries() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_transient_review_failures(5);
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-retry-exhausted.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();

    assert_eq!(transport.review_turns.load(Ordering::Relaxed), 5);
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.provider_records.len(), 5);
    assert!(artifact
        .provider_records
        .iter()
        .all(|record| record.failure_class
            == Some(auditor_review::ProviderFailureClass::Unavailable)));
    assert!(artifact.recommendations.is_empty());
}

#[test]
fn complete_source_omits_redundant_evidence_tool_and_preserves_source() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let initial_source_ids = plan.cases[0]
        .evidence_packet
        .items
        .iter()
        .filter(|item| item.kind == auditor_review::EvidenceItemKind::SourceSpan)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    assert!(!initial_source_ids.is_empty());
    let transport = ScriptedCaseTransport::new(&plan);
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-source-union.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert!(artifact.evidence_packet_history.is_empty());
    for source_id in initial_source_ids {
        assert!(artifact.evidence_packets[0]
            .items
            .iter()
            .any(|item| item.id == source_id));
    }
    assert!(transport
        .offered_case_tools
        .lock()
        .unwrap()
        .iter()
        .all(|tools| !tools
            .iter()
            .any(|tool| tool == auditor_review::TOOL_REQUEST_CASE_EVIDENCE)));
}

#[test]
fn plan_inspect_scripted_run_and_live_binding_verify_share_one_service() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let first = plan_engineering_review(&request).unwrap();
    let second = plan_engineering_review(&request).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.cases.len(), 1);
    assert_eq!(
        first.cases[0].routing.selection.mode,
        DeliberationMode::SingleReviewer
    );
    let inspection = inspect_engineering_case(&first, &first.cases[0].case.case_id).unwrap();
    assert_eq!(inspection.case.case_id, first.cases[0].case.case_id);
    let unknown =
        StableId::from_identity("engineering-case", 1, &json!({"unknown": true})).unwrap();
    assert!(inspect_engineering_case(&first, &unknown).is_err());

    let transport = ScriptedCaseTransport::new(&first);
    let output_path = fixture.report.with_file_name("reviewed-v2.json");
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    assert_eq!(output.case_count, 1);
    assert_eq!(output.blocked_case_count, 1);
    assert!(output.review_plan_path.is_file());
    assert!(output.review_artifact_path.is_file());
    assert!(output.reviewed_report_path.is_file());
    assert_eq!(transport.calls.load(Ordering::Relaxed), 16);
    assert!(transport
        .timeouts
        .lock()
        .unwrap()
        .iter()
        .all(|timeout| *timeout == 45_000));

    let verified = verify_case_engineering_review(&ReviewVerifyRequest {
        repo_root: fixture.repo.clone(),
        report: output_path,
    })
    .unwrap();
    assert!(verified.valid);
    assert_eq!(verified.review_id, output.review_id);
    assert_eq!(verified.artifact_digest, output.review_artifact_digest);
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert!(artifact.evidence_packet_history.is_empty());
    assert_eq!(artifact.provider_records.len(), 2);
    assert!(artifact
        .provider_records
        .iter()
        .all(|record| record.packet_digest == artifact.evidence_packets[0].packet_digest));
    assert_eq!(
        artifact.provider_records[0].failure_class,
        Some(auditor_review::ProviderFailureClass::Unavailable)
    );
    assert!(artifact
        .provider_records
        .iter()
        .all(|record| record.call.assessment_mode
            == auditor_review::AssessmentModeRecorded::NativeTools));
}

#[test]
fn model_context_is_decision_complete_without_host_only_identity_duplication() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let prompt_bundle: auditor_review::ReviewPromptBundleV2 =
        serde_json::from_slice(&std::fs::read(&request.prompt_bundle).unwrap()).unwrap();
    let plan = plan_engineering_review(&request).unwrap();
    let planned = &plan.cases[0];
    let resolved = planned
        .deterministic_probe_results
        .iter()
        .filter(|result| result.resolver.kind != ResolverKind::UnknownFallback)
        .cloned()
        .collect::<Vec<_>>();
    let resolved_ids = resolved
        .iter()
        .map(|result| &result.probe_id)
        .collect::<std::collections::BTreeSet<_>>();
    let unresolved = planned
        .probe_definitions
        .iter()
        .filter(|definition| !resolved_ids.contains(&definition.probe_id))
        .cloned()
        .collect::<Vec<_>>();
    let context = auditor_review::CaseProviderContext::compile(
        &planned.case,
        &planned.evidence_packet,
        &resolved,
        &unresolved,
    )
    .unwrap();
    let value = serde_json::to_value(&context).unwrap();
    assert_eq!(value["case_id"], json!(planned.case.case_id));
    assert!(value["evidence"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "source_span"));
    let recommendation_families = value["decision"]["recommendation_families"]
        .as_array()
        .unwrap();
    assert_eq!(
        recommendation_families
            .iter()
            .map(|entry| entry["family"].clone())
            .collect::<Vec<_>>(),
        serde_json::to_value(&planned.case.recommendation_families)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    );
    assert!(recommendation_families.iter().all(|entry| entry["use_when"]
        .as_str()
        .is_some_and(|text| !text.is_empty())));
    let mut recommendation_scope_ids = planned.case.subject_ids.clone();
    recommendation_scope_ids.extend(planned.case.finding_ids.iter().cloned());
    recommendation_scope_ids.sort();
    recommendation_scope_ids.dedup();
    assert_eq!(
        value["decision"]["recommendation_scope_ids"],
        json!(recommendation_scope_ids)
    );
    for host_only in [
        "schema_version",
        "packet_id",
        "packet_digest",
        "input_binding_id",
        "budget",
        "byte_count",
        "output_contract",
    ] {
        assert!(
            value["evidence"].get(host_only).is_none(),
            "{host_only} leaked into the model evidence projection"
        );
    }
    assert!(value["evidence"].get("allowed_neighborhood").is_none());
    assert!(
        value["evidence"].get("indexes").is_none(),
        "packet indexes duplicate the probe-local evidence contract"
    );
    assert_eq!(
        value["evidence"]["lookup_authority"]["symbol_neighborhood_target_ids"],
        json!([]),
        "symbols covered by the complete source must not be advertised as unread"
    );
    assert_eq!(
        value["evidence"]["lookup_authority"]["finding_source_target_ids"],
        json!([]),
        "the complete eagerly admitted finding source must not be advertised as unread"
    );
    assert!(value["evidence"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item.get("provenance").is_none() && item["evidence_classes"].is_array()));
    assert!(value.get("resolved_probes").is_none());
    assert!(value.get("unresolved_probes").is_none());
    assert!(value["model_probe_contracts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|probe| probe.get("target_ids").is_none()
            && probe.get("decision_target_ids").is_none()
            && probe["admissible_evidence_ids"].is_array()));
    for probe in value["model_probe_contracts"].as_array().unwrap() {
        for evidence_id in probe["admissible_evidence_ids"].as_array().unwrap() {
            assert!(value["evidence"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["id"] == *evidence_id));
        }
        assert!(probe.get("admissible_evidence_classes").is_none());
    }
    for slot in [
        "intervening_mutation",
        "invalidation_feasibility",
        "retention_risk",
    ] {
        let probe = value["model_probe_contracts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|probe| probe["slot"] == slot)
            .expect("source-decidable lifecycle guard probe");
        assert!(probe["admissible_evidence_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id.as_str().is_some_and(|id| id.starts_with("source-span:"))));
    }
    for result in value["host_resolved_probes"].as_array().unwrap() {
        assert!(result.get("result_id").is_none());
        assert!(result.get("case_id").is_none());
        assert!(result.get("resolver").is_none());
    }
    for definition in value["model_probe_contracts"].as_array().unwrap() {
        assert!(definition.get("case_id").is_none());
        assert!(definition.get("template_bundle_digest").is_none());
        assert!(definition.get("resolver_precedence").is_none());
        assert!(definition.get("positive_boundary").is_some());
        assert!(definition.get("negative_boundary").is_some());
    }
    let instruction = auditor_review::CaseProviderContext::reviewer_instruction(&prompt_bundle);
    assert!(instruction.contains("never answer a Host-resolved probe"));
    assert!(instruction.contains("answer yes only when evidence establishes positive_boundary"));
    assert!(instruction.contains("answer no only when evidence establishes negative_boundary"));
    assert!(instruction
        .contains("A function or type signature establishes capability, not runtime behavior"));
    assert!(instruction.contains("FnMut(&mut T)"));
    assert!(instruction.contains(
        "Without the callable body, an explicit behavioral contract, or runtime evidence"
    ));
    let conclusion_policy = value["decision"]["conclusion_derivation_policy"]
        .as_str()
        .expect("case-specific conclusion policy");
    assert!(conclusion_policy
        .contains("material_cost does not participate in this conclusion derivation"));
    assert!(conclusion_policy
        .contains("lifecycle_mismatch_substantiated regardless of the material_cost answer"));
    assert!(instruction.contains("case-probe IDs only"));
    assert!(instruction.contains("Claim status has deterministic precedence"));
    assert!(!instruction.contains("Challenger admission rules"));
    assert!(instruction.contains(
        "For structural_inference, qualitative_only, or unknown benefits, omit numeric_value"
    ));
    let consumer_probe = value["model_probe_contracts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|probe| probe["slot"] == "consumer_distinction")
        .expect("consumer distinction probe");
    assert_eq!(
        consumer_probe["question"],
        "Does at least one use of the reviewed result, including a loop iteration or downstream caller, require it to reflect different per-execution inputs or state?"
    );
    assert!(consumer_probe["positive_boundary"]
        .as_str()
        .unwrap()
        .contains("An opaque consumer or mutable argument alone is insufficient"));
    assert!(consumer_probe["positive_boundary"]
        .as_str()
        .unwrap()
        .contains("probes are not mutually exclusive"));
    let wider_owner_probe = value["model_probe_contracts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|probe| probe["slot"] == "wider_owner")
        .expect("wider-owner probe");
    assert!(wider_owner_probe["positive_boundary"]
        .as_str()
        .unwrap()
        .contains("A merely lexical enclosing scope"));
    let repetition_probe = value["model_probe_contracts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|probe| probe["slot"] == "repetition")
        .expect("production-lifecycle repetition requires semantic review");
    assert!(repetition_probe["admissible_evidence_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id.as_str().is_some_and(|id| id.starts_with("source-span:"))));
    assert!(value["host_resolved_probes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|probe| probe["slot"] != "repetition"));
    let material_cost_probe = value["model_probe_contracts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|probe| probe["slot"] == "material_cost")
        .expect("material cost probe");
    assert!(material_cost_probe["negative_boundary"]
        .as_str()
        .unwrap()
        .contains("Absence of a measurement is unknown, not no"));
    let challenger_instruction =
        auditor_review::CaseProviderContext::challenger_instruction(&prompt_bundle);
    assert!(challenger_instruction.contains("Challenger admission rules"));
    assert!(challenger_instruction.contains(
        "Objection evidence_ids accept only closed-packet evidence item IDs or case-probe IDs"
    ));
    assert!(!challenger_instruction.contains("Call answer_case_probe exactly once"));
    assert!(!challenger_instruction.contains("Claim status has deterministic precedence"));
    assert!(!challenger_instruction.contains("record_case_recommendation"));
    let legacy_projection = json!({
        "case_id": planned.case.case_id,
        "finding_ids": planned.case.finding_ids,
        "evidence_packet": planned.evidence_packet,
        "deterministic_probe_results": resolved,
        "unresolved_probes": unresolved,
        "operator_constraints": planned.case.operator_constraints,
        "output_contract": "reviewer-case-review-v1",
    });
    let compact_bytes = serde_json::to_vec(&value).unwrap().len();
    let legacy_bytes = serde_json::to_vec(&legacy_projection).unwrap().len();
    assert!(
        compact_bytes * 10 < legacy_bytes * 9,
        "compact projection did not remove at least ten percent: {compact_bytes}/{legacy_bytes}"
    );
}

#[test]
fn invalid_model_output_is_case_local_and_retains_call_audit_records() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_invalid_model_probe();
    let output_path = fixture
        .report
        .with_file_name("reviewed-v2-invalid-model.json");
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: Value =
        serde_json::from_slice(&std::fs::read(&output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(
        artifact["execution_truth"][0]["basis"],
        json!({"kind": "failure", "value": "model_output_invalid"})
    );
    assert_eq!(
        artifact["execution_truth"][0]["closure"]["detail_codes"],
        json!([
            "model_output_invalid",
            "provider_invalid_output",
            "provider_unavailable"
        ])
    );
    assert_eq!(artifact["provider_records"].as_array().unwrap().len(), 6);
    assert_eq!(artifact["egress_manifests"].as_array().unwrap().len(), 6);
    assert_eq!(
        artifact["provider_records"][0]["failure_class"],
        json!("unavailable")
    );
    assert_eq!(
        artifact["provider_records"][1]["failure_class"],
        json!("invalid_output")
    );
    assert_eq!(
        artifact["provider_records"][2]["failure_class"],
        json!("invalid_output")
    );
    assert_eq!(
        artifact["provider_records"][3]["failure_class"],
        json!("invalid_output")
    );
    assert_eq!(
        artifact["provider_records"][4]["failure_class"],
        json!("invalid_output")
    );
    assert_eq!(
        artifact["provider_records"][5]["failure_class"],
        json!("invalid_output")
    );
    assert!(artifact["provider_records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["failure_detail"]
            .as_str()
            .is_some_and(|detail| !detail.trim().is_empty())));
    assert_eq!(artifact["provider_records"][5]["repair_count"], json!(1));
    let correction_errors = transport.correction_errors.lock().unwrap();
    assert_eq!(correction_errors.len(), 4);
    assert!(correction_errors
        .iter()
        .all(|error| error.contains("source-span:outside-packet")));
    assert!(artifact["provider_records"][5]["call"]["request_id"]
        .as_str()
        .unwrap()
        .ends_with(":host-correction-4"));
    assert!(
        verify_case_engineering_review(&ReviewVerifyRequest {
            repo_root: fixture.repo.clone(),
            report: output_path,
        })
        .unwrap()
        .valid
    );
}

#[test]
fn invalid_case_tool_arguments_receive_one_audited_repair_turn() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_invalid_tool_schema_once();
    let output_path = fixture
        .report
        .with_file_name("reviewed-v2-repaired-tool.json");
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(&output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.provider_records.len(), 3);
    assert_eq!(
        artifact.provider_records[1].failure_class,
        Some(auditor_review::ProviderFailureClass::InvalidOutput)
    );
    assert_eq!(artifact.provider_records[1].repair_count, 0);
    assert_eq!(artifact.provider_records[2].failure_class, None);
    assert_eq!(artifact.provider_records[2].repair_count, 1);
    assert_eq!(
        artifact.provider_records[1].call.assessment_mode,
        auditor_review::AssessmentModeRecorded::NativeTools
    );
    assert_eq!(
        artifact.provider_records[2].call.assessment_mode,
        auditor_review::AssessmentModeRecorded::NativeTools
    );
    assert!(
        verify_case_engineering_review(&ReviewVerifyRequest {
            repo_root: fixture.repo.clone(),
            report: output_path,
        })
        .unwrap()
        .valid
    );
}

#[test]
fn valid_native_siblings_survive_partial_schema_rejection() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_partial_invalid_tool_schema_once();
    let output_path = fixture
        .report
        .with_file_name("reviewed-v2-partial-tool-repair.json");
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(&output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.provider_records.len(), 3);
    let partial = &artifact.provider_records[1];
    assert_eq!(
        partial.failure_class,
        Some(auditor_review::ProviderFailureClass::InvalidOutput)
    );
    assert!(
        !partial.call.discarded,
        "a partially admitted provider turn must retain its valid sibling calls"
    );
    assert_eq!(artifact.provider_records[2].failure_class, None);
    assert_eq!(artifact.provider_records[2].repair_count, 1);
    assert!(artifact.provider_records[2]
        .call
        .request_id
        .ends_with(":host-correction-1"));
    let correction_errors = transport.correction_errors.lock().unwrap();
    assert_eq!(correction_errors.len(), 1);
    assert!(correction_errors[0].contains("outside_contract"));
    assert!(
        verify_case_engineering_review(&ReviewVerifyRequest {
            repo_root: fixture.repo.clone(),
            report: output_path,
        })
        .unwrap()
        .valid
    );
}

#[test]
fn host_rejected_case_turn_receives_one_audited_correction() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan).with_invalid_host_claim_once();
    let output_path = fixture
        .report
        .with_file_name("reviewed-v2-corrected-host-turn.json");
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(&output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.provider_records.len(), 3);
    assert_eq!(
        artifact.provider_records[1].failure_class,
        Some(auditor_review::ProviderFailureClass::InvalidOutput)
    );
    assert!(artifact.provider_records[1].call.discarded);
    assert_eq!(artifact.provider_records[2].failure_class, None);
    assert_eq!(artifact.provider_records[2].repair_count, 1);
    assert_eq!(
        artifact.provider_records[2].call.assessment_mode,
        auditor_review::AssessmentModeRecorded::NativeTools
    );
    let correction_errors = transport.correction_errors.lock().unwrap();
    assert_eq!(correction_errors.len(), 1);
    assert!(
        correction_errors[0].contains("InvalidClaims"),
        "the correction turn must also report independently checkable claim errors"
    );
    assert!(
        verify_case_engineering_review(&ReviewVerifyRequest {
            repo_root: fixture.repo.clone(),
            report: output_path,
        })
        .unwrap()
        .valid
    );
}

#[test]
fn host_correction_schema_rejection_stays_in_native_task_protocol() {
    let fixture = Fixture::lifecycle_only();
    let request = request(&fixture);
    let plan = plan_engineering_review(&request).unwrap();
    let transport = ScriptedCaseTransport::new(&plan)
        .with_invalid_host_claim_once()
        .with_invalid_host_correction_schema_once();
    let output = run_case_engineering_review(
        &ReviewRunRequest {
            plan: request,
            output: fixture
                .report
                .with_file_name("reviewed-v2-host-and-schema-corrected.json"),
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &UncontrolledReview,
    )
    .unwrap();
    let artifact: auditor_review::EngineeringReviewV2 =
        serde_json::from_slice(&std::fs::read(output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.provider_records.len(), 4);
    assert_eq!(
        artifact.provider_records[2].failure_class,
        Some(auditor_review::ProviderFailureClass::InvalidOutput)
    );
    assert_eq!(artifact.provider_records[2].repair_count, 1);
    assert_eq!(artifact.provider_records[3].failure_class, None);
    assert_eq!(artifact.provider_records[3].repair_count, 1);
    assert!(artifact.provider_records[2]
        .call
        .request_id
        .ends_with(":host-correction-1"));
    assert!(artifact.provider_records[3]
        .call
        .request_id
        .ends_with(":host-correction-2"));
}

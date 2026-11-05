//! A scripted fake provider behind the review's transport, shared by the
//! driver and qualification tests: every request goes through the provider
//! adapters, and every reply comes back in the provider's own wire format.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use auditor_review::{
    run_engineering_review, EngineeringReview, HostedConsent, ProviderConfig, ProviderProtocol,
    Question, QuestionKind, QuestionStatus, QuestionTemplateBundle, RecommendationChecker,
    ReviewBrief, ReviewPromptBundleV2, ReviewRoutesV2, ReviewSetup, RouteBudget, RouteTarget,
    TranscriptRecorder, Transport, TransportRequest, TransportResponse, V2ProviderChain,
    DEFAULT_PATCH_CHECKS, REVIEW_ROUTES_V2_SCHEMA_VERSION,
};
use serde_json::{json, Value};

use crate::fixture::Fixture;

/// What the fake model sees of one request.
pub struct Step<'a> {
    pub question: &'a Question,
    /// Replies already given in this question's conversation, plus one.
    pub turn: usize,
    /// Requests already made in the whole review.
    pub request: usize,
    pub user: &'a str,
}

pub enum Reply {
    Calls(Vec<(&'static str, Value)>),
    Text(&'static str),
    Status(u16),
}

pub type Script = Box<dyn Fn(&Step<'_>) -> Reply + Send + Sync>;

pub struct FakeProvider {
    pub questions: Vec<Question>,
    pub script: Script,
    pub state: Mutex<FakeState>,
}

#[derive(Default)]
pub struct FakeState {
    pub turns: HashMap<String, usize>,
    pub bodies: Vec<(String, Value)>,
}

impl FakeProvider {
    pub fn new(fixture: &Fixture, script: Script) -> Self {
        Self {
            questions: fixture.view().questions(&templates()).unwrap(),
            script,
            state: Mutex::new(FakeState::default()),
        }
    }

    pub fn bodies(&self) -> Vec<(String, Value)> {
        self.state.lock().unwrap().bodies.clone()
    }
}

impl Transport for FakeProvider {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let protocol = if request.url.ends_with("/responses") {
            ProviderProtocol::Responses
        } else if request.url.ends_with("/messages") {
            ProviderProtocol::Messages
        } else {
            ProviderProtocol::ChatCompletions
        };
        let user = match protocol {
            ProviderProtocol::Messages => body["messages"][0]["content"].as_str(),
            ProviderProtocol::ChatCompletions => body["messages"][1]["content"].as_str(),
            ProviderProtocol::Responses => body["input"][1]["content"].as_str(),
        }
        .unwrap()
        .to_string();
        let question = self
            .questions
            .iter()
            .find(|question| {
                user.contains(&format!("question_id: {}", question.question_id.as_str()))
            })
            .expect("every request asks one known question");
        let mut state = self.state.lock().unwrap();
        state.bodies.push((request.url.clone(), body));
        let request_index = state.bodies.len() - 1;
        let turn = state
            .turns
            .get(question.question_id.as_str())
            .copied()
            .unwrap_or(0)
            + 1;
        let reply = (self.script)(&Step {
            question,
            turn,
            request: request_index,
            user: &user,
        });
        if !matches!(reply, Reply::Status(_)) {
            state
                .turns
                .insert(question.question_id.as_str().to_string(), turn);
        }
        Ok(wire(protocol, request_index, reply))
    }
}

/// The reply in the provider's wire format.
pub fn wire(protocol: ProviderProtocol, index: usize, reply: Reply) -> TransportResponse {
    let (calls, text) = match reply {
        Reply::Status(status) => {
            return TransportResponse {
                status,
                body: br#"{"error":{"message":"scripted failure"}}"#.to_vec(),
            }
        }
        Reply::Calls(calls) => (calls, None),
        Reply::Text(text) => (Vec::new(), Some(text)),
    };
    let id = |position: usize| format!("call_{index}_{position}");
    let body = match protocol {
        ProviderProtocol::Messages => {
            let mut content = text
                .map(|text| vec![json!({"type": "text", "text": text})])
                .unwrap_or_default();
            content.extend(calls.iter().enumerate().map(|(position, (name, input))| {
                json!({"type": "tool_use", "id": id(position), "name": name, "input": input})
            }));
            json!({
                "id": format!("msg_{index}"),
                "type": "message",
                "role": "assistant",
                "content": content,
                "stop_reason": if calls.is_empty() { "end_turn" } else { "tool_use" },
                "usage": {"input_tokens": 1000, "output_tokens": 100},
            })
        }
        ProviderProtocol::ChatCompletions => {
            let mut message = json!({"role": "assistant", "content": text});
            if !calls.is_empty() {
                message["tool_calls"] = calls
                    .iter()
                    .enumerate()
                    .map(|(position, (name, arguments))| {
                        json!({
                            "id": id(position),
                            "type": "function",
                            "function": {"name": name, "arguments": arguments.to_string()},
                        })
                    })
                    .collect();
            }
            json!({
                "id": format!("chatcmpl-{index}"),
                "choices": [{
                    "index": 0,
                    "message": message,
                    "finish_reason": if calls.is_empty() { "stop" } else { "tool_calls" },
                }],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 100},
            })
        }
        ProviderProtocol::Responses => {
            let mut output = text
                .map(|text| {
                    vec![json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    })]
                })
                .unwrap_or_default();
            output.extend(
                calls
                    .iter()
                    .enumerate()
                    .map(|(position, (name, arguments))| {
                        json!({
                            "type": "function_call",
                            "call_id": id(position),
                            "name": name,
                            "arguments": arguments.to_string(),
                        })
                    }),
            );
            json!({
                "id": format!("resp_{index}"),
                "status": "completed",
                "output": output,
                "usage": {"input_tokens": 1000, "output_tokens": 100},
            })
        }
    };
    TransportResponse {
        status: 200,
        body: serde_json::to_vec(&body).unwrap(),
    }
}

/// A reviewer that reads the store's findings, then answers well.
pub fn reviewer(step: &Step<'_>) -> Reply {
    match step.turn {
        1 => Reply::Calls(vec![("findings_in", json!({"scope": "app/store/db.py"}))]),
        _ => Reply::Calls(vec![("answer_question", good_answer(step))]),
    }
}

/// An answer that holds to the question's kind and cites only shown IDs.
pub fn good_answer(step: &Step<'_>) -> Value {
    let question = step.question;
    let subject = |index: usize| question.subjects[index].evidence_ids[0].clone();
    let claim = |reference: &str, slot: &str, kind: &str, cites: Vec<String>| {
        json!({
            "ref": reference,
            "slot": slot,
            "kind": kind,
            "proposition": format!("{slot} holds for the cited evidence."),
            "status": "substantiated",
            "supporting_evidence_ids": cites,
            "confidence": "medium",
        })
    };
    let recommendation =
        |family: &str, scope: Vec<String>, premises: Vec<String>, resolves: Vec<String>| {
            json!({
                "ref": "r1",
                "family": family,
                "summary": "Change the code the findings point at.",
                "scope_ids": scope,
                "premises": premises,
                "resolves": resolves,
                "detector_advice_relationship": "consolidates",
                "verification_steps": [{
                    "method": "Re-scan the repository.",
                    "expected_observation": "The resolved findings are gone.",
                    "evidence_ids": [],
                }],
                "materiality": "medium",
                "materiality_basis": "The findings sit on the request path.",
                "confidence": "medium",
            })
        };
    match question.kind {
        QuestionKind::HotspotAssessment => json!({
            "claims": (0..question.subjects.len())
                .map(|index| claim(&format!("c{index}"), "hotspot_risk", "actionability", vec![subject(index)]))
                .collect::<Vec<_>>(),
            "ranking": question.subjects.iter().map(|subject| subject.id.clone()).collect::<Vec<_>>(),
        }),
        QuestionKind::SharedRootCause => {
            let findings = question.finding_ids[..2].to_vec();
            json!({
                "claims": [claim("c1", "causal_overlap", "root_cause", findings.clone())],
                "recommendations": [recommendation(
                    "shared_root_remediation",
                    vec![question.subjects[0].id.clone()],
                    vec!["c1".to_string()],
                    findings,
                )],
            })
        }
        QuestionKind::ArchitectureReview => json!({
            "claims": std::iter::once(claim("c0", "intended_structure", "actionability", vec![subject(0)]))
                .chain((1..question.subjects.len()).map(|index| {
                    claim(&format!("c{index}"), "structural_violation", "actionability", vec![subject(index)])
                }))
                .collect::<Vec<_>>(),
        }),
        QuestionKind::RemediationPlan => {
            let finding = first_token(step.user, "finding:EA-");
            let mut premises = vec!["c1".to_string()];
            premises.extend(earlier_claim(step.user));
            json!({
                "claims": [claim("c1", "required_ordering", "recommendation_premise", vec![finding.clone()])],
                "recommendations": [recommendation(
                    "sequenced_remediation",
                    vec![finding.clone()],
                    premises,
                    vec![finding],
                )],
                "ranking": ["r1"],
            })
        }
    }
}

pub fn first_token(text: &str, prefix: &str) -> String {
    let start = text
        .find(prefix)
        .unwrap_or_else(|| panic!("no {prefix} in the request"));
    text[start..]
        .split(|character: char| character == ']' || character == ',' || character.is_whitespace())
        .next()
        .unwrap()
        .to_string()
}

/// A claim ID from the earlier answers the plan is shown.
pub fn earlier_claim(user: &str) -> Option<String> {
    user.find("claim engineering-claim:")
        .map(|start| first_token(&user[start + "claim ".len()..], "engineering-claim:"))
}

pub fn templates() -> QuestionTemplateBundle {
    QuestionTemplateBundle::checked_in().unwrap()
}

pub fn prompts() -> ReviewPromptBundleV2 {
    ReviewPromptBundleV2::parse(include_str!(
        "../../../../config/auditor/review-prompt-bundle-v2.json"
    ))
    .unwrap()
}

pub fn budget(max_requests: usize) -> RouteBudget {
    RouteBudget {
        max_requests,
        max_input_bytes: 8 * 1024 * 1024,
        max_output_tokens: 8192,
        timeout_ms: 60_000,
        max_cost_microusd: 1_000_000,
        allow_unknown_cost: true,
    }
}

/// Routes over one fake provider per protocol, in `targets` order.
pub fn routes(
    targets: &[(&str, ProviderProtocol)],
    review: usize,
    question: usize,
) -> ReviewRoutesV2 {
    let providers = targets
        .iter()
        .map(|(id, protocol)| {
            let mut provider = ProviderConfig::openai_compatible(
                *id,
                "https://fake.invalid/v1",
                None::<String>,
                vec!["fake-model".to_string()],
            );
            provider.model_protocols = BTreeMap::from([("fake-model".to_string(), *protocol)]);
            provider
        })
        .collect();
    let routes = ReviewRoutesV2 {
        schema_version: REVIEW_ROUTES_V2_SCHEMA_VERSION,
        providers,
        single_reviewer: V2ProviderChain {
            targets: targets
                .iter()
                .map(|(id, _)| RouteTarget {
                    provider_id: id.to_string(),
                    model: "fake-model".to_string(),
                })
                .collect(),
        },
        review_budget: budget(review),
        question_budget: budget(question),
    };
    routes.validate().unwrap();
    routes
}

pub struct Run {
    pub review: EngineeringReview,
    pub bodies: Vec<(String, Value)>,
    pub transcript: TranscriptRecorder,
}

pub fn run(
    fixture: &Fixture,
    routes: &ReviewRoutesV2,
    script: Script,
    checker: Option<&dyn RecommendationChecker>,
) -> Run {
    let provider = FakeProvider::new(fixture, script);
    let transcript = TranscriptRecorder::enabled();
    let templates = templates();
    let prompts = prompts();
    let brief = ReviewBrief::general();
    let view = fixture.view();
    let review = run_engineering_review(
        &view,
        ReviewSetup {
            repo_root: &fixture.repo,
            brief: Some(&brief),
            templates: &templates,
            prompts: &prompts,
            routes,
            consent: HostedConsent {
                allow_hosted: true,
                allow_anonymous: false,
            },
            transport: &provider,
            transcript: &transcript,
            checker,
            max_patch_checks: DEFAULT_PATCH_CHECKS,
        },
    )
    .unwrap();
    Run {
        review,
        bodies: provider.bodies(),
        transcript,
    }
}

pub fn status(review: &EngineeringReview) -> Vec<QuestionStatus> {
    review
        .questions
        .iter()
        .map(|question| question.status)
        .collect()
}

pub struct FakeChecker {
    pub checked: Mutex<Vec<(String, Vec<String>)>>,
}

impl RecommendationChecker for FakeChecker {
    fn check(&self, patch: &str, cited_findings: &[String]) -> Value {
        self.checked
            .lock()
            .unwrap()
            .push((patch.to_string(), cited_findings.to_vec()));
        json!({
            "verdict": "verified",
            "step": null,
            "reason": null,
            "resolved_findings": cited_findings,
            "remaining_findings": [],
            "added_findings": [],
        })
    }
}

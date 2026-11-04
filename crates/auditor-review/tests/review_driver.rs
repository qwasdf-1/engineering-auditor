//! The review driver end to end over a scan of a fixture repository, with a
//! scripted fake provider behind the transport: every request goes through
//! the provider adapters, and every reply comes back in the provider's own
//! wire format, so the request rendering and response parsing are the real
//! ones.

#[path = "reviewer/fixture.rs"]
mod fixture;

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use auditor_review::{
    run_engineering_review, CaseBudgetPolicy, CheckRequester, EngineeringReview, HostedConsent,
    ProviderConfig, ProviderFailureClass, ProviderProtocol, Question, QuestionKind, QuestionStatus,
    QuestionTemplateBundle, RecommendationChecker, ReviewBrief, ReviewPromptBundleV2,
    ReviewRoutesV2, ReviewSetup, RouteBudget, RouteTarget, TranscriptRecorder, Transport,
    TransportRequest, TransportResponse, V2ProviderChain, DEFAULT_PATCH_CHECKS,
    REVIEW_ROUTES_V2_SCHEMA_VERSION,
};
use fixture::{Fixture, PYTHON_APP};
use serde_json::{json, Value};

/// What the fake model sees of one request.
struct Step<'a> {
    question: &'a Question,
    /// Replies already given in this question's conversation, plus one.
    turn: usize,
    /// Requests already made in the whole review.
    request: usize,
    user: &'a str,
}

enum Reply {
    Calls(Vec<(&'static str, Value)>),
    Text(&'static str),
    Status(u16),
}

type Script = Box<dyn Fn(&Step<'_>) -> Reply + Send + Sync>;

struct FakeProvider {
    questions: Vec<Question>,
    script: Script,
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    turns: HashMap<String, usize>,
    bodies: Vec<(String, Value)>,
}

impl FakeProvider {
    fn new(fixture: &Fixture, script: Script) -> Self {
        Self {
            questions: fixture.view().questions(&templates()).unwrap(),
            script,
            state: Mutex::new(FakeState::default()),
        }
    }

    fn bodies(&self) -> Vec<(String, Value)> {
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
fn wire(protocol: ProviderProtocol, index: usize, reply: Reply) -> TransportResponse {
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
fn reviewer(step: &Step<'_>) -> Reply {
    match step.turn {
        1 => Reply::Calls(vec![("findings_in", json!({"scope": "app/store/db.py"}))]),
        _ => Reply::Calls(vec![("answer_question", good_answer(step))]),
    }
}

/// An answer that holds to the question's kind and cites only shown IDs.
fn good_answer(step: &Step<'_>) -> Value {
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

fn first_token(text: &str, prefix: &str) -> String {
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
fn earlier_claim(user: &str) -> Option<String> {
    user.find("claim engineering-claim:")
        .map(|start| first_token(&user[start + "claim ".len()..], "engineering-claim:"))
}

fn templates() -> QuestionTemplateBundle {
    QuestionTemplateBundle::checked_in().unwrap()
}

fn prompts() -> ReviewPromptBundleV2 {
    ReviewPromptBundleV2::parse(include_str!(
        "../../../config/auditor/review-prompt-bundle-v2.json"
    ))
    .unwrap()
}

fn budget(max_requests: usize) -> RouteBudget {
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
fn routes(targets: &[(&str, ProviderProtocol)], review: usize, question: usize) -> ReviewRoutesV2 {
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
        case_budgets: CaseBudgetPolicy {
            defaults: budget(question),
            per_template: BTreeMap::new(),
        },
    };
    routes.validate().unwrap();
    routes
}

struct Run {
    review: EngineeringReview,
    bodies: Vec<(String, Value)>,
    transcript: TranscriptRecorder,
}

fn run(
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

fn status(review: &EngineeringReview) -> Vec<QuestionStatus> {
    review
        .questions
        .iter()
        .map(|question| question.status)
        .collect()
}

#[test]
fn a_scripted_review_answers_every_question_on_each_protocol() {
    let fixture = Fixture::scan(PYTHON_APP);
    for protocol in [
        ProviderProtocol::Messages,
        ProviderProtocol::ChatCompletions,
        ProviderProtocol::Responses,
    ] {
        let routes = routes(&[("fake", protocol)], 32, 8);
        let Run {
            review,
            bodies,
            transcript,
        } = run(&fixture, &routes, Box::new(reviewer), None);
        assert_eq!(
            status(&review),
            [QuestionStatus::Answered; 4],
            "{protocol:?}: {:#?}",
            review.questions
        );
        assert_eq!(
            review
                .questions
                .iter()
                .map(|question| question.kind)
                .collect::<Vec<_>>(),
            QuestionKind::ALL
        );
        assert!(review
            .questions
            .iter()
            .all(|question| question.requests == 2
                && question.rejected_answers.is_empty()
                && question
                    .target
                    .as_ref()
                    .is_some_and(|target| target.provider_id == "fake")));
        assert_eq!((review.calls.len(), review.spend.requests), (8, 8));
        assert_eq!(review.spend.input_tokens, 8000);
        assert_eq!(transcript.events().len(), 8);
        assert_eq!(review.retrieval.len(), 4);
        assert!(review.stopped.is_none());

        // The plan rests on a claim from an earlier answer.
        let plan = review.questions[3].answer.as_ref().unwrap();
        let earlier = review
            .answered()
            .take(3)
            .flat_map(|answer| answer.claims.iter().map(|claim| claim.claim_id.clone()))
            .collect::<Vec<_>>();
        assert!(plan.recommendations[0]
            .premise_claim_ids
            .iter()
            .any(|premise| earlier.contains(premise)));
        assert_eq!(
            plan.ranking,
            [plan.recommendations[0].recommendation_id.as_str()]
        );

        // Each conversation opens with the pack, the brief, and the question,
        // and the second turn carries the first turn's tool result.
        let second = bodies[1].1.to_string();
        assert!(second.contains("Repository orientation") && second.contains("Operator brief"));
        assert!(second.contains("(evidence: "), "{protocol:?}");
        let plan_request = bodies[7].1.to_string();
        assert!(plan_request.contains("Earlier answers"));

        let text = serde_json::to_string(&review).unwrap();
        assert_eq!(
            serde_json::from_str::<EngineeringReview>(&text).unwrap(),
            review
        );
    }
}

#[test]
fn a_rejected_answer_comes_back_with_its_reasons_and_a_correction_is_accepted() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::HotspotAssessment, 2) => {
            let mut answer = good_answer(step);
            answer["claims"][0]["supporting_evidence_ids"] = json!(["finding:EA-made-up"]);
            Reply::Calls(vec![("answer_question", answer)])
        }
        (QuestionKind::HotspotAssessment, 3) => {
            Reply::Calls(vec![("answer_question", good_answer(step))])
        }
        _ => reviewer(step),
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    let hotspots = &review.questions[0];
    assert_eq!(hotspots.status, QuestionStatus::Answered);
    assert_eq!(hotspots.requests, 3);
    assert_eq!(hotspots.rejected_answers.len(), 1);
    assert!(hotspots.rejected_answers[0].contains("cites unknown evidence"));
    assert!(hotspots.rejected_answers[0].contains("`c0` is engineering-claim:"));
    assert!(bodies[2].1.to_string().contains("Answer rejected"));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
}

#[test]
fn a_reply_without_a_tool_call_is_corrected_and_the_question_goes_on() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    let script: Script = Box::new(|step| match step.turn {
        1 if step.question.kind == QuestionKind::SharedRootCause => {
            Reply::Text("The store and the audit log share a cause.")
        }
        turn if step.question.kind == QuestionKind::SharedRootCause => reviewer(&Step {
            turn: turn - 1,
            ..*step
        }),
        _ => reviewer(step),
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    let cluster = &review.questions[1];
    assert_eq!(cluster.status, QuestionStatus::Answered);
    assert_eq!(cluster.requests, 3);
    assert_eq!(
        review.calls[2].failure,
        Some(ProviderFailureClass::InvalidOutput)
    );
    assert!(bodies[3].1.to_string().contains("made no tool call"));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
}

#[test]
fn a_failed_request_is_retried_then_the_next_target_takes_over() {
    let fixture = Fixture::scan(PYTHON_APP);
    let retried = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let script: Script = Box::new(|step| match step.request {
        0 => Reply::Status(503),
        _ => reviewer(step),
    });
    let Run { review, .. } = run(&fixture, &retried, script, None);
    assert_eq!(
        review.calls[0].failure,
        Some(ProviderFailureClass::Unavailable)
    );
    assert_eq!((review.calls[1].retry, review.calls[1].failure), (1, None));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);

    let chain = routes(
        &[
            ("down", ProviderProtocol::Responses),
            ("up", ProviderProtocol::Messages),
        ],
        32,
        8,
    );
    let provider_down: Script = Box::new(|step| match step.request {
        0 => Reply::Status(401),
        _ => reviewer(step),
    });
    let Run { review, bodies, .. } = run(&fixture, &chain, provider_down, None);
    assert!(bodies[0].0.ends_with("/responses") && bodies[1].0.ends_with("/messages"));
    assert_eq!(
        review.calls[0].failure,
        Some(ProviderFailureClass::Authentication)
    );
    assert!(review
        .questions
        .iter()
        .all(|question| question.target.as_ref().unwrap().provider_id == "up"));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
}

#[test]
fn spent_budgets_and_failed_routes_leave_questions_unanswered_with_the_reason() {
    let fixture = Fixture::scan(PYTHON_APP);
    let one_request = routes(&[("fake", ProviderProtocol::Messages)], 32, 1);
    let Run { review, .. } = run(&fixture, &one_request, Box::new(reviewer), None);
    assert_eq!(status(&review), [QuestionStatus::Unanswered; 4]);
    assert!(review
        .questions
        .iter()
        .all(|question| question.reason.as_deref()
            == Some("the question's provider budget is spent")));
    assert!(review.stopped.is_none());

    let three_requests = routes(&[("fake", ProviderProtocol::Messages)], 3, 3);
    let Run { review, .. } = run(&fixture, &three_requests, Box::new(reviewer), None);
    assert_eq!(
        status(&review),
        [
            QuestionStatus::Answered,
            QuestionStatus::Unanswered,
            QuestionStatus::Unanswered,
            QuestionStatus::Unanswered
        ]
    );
    assert_eq!(review.spend.requests, 3);
    assert_eq!(
        review.stopped.as_deref(),
        Some("the review's provider budget is spent")
    );
    assert_eq!(review.questions[3].reason, review.stopped);

    let refused = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let Run { review, .. } = run(&fixture, &refused, Box::new(|_| Reply::Status(403)), None);
    assert_eq!(status(&review), [QuestionStatus::Unanswered; 4]);
    assert_eq!(review.spend.requests, 1);
    assert!(review
        .stopped
        .as_deref()
        .is_some_and(|reason| reason.starts_with("every route target failed")));
}

struct FakeChecker {
    checked: Mutex<Vec<(String, Vec<String>)>>,
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

#[test]
fn a_patch_is_checked_when_the_reviewer_asks_and_again_from_the_cache_when_accepted() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    const PATCH: &str = "--- a/app/core/audit.py\n+++ b/app/core/audit.py\n@@ -1,1 +1,1 @@\n-from app.store.db import Store\n+import app.store.db\n";
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::SharedRootCause, 1) => Reply::Calls(vec![
            (
                "check_patch",
                json!({"patch": PATCH, "resolves": step.question.finding_ids[..2]}),
            ),
            (
                "check_patch",
                json!({"patch": PATCH, "resolves": ["finding:EA-elsewhere"]}),
            ),
        ]),
        (QuestionKind::SharedRootCause, _) => {
            let mut answer = good_answer(step);
            answer["recommendations"][0]["patch"] = json!(PATCH);
            Reply::Calls(vec![("answer_question", answer)])
        }
        _ => reviewer(step),
    });
    let checker = FakeChecker {
        checked: Mutex::new(Vec::new()),
    };
    let Run { review, bodies, .. } = run(&fixture, &routes, script, Some(&checker));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let checked = checker.checked.lock().unwrap();
    assert_eq!(
        checked.len(),
        1,
        "the accepted patch reuses the reviewer's check"
    );
    assert!(checked[0].1.iter().all(|id| id.starts_with("EA-")));
    assert_eq!(review.spend.patch_checks, 1);
    let requesters = review
        .patch_checks
        .iter()
        .map(|check| check.requested_by)
        .collect::<Vec<_>>();
    assert_eq!(requesters, [CheckRequester::Reviewer, CheckRequester::Host]);
    let cluster = review.questions[1].answer.as_ref().unwrap();
    assert_eq!(
        review.patch_checks[1].recommendation_id.as_ref(),
        Some(&cluster.recommendations[0].recommendation_id)
    );
    let second = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("Patch check"))
        .unwrap();
    assert!(second.contains("**Verdict:** verified"));
    assert!(second.contains("must list finding IDs of this question"));
}

#[test]
fn no_request_leaves_without_hosted_consent() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let provider = FakeProvider::new(&fixture, Box::new(reviewer));
    let transcript = TranscriptRecorder::null();
    let (templates, prompts) = (templates(), prompts());
    let view = fixture.view();
    let error = run_engineering_review(
        &view,
        ReviewSetup {
            repo_root: &fixture.repo,
            brief: None,
            templates: &templates,
            prompts: &prompts,
            routes: &routes,
            consent: HostedConsent {
                allow_hosted: false,
                allow_anonymous: false,
            },
            transport: &provider,
            transcript: &transcript,
            checker: None,
            max_patch_checks: DEFAULT_PATCH_CHECKS,
        },
    )
    .unwrap_err();
    assert!(error.contains("consent"));
    assert!(provider.bodies().is_empty());
}

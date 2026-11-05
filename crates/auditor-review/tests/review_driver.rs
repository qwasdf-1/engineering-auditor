//! The review driver end to end over a scan of a fixture repository, with
//! the scripted fake provider behind the transport, so the request
//! rendering and response parsing are the real ones.

#[path = "reviewer/fixture.rs"]
mod fixture;
#[path = "reviewer/scripted.rs"]
mod scripted;

use std::sync::Mutex;

use auditor_review::{
    run_engineering_review, CheckRequester, EngineeringReview, HostedConsent, ProviderFailureClass,
    ProviderProtocol, QuestionKind, QuestionStatus, ReviewSetup, TranscriptRecorder,
    DEFAULT_PATCH_CHECKS,
};
use fixture::{Fixture, PYTHON_APP};
use scripted::{
    good_answer, prompts, reviewer, routes, run, status, templates, FakeChecker, FakeProvider,
    Reply, Run, Script, Step,
};
use serde_json::json;

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

    // A reader that never answers: its last two requests offer only the
    // answer tool, and the reads it asks for then are not run.
    let four_requests = routes(&[("fake", ProviderProtocol::Messages)], 64, 4);
    let reads = || Reply::Calls(vec![("findings_in", json!({"scope": "app/store/db.py"}))]);
    let Run { review, bodies, .. } =
        run(&fixture, &four_requests, Box::new(move |_| reads()), None);
    assert_eq!(status(&review), [QuestionStatus::Unanswered; 4]);
    assert_eq!(review.retrieval.len(), 8);
    let offered = bodies
        .iter()
        .map(|(_, body)| body["tools"].as_array().map_or(0, Vec::len))
        .collect::<Vec<_>>();
    assert_eq!(offered.len(), 16);
    for question in offered.chunks(4) {
        assert!(question[0] > 1 && question[1] > 1, "{offered:?}");
        assert_eq!(question[2..], [1, 1], "{offered:?}");
    }

    let refused = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let Run { review, .. } = run(&fixture, &refused, Box::new(|_| Reply::Status(403)), None);
    assert_eq!(status(&review), [QuestionStatus::Unanswered; 4]);
    assert_eq!(review.spend.requests, 1);
    assert!(review
        .stopped
        .as_deref()
        .is_some_and(|reason| reason.starts_with("every route target failed")));
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

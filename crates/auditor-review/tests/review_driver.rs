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
    good_answer, logged_run, prompts, resumable_run, reviewer, routes, run, status, templates,
    FakeChecker, FakeProvider, Reply, Run, Script, Step,
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
fn a_held_item_without_a_ref_is_dropped_and_a_broken_held_item_is_named() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::HotspotAssessment, 2) => {
            let mut answer = good_answer(step);
            answer["claims"][0].as_object_mut().unwrap().remove("slot");
            Reply::Calls(vec![("answer_question", answer)])
        }
        (QuestionKind::HotspotAssessment, 3) => {
            let mut claim = good_answer(step)["claims"][1].clone();
            claim.as_object_mut().unwrap().remove("ref");
            Reply::Calls(vec![(
                "answer_question",
                json!({"revise": true, "claims": [claim]}),
            )])
        }
        (QuestionKind::HotspotAssessment, 4) => Reply::Calls(vec![(
            "answer_question",
            json!({"revise": true, "ranking": good_answer(step)["ranking"]}),
        )]),
        (QuestionKind::HotspotAssessment, 5) => Reply::Calls(vec![(
            "answer_question",
            json!({"revise": true, "claims": [good_answer(step)["claims"][0]]}),
        )]),
        _ => reviewer(step),
    });
    let Run { review, .. } = run(&fixture, &routes, script, None);
    let hotspots = &review.questions[0];
    assert_eq!(hotspots.status, QuestionStatus::Answered);
    assert_eq!(hotspots.requests, 5);
    let reasons = &hotspots.rejected_answers;
    assert_eq!(reasons.len(), 3, "{reasons:?}");
    assert!(
        reasons[1].contains("omits required field `ref`"),
        "{reasons:?}"
    );
    // The claim with no ref was not held, so only the held claim that lost
    // its slot is left to fix, and the reason names it.
    assert!(
        reasons[2].contains("claim `c0`: missing field `slot`"),
        "{reasons:?}"
    );
}

#[test]
fn a_rejected_answer_is_held_and_a_revision_sends_only_what_changes() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::HotspotAssessment, 2) => {
            let mut answer = good_answer(step);
            answer["claims"][0]["supporting_evidence_ids"] = json!(["finding:EA-made-up"]);
            let mut extra = good_answer(step)["claims"][0].clone();
            extra["ref"] = json!("extra");
            answer["claims"].as_array_mut().unwrap().push(extra);
            Reply::Calls(vec![("answer_question", answer)])
        }
        (QuestionKind::HotspotAssessment, 3) => {
            let claim = good_answer(step)["claims"][0].clone();
            Reply::Calls(vec![(
                "answer_question",
                json!({"revise": true, "claims": [claim], "remove": ["extra"]}),
            )])
        }
        _ => reviewer(step),
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    let hotspots = &review.questions[0];
    assert_eq!(hotspots.status, QuestionStatus::Answered);
    assert_eq!(hotspots.requests, 3);
    assert_eq!(hotspots.rejected_answers.len(), 1);
    assert!(hotspots.rejected_answers[0].contains("claim `c0` (slot"));
    assert!(hotspots.rejected_answers[0].contains("cites unknown evidence"));
    let rejection = bodies[2].1.to_string();
    assert!(rejection.contains("Answer rejected") && rejection.contains("is held"));
    // The held answer kept every claim and the ranking the revision did
    // not touch, and dropped the claim it removed.
    let subjects = hotspots.subject_ids.len();
    let answer = hotspots.answer.as_ref().unwrap();
    assert_eq!(answer.claims.len(), subjects);
    assert_eq!(answer.ranking.len(), subjects);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
}

#[test]
fn the_reviewer_sees_hash_ids_by_alias_and_the_record_keeps_the_ids() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    let Run { review, bodies, .. } = run(&fixture, &routes, Box::new(reviewer), None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let shown = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .collect::<String>();
    assert!(!shown.contains("finding:EA-"), "finding IDs are aliased");
    for (alias, id) in &review.id_aliases {
        assert!(
            !shown.contains(id.as_str()),
            "{id} is shown only as {alias}"
        );
    }
    // The plan cited the aliases it was shown; its record holds the IDs.
    let plan = review.questions[3].answer.as_ref().unwrap();
    let step = &plan.recommendations[0];
    assert!(step.affected_finding_ids[0].starts_with("finding:EA-"));
    assert!(step.premise_claim_ids.len() >= 2);
    assert!(review
        .id_aliases
        .values()
        .any(|id| id == &step.affected_finding_ids[0]));
}

#[test]
fn a_read_repeated_in_a_question_points_at_its_result_instead_of_running_again() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::HotspotAssessment, 2) => reviewer(&Step { turn: 1, ..*step }),
        (QuestionKind::HotspotAssessment, turn) if turn > 2 => reviewer(&Step {
            turn: turn - 1,
            ..*step
        }),
        _ => reviewer(step),
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert!(bodies[2]
        .1
        .to_string()
        .contains("already answered in this conversation"));
    assert_eq!(
        review.retrieval.len(),
        4,
        "one read a question; the repeat is not run"
    );
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
    let (Run { review, .. }, lines) = logged_run(&fixture, &retried, script);
    assert_eq!(
        review.calls[0].failure,
        Some(ProviderFailureClass::Unavailable)
    );
    assert_eq!((review.calls[1].retry, review.calls[1].failure), (1, None));
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    // The log shows the retry and each question's start and end.
    assert_eq!(lines[0], "question 1 of 4 (HotspotAssessment) started");
    assert_eq!(
        lines[1],
        "question 1 of 4: request #1 failed (fake/fake-model: Unavailable: provider error: scripted failure); retry 1 of 10 in 15 s"
    );
    let first = &review.questions[0];
    assert_eq!(
        lines[2],
        format!("question 1 of 4 Answered after {} requests", first.requests)
    );
    assert_eq!(lines.len(), 9);

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
    let (Run { review, bodies, .. }, lines) = logged_run(&fixture, &chain, provider_down);
    assert!(bodies[0].0.ends_with("/responses") && bodies[1].0.ends_with("/messages"));
    let down = &review.calls[0].target;
    let up = &review.calls[1].target;
    assert_eq!(
        lines[1],
        format!(
            "question 1 of 4: down/{} failed for good; up/{} takes over",
            down.model, up.model
        )
    );
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
fn an_outage_on_the_last_target_is_waited_out_outside_the_request_budget() {
    let fixture = Fixture::scan(PYTHON_APP);
    // Three requests a question; the reviewer needs two once the outage
    // ends, after the four quick retries and four of the six resends.
    let tight = routes(&[("fake", ProviderProtocol::Messages)], 32, 3);
    let outage: Script = Box::new(|step| match step.request {
        0..=7 => Reply::Status(503),
        _ => reviewer(step),
    });
    let Run { review, .. } = run(&fixture, &tight, outage, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert_eq!(
        review.calls[..8]
            .iter()
            .map(|call| (call.retry, call.failure))
            .collect::<Vec<_>>(),
        (0..8)
            .map(|retry| (retry, Some(ProviderFailureClass::Unavailable)))
            .collect::<Vec<_>>()
    );
    assert!(review.calls[..8]
        .iter()
        .all(|call| call.produced_no_output()));
    assert_eq!(review.questions[0].requests, 10);
    assert_eq!(review.spend.requests, review.calls.len());

    // A gateway timeout is resent like an outage; past the resends it
    // stops the review after eleven attempts, and the questions after are
    // logged as not asked.
    let gateway: Script = Box::new(|_| Reply::Status(504));
    let (Run { review, .. }, lines) = logged_run(&fixture, &tight, gateway);
    assert_eq!(review.calls.len(), 11);
    assert!(review
        .calls
        .iter()
        .all(|call| call.failure == Some(ProviderFailureClass::Timeout)));
    assert!(review
        .stopped
        .as_deref()
        .is_some_and(|reason| reason.starts_with("every route target failed")));
    assert_eq!(lines.len(), 1 + 10 + 1 + 3);
    assert_eq!(lines[11], "question 1 of 4 Unanswered after 11 requests");
    assert!(lines[12..]
        .iter()
        .all(|line| line.ends_with(") not asked: the review stopped")));
}

/// A review that stops on the root-cause question: every request for it
/// fails, so the hotspot answer is the only one.
fn stopped_review(fixture: &Fixture, routes: &auditor_review::ReviewRoutesV2) -> EngineeringReview {
    let down: Script = Box::new(|step| match step.question.kind {
        QuestionKind::SharedRootCause => Reply::Status(503),
        _ => reviewer(step),
    });
    let review = run(fixture, routes, down, None).review;
    assert_eq!(
        status(&review),
        [
            QuestionStatus::Answered,
            QuestionStatus::Unanswered,
            QuestionStatus::Unanswered,
            QuestionStatus::Unanswered
        ]
    );
    review
}

#[test]
fn a_stopped_review_resumes_from_the_question_it_stopped_on() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let stopped = stopped_review(&fixture, &routes);
    assert_eq!(stopped.calls.len(), 2 + 11);

    let Run { review, bodies, .. } = resumable_run(
        &fixture,
        &routes,
        Box::new(reviewer),
        None,
        Some(&stopped),
        None,
    )
    .unwrap();
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert_eq!((review.resumed, review.stopped.as_ref()), (1, None));
    // The kept question is not asked again; the rest take two requests.
    assert_eq!(review.questions[0], stopped.questions[0]);
    assert_eq!(bodies.len(), 6);
    assert_eq!(review.calls[..13], stopped.calls[..]);
    assert_eq!(review.questions[1].requests, 11 + 2);
    assert_eq!(review.spend.requests, review.calls.len());
    assert_eq!(
        review.retrieval[..stopped.retrieval.len()],
        stopped.retrieval[..]
    );
    assert!(stopped
        .id_aliases
        .iter()
        .all(|(alias, id)| review.id_aliases.get(alias) == Some(id)));

    let refused = |prior: &EngineeringReview, routes: &auditor_review::ReviewRoutesV2| {
        resumable_run(
            &fixture,
            routes,
            Box::new(reviewer),
            None,
            Some(prior),
            None,
        )
        .err()
        .unwrap()
    };
    assert!(refused(&review, &routes).contains("only a stopped review resumes"));
    let other = scripted::routes(&[("fake", ProviderProtocol::Messages)], 32, 9);
    assert!(refused(&stopped, &other).contains("different routes"));
}

#[test]
fn a_checkpoint_after_each_question_resumes_an_interrupted_run() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let checkpoints = Mutex::new(Vec::new());
    let keep = |review: &EngineeringReview| checkpoints.lock().unwrap().push(review.clone());
    let Run { review, .. } = resumable_run(
        &fixture,
        &routes,
        Box::new(reviewer),
        None,
        None,
        Some(&keep),
    )
    .unwrap();
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let checkpoints = checkpoints.into_inner().unwrap();
    assert_eq!(checkpoints.len(), 3);
    let second = &checkpoints[1];
    assert_eq!(second.questions.len(), 2);
    assert_eq!(
        second.stopped.as_deref(),
        Some("the run was interrupted after question 2 of 4")
    );

    let Run {
        review: resumed,
        bodies,
        ..
    } = resumable_run(
        &fixture,
        &routes,
        Box::new(reviewer),
        None,
        Some(second),
        None,
    )
    .unwrap();
    assert_eq!(status(&resumed), [QuestionStatus::Answered; 4]);
    assert_eq!(bodies.len(), 4);
    assert_eq!(resumed.questions[..2], second.questions[..]);
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
    // answer tool, and the reads it asks for then are not run; it repeats
    // one read, which runs once a question.
    let four_requests = routes(&[("fake", ProviderProtocol::Messages)], 64, 4);
    let reads = || Reply::Calls(vec![("findings_in", json!({"scope": "app/store/db.py"}))]);
    let Run { review, bodies, .. } =
        run(&fixture, &four_requests, Box::new(move |_| reads()), None);
    assert_eq!(status(&review), [QuestionStatus::Unanswered; 4]);
    assert_eq!(review.retrieval.len(), 4);
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
            resume: None,
            checkpoint: None,
            progress: None,
        },
    )
    .unwrap_err();
    assert!(error.contains("consent"));
    assert!(provider.bodies().is_empty());
}

//! The review driver end to end over a scan of a fixture repository, with
//! the scripted fake provider behind the transport, so the request
//! rendering and response parsing are the real ones.

#[path = "reviewer/fixture.rs"]
mod fixture;
#[path = "reviewer/scripted.rs"]
mod scripted;

use std::sync::Mutex;
use std::time::Duration;

use auditor_review::{
    run_engineering_review, CheckRequester, CostClass, EngineeringReview, HostedConsent,
    ProviderFailureClass, ProviderProtocol, QuestionKind, QuestionStatus, ReviewSetup,
    TranscriptRecorder, DEFAULT_PATCH_CHECKS,
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
            ..
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
        assert_eq!(review.retrieval.len(), 8);
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
fn a_claim_s_wrong_import_fact_is_rejected_with_the_map_s_edges() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    let fact =
        |from: &str, to: &str, imports: bool| json!({"from": from, "to": to, "imports": imports});
    let script: Script = Box::new(move |step| match (step.question.kind, step.turn) {
        (QuestionKind::HotspotAssessment, 2) => {
            let mut answer = good_answer(step);
            answer["claims"][0]["imports"] = json!([
                fact("app/core/audit.py", "app/store/db.py", false),
                fact("app/store/base.py", "app/main.py", true),
                fact("app/nowhere.py", "app/main.py", true),
            ]);
            Reply::Calls(vec![("answer_question", answer)])
        }
        (QuestionKind::HotspotAssessment, 3) => {
            let mut answer = good_answer(step);
            answer["claims"][0]["imports"] = json!([
                fact("app/core/audit.py", "app/store/db.py", true),
                fact("app/store/base.py", "app/main.py", false),
            ]);
            Reply::Calls(vec![("answer_question", answer)])
        }
        _ => reviewer(step),
    });
    let Run { review, .. } = run(&fixture, &routes, script, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let hotspots = &review.questions[0];
    assert_eq!(hotspots.requests, 3);
    assert_eq!(hotspots.rejected_answers.len(), 1);
    let reason = &hotspots.rejected_answers[0];
    assert!(
        reason.contains("states that `app/core/audit.py` does not import `app/store/db.py`, but the map has one: app/core/audit.py:1 -> app/store/db.py"),
        "{reason}"
    );
    assert!(
        reason.contains(
            "states that `app/store/base.py` imports `app/main.py`, but the map has no import"
        ),
        "{reason}"
    );
    assert!(
        reason.contains("about `app/nowhere.py`, which names no mapped file or module"),
        "{reason}"
    );
}

#[test]
fn a_question_shows_the_source_its_findings_sit_in() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    // No reads at all: the attached source is enough to recommend on.
    let script: Script =
        Box::new(|step| Reply::Calls(vec![("answer_question", good_answer(step))]));
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert!(review
        .questions
        .iter()
        .all(|question| question.rejected_answers.is_empty()));
    let first = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("Source of the cited findings"))
        .unwrap();
    assert!(first.contains("source-span:app/core/audit.py#L"), "{first}");
    assert!(first.contains("def record(event, value):"), "{first}");
    // The plan builds on the other answers and attaches nothing.
    let plan = bodies.last().unwrap().1.to_string();
    assert!(!plan.contains("Source of the cited findings"));
}

/// [`PYTHON_APP`] with `audit.py`'s findings in spans too long to attach.
fn long_audit() -> Vec<(&'static str, String)> {
    let padding = "x".repeat(5000);
    PYTHON_APP
        .iter()
        .map(|(path, text)| match *path {
            "app/core/audit.py" => (
                *path,
                format!(
                    "from app.store.db import Store\n# {padding}\n\n\ndef record(event, value):\n    \"\"\"{padding}\"\"\"\n    try:\n        print(event, value)\n    except:\n        pass\n\n\ndef default_store():\n    return Store()\n"
                ),
            ),
            _ => (*path, (*text).to_string()),
        })
        .collect()
}

#[test]
fn a_finding_inside_a_long_function_brings_the_lines_around_it() {
    let padding = "x".repeat(5000);
    let filler = "    print(event)\n".repeat(15);
    // Far enough from the import that its lines leave the docstring out.
    let blank = "\n".repeat(12);
    let audit = format!(
        "from app.store.db import Store\n{blank}def record(event, value):\n    \"\"\"{padding}\"\"\"\n{filler}    try:\n        print(event, value)\n    except:\n        pass\n\n\ndef default_store():\n    return Store()\n"
    );
    let files = PYTHON_APP
        .iter()
        .map(|(path, text)| match *path {
            "app/core/audit.py" => (*path, audit.as_str()),
            _ => (*path, *text),
        })
        .collect::<Vec<_>>();
    let fixture = Fixture::scan(&files);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    let script: Script =
        Box::new(|step| Reply::Calls(vec![("answer_question", good_answer(step))]));
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let body = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("Part of record (lines 14-"))
        .unwrap();
    assert!(body.contains("except:"), "{body}");
    assert!(!body.contains(&padding), "{body}");
}

#[test]
fn a_rejected_change_to_unshown_lines_brings_their_source() {
    // Twenty functions of about 3 KiB, each with findings: more source
    // than the questions carry, so the plan resolves some none showed.
    let padding = "x".repeat(3000);
    let many = (0..20)
        .map(|index| {
            format!("def f{index}(event, value):\n    \"\"\"{padding}\"\"\"\n    try:\n        print(event, value)\n    except:\n        pass\n\n\n")
        })
        .collect::<String>();
    let mut files = PYTHON_APP.to_vec();
    files.push(("app/core/many.py", many.as_str()));
    let fixture = Fixture::scan(&files);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    // The plan resolves every finding, some on lines no question showed.
    let script: Script = Box::new(|step| {
        let mut answer = good_answer(step);
        if step.question.kind == QuestionKind::RemediationPlan {
            answer["recommendations"][0]["resolves"] = json!(step.question.finding_ids);
        }
        Reply::Calls(vec![("answer_question", answer)])
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    assert!(status(&review)
        .iter()
        .all(|status| *status == QuestionStatus::Answered));
    let plan = review.questions.last().unwrap();
    assert_eq!(plan.kind, QuestionKind::RemediationPlan);
    // Each rejection brings what fits of the source, so fewer lines stay
    // unshown each time, until the same answer is accepted.
    let unshown = plan
        .rejected_answers
        .iter()
        .map(|reason| reason.matches(" at app/").count())
        .collect::<Vec<_>>();
    assert!(
        unshown.len() >= 2 && unshown.windows(2).all(|pair| pair[1] < pair[0]),
        "{unshown:?}"
    );
    let rejected = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("Source of the lines to change"))
        .unwrap();
    assert!(rejected.contains("source-span:"), "{rejected}");
}

#[test]
fn a_recommendation_on_lines_no_read_has_shown_is_rejected_until_they_are_read() {
    let files = long_audit();
    let files = files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect::<Vec<_>>();
    let fixture = Fixture::scan(&files);
    let routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    let findings_only = || Reply::Calls(vec![("findings_in", json!({"scope": "app/store/db.py"}))]);
    let script: Script = Box::new(move |step| match (step.question.kind, step.turn) {
        (QuestionKind::SharedRootCause, 3) => {
            let read = |start: usize, end: usize| {
                let range =
                    json!({"path": "app/core/audit.py", "start_line": start, "end_line": end});
                ("read_source", range)
            };
            Reply::Calls(vec![read(1, 1), read(7, 14)])
        }
        (QuestionKind::SharedRootCause, 4) => {
            Reply::Calls(vec![("answer_question", good_answer(step))])
        }
        (_, 1) => findings_only(),
        _ => Reply::Calls(vec![("answer_question", good_answer(step))]),
    });
    let Run { review, bodies, .. } = run(&fixture, &routes, script, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    let shared = &review.questions[1];
    assert_eq!(shared.kind, QuestionKind::SharedRootCause);
    assert_eq!(shared.requests, 4);
    assert_eq!(shared.rejected_answers.len(), 1);
    let reason = &shared.rejected_answers[0];
    assert!(
        reason.contains("recommendation `r1` resolves findings whose lines no read has shown")
            && reason.contains("at app/core/audit.py:"),
        "{reason}"
    );
    // The reviewer is told what to read, by the aliases it sees.
    let body = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("no read has shown"))
        .unwrap();
    let told = body
        .split("# Answer rejected")
        .nth(1)
        .and_then(|rest| rest.split("The rejected answer is held").next())
        .unwrap();
    assert!(
        told.contains("finding:") && told.contains("read any lines named instead with read_source"),
        "{told}"
    );
    assert!(!told.contains("EA-"), "{told}");
    // The question named the spans it could not attach.
    assert!(body.contains("too long to attach"), "{body}");
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
        8,
        "two reads a question; the repeats are not run"
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
fn a_rate_limit_s_retry_after_and_the_provider_s_interval_space_out_requests() {
    let fixture = Fixture::scan(PYTHON_APP);
    let mut paced = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    paced.providers[0].min_request_interval_ms = Some(2_000);
    let script: Script = Box::new(|step| match step.request {
        0 => Reply::RateLimited(90),
        _ => reviewer(step),
    });
    let (Run { review, waits, .. }, lines) = logged_run(&fixture, &paced, script);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    // The retry waits the 90 s the provider asked for, not the 15 s
    // backoff; every request after the first then waits out what is left
    // of the interval since the one before.
    assert!(lines[1].ends_with("retry 1 of 10 in 90 s"), "{}", lines[1]);
    assert_eq!(waits[0], Duration::from_secs(90));
    assert_eq!(waits.len(), review.calls.len());
    assert!(waits[1..]
        .iter()
        .all(|wait| (Duration::from_millis(1_500)..=Duration::from_secs(2)).contains(wait)));
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
fn a_streamed_chat_review_answers_every_question() {
    let fixture = Fixture::scan(PYTHON_APP);
    let mut routes = routes(&[("fake", ProviderProtocol::ChatCompletions)], 32, 8);
    routes.providers[0].stream = true;
    routes.validate().unwrap();
    let Run { review, bodies, .. } = run(&fixture, &routes, Box::new(reviewer), None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert_eq!(review.spend.input_tokens, 8000);
    assert!(
        bodies
            .iter()
            .all(|(_, body)| body["stream"] == true
                && body["stream_options"]["include_usage"] == true)
    );

    let mut messages = auditor_review::ProviderConfig::openai_compatible(
        "m",
        "https://fake.invalid/v1",
        None::<String>,
        vec!["fake-model".to_string()],
    );
    messages.model_protocols = [("fake-model".to_string(), ProviderProtocol::Messages)].into();
    messages.stream = true;
    assert!(messages
        .validate()
        .unwrap_err()
        .contains("chat-completions"));
}

#[test]
fn a_provider_out_of_capacity_costs_nothing_against_the_cost_budget() {
    let fixture = Fixture::scan(PYTHON_APP);
    // Each billed request is estimated at 10,000 µUSD and a question may
    // spend 30,000: the four rate limits must not count, or the question's
    // budget is spent before the reviewer's two requests.
    let mut priced = routes(&[("fake", ProviderProtocol::Messages)], 32, 8);
    priced.providers[0].cost_class = CostClass::Known;
    priced.providers[0].estimated_cost_microusd_per_request = Some(10_000);
    priced.question_budget.max_cost_microusd = 30_000;
    priced.validate().unwrap();
    let busy: Script = Box::new(|step| match step.request {
        0..=3 => Reply::Status(429),
        _ => reviewer(step),
    });
    let Run { review, .. } = run(&fixture, &priced, busy, None);
    assert_eq!(status(&review), [QuestionStatus::Answered; 4]);
    assert!(review.calls[..4]
        .iter()
        .all(|call| call.turned_away() && call.failure == Some(ProviderFailureClass::RateLimit)));
    let billed = review
        .calls
        .iter()
        .filter(|call| !call.turned_away())
        .count();
    assert_eq!(review.spend.cost_microusd, 10_000 * billed as u64);
    assert_eq!(review.spend.requests, review.calls.len());
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

    // A gateway timeout is retried twice, not waited out like an outage:
    // the review stops after three attempts, and the questions after are
    // logged as not asked.
    let gateway: Script = Box::new(|_| Reply::Status(504));
    let (Run { review, .. }, lines) = logged_run(&fixture, &tight, gateway);
    assert_eq!(review.calls.len(), 3);
    assert!(review
        .calls
        .iter()
        .all(|call| call.failure == Some(ProviderFailureClass::Timeout)));
    assert!(review
        .stopped
        .as_deref()
        .is_some_and(|reason| reason.starts_with("every route target failed")));
    assert_eq!(lines.len(), 1 + 2 + 1 + 3);
    assert!(lines[1].ends_with("retry 1 of 2 in 15 s"), "{}", lines[1]);
    assert_eq!(lines[3], "question 1 of 4 Unanswered after 3 requests");
    assert!(lines[4..]
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
            (
                "check_patch",
                json!({"patch": PATCH, "resolves": step.question.finding_ids[..2]}),
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
        "the repeated and the accepted patch reuse the reviewer's check"
    );
    assert!(checked[0].1.iter().all(|id| id.starts_with("EA-")));
    assert_eq!(review.spend.patch_checks, 1);
    let requesters = review
        .patch_checks
        .iter()
        .map(|check| check.requested_by)
        .collect::<Vec<_>>();
    assert_eq!(
        requesters,
        [
            CheckRequester::Reviewer,
            CheckRequester::Reviewer,
            CheckRequester::Host
        ]
    );
    let cluster = review.questions[1].answer.as_ref().unwrap();
    assert_eq!(
        review.patch_checks[2].recommendation_id.as_ref(),
        Some(&cluster.recommendations[0].recommendation_id)
    );
    let second = bodies
        .iter()
        .map(|(_, body)| body.to_string())
        .find(|body| body.contains("Patch check"))
        .unwrap();
    assert!(second.contains("**Verdict:** verified"));
    assert!(second.contains("must list finding IDs of this question"));
    assert_eq!(
        second.matches("cost no check").count(),
        1,
        "only the repeat says it reused a check"
    );
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

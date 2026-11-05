//! The qualification harness over recorded reviews of a fixture scan: the
//! scripted reviewer's runs pass every gate and agree, and each kind of
//! defect, written into a recorded run, fails the gate that owns it.

#[path = "reviewer/fixture.rs"]
mod fixture;
#[path = "reviewer/scripted.rs"]
mod scripted;

use auditor_review::{
    qualify_reviews, usefulness_sheet, EngineeringReview, Gate, ProviderProtocol,
    QualificationInputs, QuestionKind, RecordedReview, ReviewBrief, ReviewQualification,
    ReviewRoutesV2, ScoreCheck, StabilityPolicy, UsefulnessScores, UsefulnessSheet,
    UsefulnessStatus,
};
use fixture::{Fixture, PYTHON_APP};
use scripted::{
    good_answer, prompts, reviewer, routes, run, templates, FakeChecker, Reply, Script,
};
use serde_json::{json, Value};

fn fake_routes() -> ReviewRoutesV2 {
    routes(&[("fake", ProviderProtocol::Messages)], 32, 8)
}

fn qualify(
    fixture: &Fixture,
    routes: &ReviewRoutesV2,
    reviews: &[EngineeringReview],
    sheets: &[UsefulnessSheet],
) -> Result<ReviewQualification, String> {
    let view = fixture.view();
    let brief = ReviewBrief::general();
    let (templates, prompts) = (templates(), prompts());
    let runs = reviews
        .iter()
        .enumerate()
        .map(|(index, review)| RecordedReview {
            label: format!("run-{index}"),
            review: review.clone(),
        })
        .collect::<Vec<_>>();
    qualify_reviews(
        &QualificationInputs {
            view: &view,
            brief: Some(&brief),
            templates: &templates,
            prompts: &prompts,
            routes,
        },
        &runs,
        StabilityPolicy::default(),
        sheets,
    )
}

/// The gates the first run failed.
fn failed(qualification: &ReviewQualification) -> Vec<Gate> {
    qualification.runs[0]
        .gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.gate)
        .collect()
}

fn good_review(fixture: &Fixture) -> EngineeringReview {
    run(fixture, &fake_routes(), Box::new(reviewer), None).review
}

/// The review with `change` applied to its JSON form.
fn tampered(review: &EngineeringReview, change: impl FnOnce(&mut Value)) -> EngineeringReview {
    let mut value = serde_json::to_value(review).unwrap();
    change(&mut value);
    serde_json::from_value(value).unwrap()
}

#[test]
fn two_scripted_runs_pass_every_gate_and_agree() {
    let fixture = Fixture::scan(PYTHON_APP);
    let reviews = [good_review(&fixture), good_review(&fixture)];
    let qualification = qualify(&fixture, &fake_routes(), &reviews, &[]).unwrap();
    assert!(qualification.passed, "{qualification:#?}");
    for run in &qualification.runs {
        assert_eq!((run.questions, run.answered), (4, 4));
        assert_eq!(
            run.gates.iter().map(|gate| gate.gate).collect::<Vec<_>>(),
            [
                Gate::Binding,
                Gate::Coverage,
                Gate::Citations,
                Gate::Contradictions,
                Gate::PatchChecks,
                Gate::Budget
            ]
        );
    }
    let pair = &qualification.stability.pairs[0];
    assert_eq!(pair.runs, [0, 1]);
    assert!(pair
        .hotspots
        .iter()
        .all(|overlap| overlap.shared == overlap.compared && overlap.compared > 0));
    assert!(pair.plan.as_ref().is_some_and(|plan| plan.shared == 1));
    assert!(qualification.usefulness.is_none());

    let text = serde_json::to_string(&qualification).unwrap();
    assert_eq!(
        serde_json::from_str::<ReviewQualification>(&text).unwrap(),
        qualification
    );
}

#[test]
fn each_recorded_defect_fails_the_gate_that_owns_it() {
    let fixture = Fixture::scan(PYTHON_APP);
    let good = good_review(&fixture);
    let routes = fake_routes();
    let gates = |review: EngineeringReview| {
        failed(&qualify(&fixture, &routes, &[review, good.clone()], &[]).unwrap())
    };

    let made_up = tampered(&good, |value| {
        value["questions"][0]["answer"]["claims"][0]["supporting_evidence_ids"] =
            json!(["finding:EA-made-up"]);
    });
    assert_eq!(gates(made_up), [Gate::Citations]);

    let unanswered = tampered(&good, |value| {
        value["questions"][2]["answer"] = Value::Null;
        value["questions"][2]["status"] = json!("unanswered");
    });
    assert_eq!(gates(unanswered), [Gate::Coverage]);

    let both_ways = tampered(&good, |value| {
        let claim = &mut value["questions"][0]["answer"]["claims"][0];
        claim["counter_evidence_ids"] = claim["supporting_evidence_ids"].clone();
    });
    assert!(gates(both_ways).contains(&Gate::Contradictions));

    let (finding, key, measured) = measured_value(&fixture, &good);
    let misquote = tampered(&good, |value| {
        let claim = &mut value["questions"][3]["answer"]["claims"][0];
        claim["supporting_evidence_ids"] = json!([finding]);
        claim["proposition"] = json!(format!("Its {key} is {}.", measured + 7.0));
    });
    assert!(gates(misquote).contains(&Gate::Contradictions));

    let overspent = tampered(&good, |value| {
        value["spend"]["requests"] = json!(good.spend.requests + 1);
    });
    assert_eq!(gates(overspent), [Gate::Budget]);

    let other_routes = scripted::routes(&[("elsewhere", ProviderProtocol::Messages)], 32, 8);
    let qualification =
        qualify(&fixture, &other_routes, &[good.clone(), good.clone()], &[]).unwrap();
    assert_eq!(failed(&qualification), [Gate::Binding, Gate::Budget]);
    assert!(!qualification.passed);
}

/// A plan finding the report measured, one of its specific numeric
/// evidence keys, and the value.
fn measured_value(fixture: &Fixture, review: &EngineeringReview) -> (String, String, f64) {
    let plan = review.questions[3].answer.as_ref().unwrap();
    let cited = &plan.claims[0].supporting_evidence_ids[0];
    fixture.report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| format!("finding:{}", finding["id"].as_str().unwrap()) == *cited)
        .chain(fixture.report["findings"].as_array().unwrap())
        .find_map(|finding| {
            let (key, value) = finding["evidence"]
                .as_object()?
                .iter()
                .find(|(key, value)| key.contains('_') && value.is_number())?;
            Some((
                format!("finding:{}", finding["id"].as_str()?),
                key.replace('_', " "),
                value.as_f64()?,
            ))
        })
        .expect("a finding with a measured value")
}

const PATCH: &str = "--- a/app/core/audit.py\n+++ b/app/core/audit.py\n@@ -1,1 +1,1 @@\n-from app.store.db import Store\n+import app.store.db\n";

#[test]
fn an_accepted_patch_must_carry_the_hosts_verdict() {
    let fixture = Fixture::scan(PYTHON_APP);
    let routes = fake_routes();
    let script: Script = Box::new(|step| match (step.question.kind, step.turn) {
        (QuestionKind::SharedRootCause, 2) => {
            let mut answer = good_answer(step);
            answer["recommendations"][0]["patch"] = json!(PATCH);
            Reply::Calls(vec![("answer_question", answer)])
        }
        _ => reviewer(step),
    });
    let checker = FakeChecker {
        checked: Default::default(),
    };
    let checked = run(&fixture, &routes, script, Some(&checker)).review;
    let reviews = [checked.clone(), good_review(&fixture)];
    assert_eq!(
        failed(&qualify(&fixture, &routes, &reviews, &[]).unwrap()),
        []
    );

    let unchecked = tampered(&checked, |value| value["patch_checks"] = json!([]));
    let qualification =
        qualify(&fixture, &routes, &[unchecked, good_review(&fixture)], &[]).unwrap();
    assert_eq!(failed(&qualification), [Gate::PatchChecks]);
    let gate = &qualification.runs[0].gates[4];
    assert!(
        gate.failures[0].ends_with("patch was never checked"),
        "{gate:?}"
    );
}

#[test]
fn runs_must_agree_and_one_run_is_not_enough() {
    let fixture = Fixture::scan(PYTHON_APP);
    let good = good_review(&fixture);
    let routes = fake_routes();
    let alone = qualify(&fixture, &routes, std::slice::from_ref(&good), &[]).unwrap();
    assert!(failed(&alone).is_empty());
    assert!(!alone.stability.passed && !alone.passed);

    let silent = tampered(&good, |value| {
        value["questions"][3]["answer"] = Value::Null;
        value["questions"][3]["status"] = json!("unanswered");
    });
    let disagreeing = qualify(&fixture, &routes, &[good.clone(), silent], &[]).unwrap();
    assert!(disagreeing
        .stability
        .failures
        .iter()
        .any(|failure| failure.contains("only one answered")));

    let elsewhere = tampered(&good, |value| {
        let plan = &mut value["questions"][3]["answer"]["recommendations"][0];
        plan["affected_finding_ids"] = json!(["EA-elsewhere"]);
    });
    let plans = qualify(&fixture, &routes, &[good, elsewhere], &[]).unwrap();
    assert_eq!(plans.stability.pairs[0].plan.as_ref().unwrap().shared, 0);
    assert!(!plans.stability.passed);
}

fn scores(useful: bool) -> UsefulnessScores {
    UsefulnessScores {
        correct: true,
        specific: true,
        actionable: true,
        novel: useful,
    }
}

#[test]
fn usefulness_counts_only_corroborated_and_final_scores() {
    let fixture = Fixture::scan(PYTHON_APP);
    let good = good_review(&fixture);
    let routes = fake_routes();
    let blank = usefulness_sheet(&good);
    assert_eq!(blank.review_digest, good.digest());
    assert!(blank.items.len() >= 6);
    assert!(blank
        .items
        .iter()
        .all(|item| item.status == UsefulnessStatus::Pending && item.scores.is_none()));
    assert!(blank.items[0]
        .text
        .starts_with("hotspot_risk (substantiated): "));
    let recommendation = blank
        .items
        .iter()
        .find(|item| item.text.starts_with("shared_root_remediation: "))
        .unwrap();
    assert_eq!(recommendation.patch_verdict, None);

    let check = |scorer: &str, useful| ScoreCheck {
        scorer: scorer.to_string(),
        scores: scores(useful),
        reason: "read the cited code".to_string(),
    };
    let mut sheet = blank.clone();
    let items = &mut sheet.items;
    items[0].status = UsefulnessStatus::PreLabel;
    items[0].scores = Some(scores(true));
    items[0].scorer = Some("model:judge-a".to_string());
    items[0].checks = vec![check("model:judge-b", false)];
    items[1].status = UsefulnessStatus::Corroborated;
    items[1].scores = Some(scores(true));
    items[1].scorer = Some("model:judge-a".to_string());
    items[1].checks = vec![check("model:judge-b", true)];
    items[2].status = UsefulnessStatus::Final;
    items[2].scores = Some(scores(false));
    items[2].scorer = Some("human:operator".to_string());

    let reviews = [good.clone(), good_review(&fixture)];
    let summary = qualify(&fixture, &routes, &reviews, &[sheet.clone()])
        .unwrap()
        .usefulness
        .unwrap();
    assert_eq!(
        (
            summary.sheets,
            summary.items,
            summary.pending,
            summary.pre_label,
            summary.disputed
        ),
        (1, blank.items.len(), blank.items.len() - 3, 1, 1)
    );
    assert_eq!(
        (summary.corroborated, summary.final_scores, summary.counted),
        (1, 1, 2)
    );
    assert_eq!((summary.useful, summary.correct, summary.novel), (1, 2, 1));

    let mut by_reviewer = sheet.clone();
    by_reviewer.items[1].checks = vec![check("model:fake-model", true)];
    let error = qualify(&fixture, &routes, &reviews, &[by_reviewer]).unwrap_err();
    assert!(error.contains("ran the review"), "{error}");

    let mut self_corroborated = sheet.clone();
    self_corroborated.items[1].checks = vec![check("model:judge-a", true)];
    assert!(qualify(&fixture, &routes, &reviews, &[self_corroborated]).is_err());

    let mut restated = sheet.clone();
    restated.items[0].text.push_str(" (and more)");
    let error = qualify(&fixture, &routes, &reviews, &[restated]).unwrap_err();
    assert!(
        error.contains("does not list that review's items"),
        "{error}"
    );

    let other = tampered(&good, |value| value["stopped"] = json!("elsewhere"));
    let error = qualify(&fixture, &routes, &[other.clone(), other], &[sheet]).unwrap_err();
    assert!(error.contains("not given"), "{error}");
}

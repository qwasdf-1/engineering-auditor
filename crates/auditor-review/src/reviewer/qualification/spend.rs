//! The patch-check and budget gates. Every accepted recommendation that
//! carries a patch must have the host's check with a verdict of verified
//! or refuted. The recorded spend must add up from the calls, with costs
//! from the routes, and stay within the review's and each question's
//! provider budget, the retrieval budget (the brief tightens it), and the
//! review's patch-check limit; every call must go to a route target. A
//! request that produced no model output counts against the cost budget
//! only, as the driver counts it.

use std::collections::BTreeMap;

use super::super::driver::{CheckRequester, EngineeringReview, Spend};
use super::super::retrieval::RetrievalBudget;
use crate::{RetrievalOutcome, ReviewBrief, ReviewRoutesV2, RouteBudget};

pub(super) fn patch_check_failures(review: &EngineeringReview) -> Vec<String> {
    let mut failures = Vec::new();
    let mut patched = Vec::new();
    for step in review.answered().flat_map(|answer| &answer.recommendations) {
        patched.push(&step.recommendation_id);
        if step.patch.is_none() {
            continue;
        }
        let verdict = review
            .patch_checks
            .iter()
            .rev()
            .find(|check| {
                check.requested_by == CheckRequester::Host
                    && check.recommendation_id.as_ref() == Some(&step.recommendation_id)
            })
            .map(|check| {
                (
                    check.check["verdict"].as_str(),
                    check.check["reason"].as_str(),
                )
            });
        let id = step.recommendation_id.as_str();
        match verdict {
            Some((Some("verified" | "refuted"), _)) => {}
            Some((verdict, reason)) => failures.push(format!(
                "recommendation {id}'s patch was left {}: {}",
                verdict.unwrap_or("without a verdict"),
                reason.unwrap_or("no reason recorded")
            )),
            None => failures.push(format!("recommendation {id}'s patch was never checked")),
        }
    }
    failures.extend(
        review
            .patch_checks
            .iter()
            .filter_map(|check| check.recommendation_id.as_ref())
            .filter(|id| !patched.contains(id))
            .map(|id| {
                format!(
                    "a patch check names {}, which no answer recommends",
                    id.as_str()
                )
            }),
    );
    failures
}

/// What a set of calls spent, recomputed: every request sent, and the
/// requests and input bytes the budgets count.
#[derive(Default)]
struct Tally {
    sent: usize,
    sent_bytes: usize,
    requests: usize,
    input_bytes: usize,
    cost_microusd: u64,
}

impl Tally {
    fn over(&self, budget: RouteBudget) -> Option<&'static str> {
        if self.requests > budget.max_requests {
            Some("requests")
        } else if self.input_bytes > budget.max_input_bytes {
            Some("input bytes")
        } else if self.cost_microusd > budget.max_cost_microusd {
            Some("estimated cost")
        } else {
            None
        }
    }
}

pub(super) fn budget_failures(
    routes: &ReviewRoutesV2,
    brief: Option<&ReviewBrief>,
    review: &EngineeringReview,
) -> Vec<String> {
    let Recount {
        mut failures,
        total,
        spend,
        per_question,
    } = recount(routes, review);
    if spend != review.spend {
        failures.push("the recorded spend does not add up from the calls".to_string());
    }
    if let Some(over) = total.over(routes.review_budget) {
        failures.push(format!("the review spent more {over} than its budget"));
    }
    failures.extend(question_failures(routes, review, per_question));
    failures.extend(retrieval_failures(brief, review));
    if review.spend.patch_checks > review.max_patch_checks {
        failures.push(format!(
            "the review ran {} patch checks, over its limit of {}",
            review.spend.patch_checks, review.max_patch_checks
        ));
    }
    failures
}

/// The spend the calls add up to, overall and per question, and the calls
/// that broke a per-call rule.
struct Recount<'r> {
    failures: Vec<String>,
    total: Tally,
    spend: Spend,
    per_question: BTreeMap<&'r str, Tally>,
}

fn recount<'r>(routes: &ReviewRoutesV2, review: &'r EngineeringReview) -> Recount<'r> {
    let mut failures = Vec::new();
    let mut total = Tally::default();
    let mut spend = Spend::default();
    let mut per_question = BTreeMap::<&str, Tally>::new();
    let output_limit = routes
        .question_budget
        .max_output_tokens
        .min(routes.review_budget.max_output_tokens);
    for call in &review.calls {
        if !routes.single_reviewer.targets.contains(&call.target) {
            failures.push(format!(
                "request {} went to {}/{}, which is not a route target",
                call.request_id, call.target.provider_id, call.target.model
            ));
        }
        if call.usage.output_tokens > output_limit {
            failures.push(format!(
                "request {} produced {} output tokens, over the limit of {output_limit}",
                call.request_id, call.usage.output_tokens
            ));
        }
        let cost = routes
            .provider(&call.target.provider_id)
            .and_then(|provider| provider.estimated_cost_microusd_per_request)
            .unwrap_or(0);
        for tally in [
            &mut total,
            per_question.entry(call.question_id.as_str()).or_default(),
        ] {
            tally.sent += 1;
            tally.sent_bytes += call.input_bytes;
            if !call.produced_no_output() {
                tally.requests += 1;
                tally.input_bytes += call.input_bytes;
            }
            tally.cost_microusd += cost;
        }
        spend.input_tokens += call.usage.input_tokens;
        spend.cached_input_tokens += call.usage.cached_input_tokens;
        spend.output_tokens += call.usage.output_tokens;
    }
    spend.requests = total.sent;
    spend.input_bytes = total.sent_bytes;
    spend.cost_microusd = total.cost_microusd;
    spend.patch_checks = review.spend.patch_checks;
    Recount {
        failures,
        total,
        spend,
        per_question,
    }
}

/// Each question's calls against its record and the question budget, and
/// calls naming a question the review did not ask.
fn question_failures(
    routes: &ReviewRoutesV2,
    review: &EngineeringReview,
    mut per_question: BTreeMap<&str, Tally>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for record in &review.questions {
        let id = record.question_id.as_str();
        let tally = per_question.remove(id).unwrap_or_default();
        if tally.sent != record.requests {
            failures.push(format!(
                "{id} records {} requests, but {} calls name it",
                record.requests, tally.sent
            ));
        }
        if let Some(over) = tally.over(routes.question_budget) {
            failures.push(format!("{id} spent more {over} than the question budget"));
        }
    }
    failures.extend(
        per_question
            .keys()
            .map(|question| format!("calls name {question}, which the review did not ask")),
    );
    failures
}

fn retrieval_failures(brief: Option<&ReviewBrief>, review: &EngineeringReview) -> Vec<String> {
    let budget = RetrievalBudget::default().tightened_by(brief);
    let answered = review
        .retrieval
        .iter()
        .filter(|record| record.outcome == RetrievalOutcome::Answered)
        .collect::<Vec<_>>();
    let mut failures = answered
        .iter()
        .filter(|record| record.result_bytes > budget.max_result_bytes)
        .map(|record| {
            format!(
                "retrieval call {} returned {} bytes, over the {}-byte result limit",
                record.call_id, record.result_bytes, budget.max_result_bytes
            )
        })
        .collect::<Vec<_>>();
    if answered.len() > budget.max_calls {
        failures.push(format!(
            "{} retrieval calls were answered, over the limit of {}",
            answered.len(),
            budget.max_calls
        ));
    }
    let bytes = answered
        .iter()
        .map(|record| record.result_bytes)
        .sum::<usize>();
    if bytes > budget.max_total_bytes {
        failures.push(format!(
            "retrieval returned {bytes} bytes, over the review's {}",
            budget.max_total_bytes
        ));
    }
    failures
}

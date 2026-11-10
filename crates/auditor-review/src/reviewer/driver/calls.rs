//! Provider calls for one review: the route's targets in order, a request
//! retried while the provider returned no model output, the next target
//! when one fails for good, an outage on the last target waited out (a
//! gateway timeout only retried twice), and the review and question
//! budgets.

use std::time::Instant;

use crate::provider::retry::{no_model_output_retry_delay, retries_allowed, target_retries};
use crate::{
    BuiltinAdapter, CallDispatch, HostedConsent, NativeToolTurn, ProviderConfig, ProviderFailure,
    ProviderFailureClass, ProviderRequest, ProviderResponse, ReviewRoutesV2, RouteBudget,
    RouteTarget, StableId, TranscriptRecorder, Transport,
};

use super::progress::{Progress, SLOW_REQUEST_SECONDS};
use super::record::{EngineeringReview, ReviewCall, Spend};
use super::ReviewSetup;

/// The output-schema name the adapters answer with the turn's tool calls.
pub(crate) const REVIEWER_TURN: &str = "reviewer_turn";
pub(super) const REVIEW_STAGE: &str = "engineering_review";

pub(super) struct Caller<'a> {
    pub(super) routes: &'a ReviewRoutesV2,
    pub(super) consent: HostedConsent,
    pub(super) transport: &'a dyn Transport,
    pub(super) transcript: &'a TranscriptRecorder,
    /// The target in use; later ones are tried when it fails for good.
    target: usize,
    pub(super) spend: Spend,
    pub(super) calls: Vec<ReviewCall>,
    /// What the review's request and byte budgets count.
    counted: Counted,
    pub(super) progress: Progress<'a>,
}

/// Requests and input bytes the budgets count: every request that got
/// model output ([`ReviewCall::produced_no_output`]).
#[derive(Clone, Copy, Default)]
struct Counted {
    requests: usize,
    input_bytes: usize,
}

impl Counted {
    fn add(&mut self, call: &ReviewCall) {
        if !call.produced_no_output() {
            self.requests += 1;
            self.input_bytes += call.input_bytes;
        }
    }
}

/// What one question has spent.
#[derive(Default)]
pub(super) struct QuestionSpend {
    /// Every request sent for the question, as its record counts them.
    pub(super) requests: usize,
    counted: Counted,
    cost_microusd: u64,
}

pub(super) enum TurnOutcome {
    Turn(NativeToolTurn, RouteTarget),
    /// The model answered without a tool call, or ran out of tokens.
    NoCalls,
    /// No budget is left for the request.
    Spent(String),
    /// Every target failed.
    Failed(String),
}

impl<'a> Caller<'a> {
    pub(super) fn new(setup: &ReviewSetup<'a>) -> Result<Self, String> {
        let (routes, consent) = (setup.routes, setup.consent);
        routes.validate()?;
        if !consent.allow_hosted {
            return Err("a review sends repository evidence to the route's providers; it needs explicit hosted-review consent".to_string());
        }
        for target in &routes.single_reviewer.targets {
            let provider = routes
                .provider(&target.provider_id)
                .ok_or("the route names an unknown provider")?;
            if provider.kind.is_anonymous() && !consent.allow_anonymous {
                return Err(format!(
                    "provider `{}` is anonymous and needs separate consent",
                    provider.id
                ));
            }
        }
        Ok(Self {
            routes,
            consent,
            transport: setup.transport,
            transcript: setup.transcript,
            target: 0,
            spend: Spend::default(),
            calls: Vec::new(),
            counted: Counted::default(),
            progress: Progress::new(setup.progress),
        })
    }

    /// Continues a stopped review: its calls and spend carry over, and the
    /// budgets count them.
    pub(super) fn resume(&mut self, prior: &EngineeringReview) {
        self.calls = prior.calls.clone();
        self.spend = prior.spend;
        self.counted = Counted::default();
        for call in &prior.calls {
            self.counted.add(call);
        }
    }

    /// What earlier attempts at a question spent, which its budget keeps
    /// counting when a resumed review asks it again.
    pub(super) fn question_spend(&self, question_id: &StableId) -> QuestionSpend {
        let mut spent = QuestionSpend::default();
        for call in self
            .calls
            .iter()
            .filter(|call| &call.question_id == question_id)
        {
            spent.requests += 1;
            spent.counted.add(call);
            spent.cost_microusd += self.cost(&call.target);
        }
        spent
    }

    fn cost(&self, target: &RouteTarget) -> u64 {
        self.routes
            .provider(&target.provider_id)
            .and_then(|provider| provider.estimated_cost_microusd_per_request)
            .unwrap_or(0)
    }

    /// The budget one question may spend.
    pub(super) fn question_budget(&self) -> RouteBudget {
        self.routes.question_budget
    }

    /// Requests left before the question's or the review's budget is spent.
    pub(super) fn requests_left(&self, question: &QuestionSpend) -> usize {
        let review = self.routes.review_budget.max_requests;
        let question_left = self
            .question_budget()
            .max_requests
            .saturating_sub(question.counted.requests);
        question_left.min(review.saturating_sub(self.counted.requests))
    }

    /// Sends one request: retried on the same target while the provider
    /// gives no model output, then on the next target.
    pub(super) fn turn(
        &mut self,
        question_id: &StableId,
        request: &ProviderRequest,
        dispatch: &CallDispatch<'_>,
        input_bytes: usize,
        question: &mut QuestionSpend,
    ) -> TurnOutcome {
        let mut last_failure = String::new();
        while let Some(target) = self
            .routes
            .single_reviewer
            .targets
            .get(self.target)
            .cloned()
        {
            let attempt = Attempt {
                question_id,
                request,
                dispatch,
                input_bytes,
                target: &target,
            };
            match self.on_target(&attempt, question) {
                Ok(outcome) => return outcome,
                Err(failure) => last_failure = failure,
            }
            self.target += 1;
            if let Some(next) = self.routes.single_reviewer.targets.get(self.target) {
                self.progress.target_changed(&target, next);
            }
        }
        TurnOutcome::Failed(format!("every route target failed; last: {last_failure}"))
    }

    /// Sends the request to one target, retried while the provider gives
    /// no model output (and, on the last target, resent through an
    /// outage); `Err` with the last failure when the target failed for
    /// good.
    fn on_target(
        &mut self,
        attempt: &Attempt<'_>,
        question: &mut QuestionSpend,
    ) -> Result<TurnOutcome, String> {
        let target = attempt.target;
        let provider = self
            .routes
            .provider(&target.provider_id)
            .expect("validated routes name known providers");
        let cost = self.cost(target);
        let last_target = self.target + 1 == self.routes.single_reviewer.targets.len();
        let retries = target_retries(last_target);
        // The retries the last failure allows.
        let mut allowed = retries;
        let mut last_failure = String::new();
        for retry in 0..=retries {
            if retry > 0 {
                let delay = no_model_output_retry_delay(retry - 1);
                let request = &attempt.request.request_id;
                self.progress
                    .retry(request, &last_failure, (retry, allowed), delay);
                self.transport.wait(delay);
            }
            if let Some(reason) = self.exhausted(question, attempt.input_bytes, cost) {
                return Ok(TurnOutcome::Spent(reason));
            }
            let result = self.send(attempt, provider);
            let call = attempt.record(retry, &result);
            question.charge(&call, cost);
            self.counted.add(&call);
            self.spend.add_request(attempt.input_bytes, cost);
            self.calls.push(call);
            let failure = match result {
                Ok(response) => {
                    self.spend.add_usage(response.usage);
                    return Ok(parsed_turn(response, target));
                }
                Err(failure) => failure,
            };
            self.spend.add_usage(failure.usage);
            if no_calls(&failure) {
                return Ok(TurnOutcome::NoCalls);
            }
            last_failure = format!(
                "{}/{}: {:?}: {}",
                target.provider_id, target.model, failure.class, failure.message
            );
            allowed = retries_allowed(&failure, retries);
            if retry >= allowed {
                break;
            }
        }
        Err(last_failure)
    }

    /// Sends the request once, reporting it when it was slow.
    fn send(
        &self,
        attempt: &Attempt<'_>,
        provider: &ProviderConfig,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let target = attempt.target;
        let adapter = BuiltinAdapter::new(provider, self.transport, self.consent);
        let started = Instant::now();
        let result = adapter.execute_dispatched(
            &target.model,
            attempt.request,
            attempt.dispatch,
            self.transcript,
            self.transcript.is_enabled(),
        );
        let seconds = started.elapsed().as_secs();
        if seconds >= SLOW_REQUEST_SECONDS {
            self.progress
                .slow(&attempt.request.request_id, target, seconds);
        }
        result
    }

    /// Why the next request would overrun the question's or the review's
    /// budget.
    fn exhausted(&self, question: &QuestionSpend, input_bytes: usize, cost: u64) -> Option<String> {
        let over = |budget: RouteBudget, counted: Counted, spent: u64| {
            counted.requests >= budget.max_requests
                || counted.input_bytes + input_bytes > budget.max_input_bytes
                || spent + cost > budget.max_cost_microusd
        };
        if over(
            self.routes.review_budget,
            self.counted,
            self.spend.cost_microusd,
        ) {
            return Some("the review's provider budget is spent".to_string());
        }
        over(
            self.question_budget(),
            question.counted,
            question.cost_microusd,
        )
        .then(|| "the question's provider budget is spent".to_string())
    }
}

/// One request on one target.
struct Attempt<'r> {
    question_id: &'r StableId,
    request: &'r ProviderRequest,
    dispatch: &'r CallDispatch<'r>,
    input_bytes: usize,
    target: &'r RouteTarget,
}

impl Attempt<'_> {
    fn record(&self, retry: u8, result: &Result<ProviderResponse, ProviderFailure>) -> ReviewCall {
        let (usage, finish_reason, failure, failure_detail) = match result {
            Ok(response) => (response.usage, response.finish_reason.clone(), None, None),
            Err(failure) => (
                failure.usage,
                None,
                Some(failure.class),
                Some(failure.message.clone()),
            ),
        };
        ReviewCall {
            question_id: self.question_id.clone(),
            request_id: self.request.request_id.clone(),
            target: self.target.clone(),
            retry,
            input_bytes: self.input_bytes,
            usage,
            finish_reason,
            failure,
            failure_detail,
        }
    }
}

impl QuestionSpend {
    fn charge(&mut self, call: &ReviewCall, cost_microusd: u64) {
        self.requests += 1;
        self.counted.add(call);
        self.cost_microusd += cost_microusd;
    }
}

fn parsed_turn(response: ProviderResponse, target: &RouteTarget) -> TurnOutcome {
    match serde_json::from_value(response.structured_output) {
        Ok(turn) => TurnOutcome::Turn(turn, target.clone()),
        Err(error) => TurnOutcome::Failed(format!("the adapter's turn did not parse: {error}")),
    }
}

/// A turn with no tool call, or cut off at the token limit: the model
/// answered, so the request is not retried; the review tells it what to do.
fn no_calls(failure: &ProviderFailure) -> bool {
    matches!(
        failure.class,
        ProviderFailureClass::InvalidOutput | ProviderFailureClass::Truncated
    ) && !failure.retryable_without_model_output
}

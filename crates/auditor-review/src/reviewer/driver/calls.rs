//! Provider calls for one review: the route's targets in order, a request
//! retried while the provider returned no model output, the next target
//! when one fails for good, and the review and question budgets.

use crate::provider::retry::{wait_before_no_model_output_retry, MAX_NO_MODEL_OUTPUT_RETRIES};
use crate::{
    BuiltinAdapter, CallDispatch, HostedConsent, NativeToolTurn, ProviderFailure,
    ProviderFailureClass, ProviderRequest, ProviderResponse, ReviewRoutesV2, RouteBudget,
    RouteTarget, StableId, TranscriptRecorder, Transport,
};

use super::record::{ReviewCall, Spend};

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
}

/// What one question has spent.
#[derive(Default)]
pub(super) struct QuestionSpend {
    pub(super) requests: usize,
    input_bytes: usize,
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
    pub(super) fn new(
        routes: &'a ReviewRoutesV2,
        consent: HostedConsent,
        transport: &'a dyn Transport,
        transcript: &'a TranscriptRecorder,
    ) -> Result<Self, String> {
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
            transport,
            transcript,
            target: 0,
            spend: Spend::default(),
            calls: Vec::new(),
        })
    }

    /// The budget one question may spend.
    pub(super) fn question_budget(&self) -> RouteBudget {
        self.routes.question_budget
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
        }
        TurnOutcome::Failed(format!("every route target failed; last: {last_failure}"))
    }

    /// Sends the request to one target, retried while the provider gives
    /// no model output; `Err` with the last failure when the target failed
    /// for good.
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
        let cost = provider.estimated_cost_microusd_per_request.unwrap_or(0);
        let mut last_failure = String::new();
        for retry in 0..=MAX_NO_MODEL_OUTPUT_RETRIES {
            if retry > 0 {
                wait_before_no_model_output_retry(retry - 1);
            }
            if let Some(reason) = self.exhausted(question, attempt.input_bytes, cost) {
                return Ok(TurnOutcome::Spent(reason));
            }
            question.charge(attempt.input_bytes, cost);
            self.spend.add_request(attempt.input_bytes, cost);
            let adapter = BuiltinAdapter::new(provider, self.transport, self.consent);
            let result = adapter.execute_dispatched(
                &target.model,
                attempt.request,
                attempt.dispatch,
                self.transcript,
                self.transcript.is_enabled(),
            );
            self.calls.push(attempt.record(retry, &result));
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
            if !failure.retryable_without_model_output {
                break;
            }
        }
        Err(last_failure)
    }

    /// Why the next request would overrun the question's or the review's
    /// budget.
    fn exhausted(&self, question: &QuestionSpend, input_bytes: usize, cost: u64) -> Option<String> {
        let over = |budget: RouteBudget, requests: usize, bytes: usize, spent: u64| {
            requests >= budget.max_requests
                || bytes + input_bytes > budget.max_input_bytes
                || spent + cost > budget.max_cost_microusd
        };
        if over(
            self.routes.review_budget,
            self.spend.requests,
            self.spend.input_bytes,
            self.spend.cost_microusd,
        ) {
            return Some("the review's provider budget is spent".to_string());
        }
        over(
            self.question_budget(),
            question.requests,
            question.input_bytes,
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
    fn charge(&mut self, input_bytes: usize, cost_microusd: u64) {
        self.requests += 1;
        self.input_bytes += input_bytes;
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

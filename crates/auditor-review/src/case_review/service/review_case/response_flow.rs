use super::{response_tasks::ResponseTaskApplication, *};

pub(super) fn respond_to_challenge(
    request: ResponseRequest<'_>,
    state: &CaseEvidenceState,
    log: &mut CallLog,
) -> Result<Option<ChallengeResponse>, ReviewServiceError> {
    if request.challenge.objections.is_empty() {
        return Ok(None);
    }
    checkpoint(request.runtime.control, ReviewStage::Responding)?;
    ResponseFlow::new(request, state, log)?.run(log).map(Some)
}

struct ResponseFlow<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    review: &'a ReviewerCaseReview,
    reviewer_route: &'a ReviewerRoute,
    challenge: &'a crate::ChallengeRecord,
    packet: &'a crate::EvidencePacket,
    allowed_probe_ids: BTreeSet<StableId>,
    ledger: ResponseTaskLedger,
    provider_context: Value,
    context_markdown: String,
    instruction: String,
    conversation: Vec<NativeConversationMessage>,
    budget: TaskTurnBudget,
    followup: CaseCallFollowup,
}

struct AppliedTurn {
    ledger_changed: bool,
    application: ResponseTaskApplication,
}

enum TurnProgress {
    Retry,
    Complete(ChallengeResponse),
}

impl<'a> ResponseFlow<'a> {
    fn new(
        request: ResponseRequest<'a>,
        state: &'a CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<Self, ReviewServiceError> {
        let ResponseRequest {
            planned,
            runtime,
            review,
            reviewer_route,
            challenge,
        } = request;
        let allowed_probe_ids = probe_ids(&state.definitions);
        let ledger = ResponseTaskLedger::new(ResponseTaskRequirements {
            objection_ids: challenge
                .objections
                .iter()
                .map(|objection| objection.objection_id.to_string())
                .collect(),
        });
        let provider_context = json!({
            "challenge": challenge_context(challenge),
            "reviewer_review": reviewer_review_context(review),
        });
        let context_markdown = render_response_context(&provider_context, log)?;
        let instruction = format!(
            "{}\n\nAddress every objection exactly once with record_objection_response. Record a complete replacement recommendation first when narrowing, then reference it. Finish with complete_challenge_response.",
            runtime.prompts.bounded_response_instruction
        );
        Ok(Self {
            planned,
            runtime,
            review,
            reviewer_route,
            challenge,
            packet: &state.packet,
            allowed_probe_ids,
            ledger,
            provider_context,
            context_markdown,
            instruction,
            conversation: Vec::new(),
            budget: TaskTurnBudget::default(),
            followup: CaseCallFollowup::None,
        })
    }

    fn run(mut self, log: &mut CallLog) -> Result<ChallengeResponse, ReviewServiceError> {
        loop {
            let turn = self.call_and_apply(log)?;
            if let TurnProgress::Complete(response) = self.handle(turn, log)? {
                return Ok(response);
            }
        }
    }

    fn call_and_apply(&mut self, log: &mut CallLog) -> Result<AppliedTurn, ReviewServiceError> {
        let tool_parameter_overrides =
            response_task_parameter_overrides(self.challenge, self.review, &self.allowed_probe_ids);
        let output = execute_logged_case_call(
            CaseCallRequest {
                case: &self.planned.case,
                packet: self.packet,
                role: ReviewRole::SingleReviewer,
                pass: self.pass(),
                stage: ReviewStage::Responding,
                stage_name: self.stage_name(),
                instruction: self.instruction.clone(),
                context: self.provider_context.clone(),
                context_markdown: self.context_markdown.clone(),
                conversation: with_followup_note(self.conversation.clone(), self.followup),
                output_schema_name: "challenge_response_tasks",
                output_schema: json!({"items": {"type": "object"}, "type": "array"}),
                tool_parameter_overrides,
                evidence_requests_allowed: false,
                preferred_target: Some(self.reviewer_route.target.clone()),
                followup_attempt: self.budget.followup_turns,
                followup: self.followup,
            },
            call_runtime(self.runtime),
            log,
        )?;
        let native_turn =
            record_native_assistant_turn(output.value, &mut self.conversation, "response-task")?;
        let ledger_before = self.ledger.clone();
        let application = apply_response_task_calls(
            &mut self.ledger,
            &native_turn.tool_calls,
            last_request_id(log),
        );
        Ok(AppliedTurn {
            ledger_changed: self.ledger != ledger_before,
            application,
        })
    }

    fn handle(
        &mut self,
        turn: AppliedTurn,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        self.conversation.extend(turn.application.tool_results);
        if !turn.application.errors.is_empty() {
            return self.correct_task_calls(turn.application.errors, turn.ledger_changed, log);
        }
        if !self.ledger.is_complete() {
            return self.request_completion(turn.ledger_changed);
        }
        self.admit_completed_response(log)
    }

    fn correct_task_calls(
        &mut self,
        errors: Vec<String>,
        made_progress: bool,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let validation_error = errors.join("; ");
        mark_last_call_host_rejected(
            log,
            self.runtime.transcript,
            &validation_error,
            !made_progress,
        );
        if self.budget.record_invalid(made_progress) {
            return Err(failed(validation_error));
        }
        self.conversation
            .push(NativeConversationMessage::UserCorrection {
                content_markdown: render_task_correction(
                    "Challenge response calls need correction",
                    &errors,
                ),
            });
        self.followup = CaseCallFollowup::HostCorrection;
        Ok(TurnProgress::Retry)
    }

    fn request_completion(
        &mut self,
        made_progress: bool,
    ) -> Result<TurnProgress, ReviewServiceError> {
        if self.budget.record_incomplete(made_progress) {
            return Err(failed(
                "challenge response ledger made no progress for four consecutive turns",
            ));
        }
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: render_task_correction(
                    "Challenge response completion is still required",
                    &[
                        "complete_challenge_response must follow one response for every objection"
                            .to_string(),
                    ],
                ),
            });
        self.followup = CaseCallFollowup::AcceptedProgress;
        Ok(TurnProgress::Retry)
    }

    fn admit_completed_response(
        &mut self,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let response_value = response_value_from_ledger(&self.ledger)?;
        let admission = ChallengeResponseAdmission::for_review(
            self.reviewer_route.run_id.clone(),
            &self.planned.case,
            self.challenge,
            self.review,
            &self.allowed_probe_ids,
        );
        match ChallengeResponse::admit_wire(response_value, admission) {
            Ok(response) => {
                record_response_task_audit(log, &self.ledger);
                Ok(TurnProgress::Complete(response))
            }
            Err(error) => self.correct_semantic_admission(error.to_string(), log),
        }
    }

    fn correct_semantic_admission(
        &mut self,
        validation_error: String,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        mark_last_call_host_rejected(log, self.runtime.transcript, &validation_error, true);
        if self.budget.record_invalid(false) {
            return Err(failed(validation_error));
        }
        self.conversation
            .push(NativeConversationMessage::UserCorrection {
                content_markdown: render_task_correction(
                    "The completed response ledger failed semantic admission",
                    &[validation_error],
                ),
            });
        self.followup = CaseCallFollowup::HostCorrection;
        Ok(TurnProgress::Retry)
    }

    fn pass(&self) -> u8 {
        if self.followup == CaseCallFollowup::None {
            3
        } else {
            4
        }
    }

    fn stage_name(&self) -> &'static str {
        match self.followup {
            CaseCallFollowup::None => "case_responding",
            CaseCallFollowup::AcceptedProgress => "case_response_continuation",
            CaseCallFollowup::HostCorrection => "case_response_repair",
        }
    }
}

fn render_response_context(
    context: &Value,
    log: &mut CallLog,
) -> Result<String, ReviewServiceError> {
    let digest = sha256_digest_bytes(
        &serde_json::to_vec(context).map_err(|error| failed(error.to_string()))?,
    );
    let projection = render_structured_model_document(
        "Challenge response tasks",
        context,
        digest,
        &[
            "challenge.challenge_id",
            "challenge.status",
            "reviewer_review.review_id",
            "reviewer_review.route_target",
            "reviewer_review.reviewer_run_id",
        ],
    )
    .map_err(|error| failed(error.to_string()))?;
    log.projection_audits.push(projection.audit);
    Ok(projection.markdown)
}

fn last_request_id(log: &CallLog) -> &str {
    log.calls
        .last()
        .map(|record| record.call.request_id.as_str())
        .unwrap_or("unknown-request")
}

use super::{task_application::TaskApplication, *};

pub(super) struct ChallengeRequest<'a> {
    pub(super) planned: &'a ReviewPlanCase,
    pub(super) runtime: &'a CaseExecutionRuntime<'a>,
    pub(super) review: ReviewerCaseReview,
    pub(super) reviewer_route: &'a ReviewerRoute,
    pub(super) target_ids: Option<BTreeSet<StableId>>,
    pub(super) challenger_run_prefix: &'a str,
    pub(super) evidence_requests_allowed: bool,
}

pub(super) fn run_challenge(
    request: ChallengeRequest<'_>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<(ReviewerCaseReview, ChallengeOutcome), ReviewServiceError> {
    ChallengeFlow::new(request, state)?.run(state, log)
}

struct ChallengeFlow<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    review: ReviewerCaseReview,
    reviewer_route: &'a ReviewerRoute,
    challenger_run_id: StableId,
    session: ChallengeSession,
    ledger: ChallengeTaskLedger,
    conversation: Vec<NativeConversationMessage>,
    preferred_target: Option<crate::RouteTarget>,
    budget: TaskTurnBudget,
    pass: u8,
    followup: CaseCallFollowup,
    target_ids: Option<BTreeSet<StableId>>,
    evidence_requests_allowed: bool,
}

struct AppliedTurn {
    ledger_changed: bool,
    application: TaskApplication,
}

struct EvidenceTurn {
    requests: Vec<CaseEvidenceRequest>,
    calls: Vec<NativeToolCall>,
}

enum TurnProgress {
    Retry,
    Complete(Box<ChallengeOutcome>),
}

impl<'a> ChallengeFlow<'a> {
    fn new(
        request: ChallengeRequest<'a>,
        state: &CaseEvidenceState,
    ) -> Result<Self, ReviewServiceError> {
        let ChallengeRequest {
            planned,
            runtime,
            review,
            reviewer_route,
            target_ids,
            challenger_run_prefix,
            evidence_requests_allowed,
        } = request;
        let challenger_run_id = run_id(challenger_run_prefix, &planned.case.case_id)?;
        let session = ChallengeSession::new(&planned.case, &state.packet)
            .map_err(|error| failed(error.to_string()))?;
        let ledger = challenge_task_ledger(&review, target_ids.as_ref());
        Ok(Self {
            planned,
            runtime,
            review,
            reviewer_route,
            challenger_run_id,
            session,
            ledger,
            conversation: Vec::new(),
            preferred_target: None,
            budget: TaskTurnBudget::default(),
            pass: 1,
            followup: CaseCallFollowup::None,
            target_ids,
            evidence_requests_allowed,
        })
    }

    fn run(
        mut self,
        state: &mut CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<(ReviewerCaseReview, ChallengeOutcome), ReviewServiceError> {
        loop {
            let turn = self.call_and_apply(state, log)?;
            if let TurnProgress::Complete(outcome) = self.handle(turn, state, log)? {
                return Ok((self.review, *outcome));
            }
        }
    }

    fn call_and_apply(
        &mut self,
        state: &CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<AppliedTurn, ReviewServiceError> {
        let resolved = resolved_results(&state.deterministic_results);
        let context = CaseProviderContext::compile(
            &self.planned.case,
            &state.packet,
            &resolved,
            &unresolved_definitions(&state.definitions, &resolved),
        )
        .map_err(failed)?;
        let output = execute_role_call(
            RoleRequest {
                planned: self.planned,
                runtime: self.runtime,
                role: ReviewRole::Challenger,
                pass: self.pass,
                followup_attempt: self.budget.followup_turns,
                followup: self.followup,
                case_context: context,
                reviewer_review: Some(&self.review),
                challenge_target_ids: self.target_ids.as_ref(),
                preferred_target: self.preferred_target.clone(),
                evidence_round_allowed: self.evidence_requests_allowed,
                conversation: self.conversation.clone(),
            },
            state,
            log,
        )?;
        self.preferred_target = Some(output.target);
        let native_turn =
            record_native_assistant_turn(output.value, &mut self.conversation, "challenge-task")?;
        let ledger_before = self.ledger.clone();
        let application = apply_challenge_task_calls(
            &mut self.ledger,
            &native_turn.tool_calls,
            &self.planned.case,
            state,
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
        state: &mut CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let TaskApplication {
            evidence_requests,
            evidence_calls,
            tool_results,
            errors,
        } = turn.application;
        self.conversation.extend(tool_results);
        if !errors.is_empty() {
            return self.correct_task_calls(errors, turn.ledger_changed, log);
        }
        if !evidence_requests.is_empty() {
            if !self.evidence_requests_allowed {
                return Err(failed(
                    "challenge evidence requests are not authorized for this frozen comparison",
                ));
            }
            self.expand_requested_evidence(
                EvidenceTurn {
                    requests: evidence_requests,
                    calls: evidence_calls,
                },
                state,
                log,
            )?;
            return Ok(TurnProgress::Retry);
        }
        if !self.ledger.is_complete() {
            return self.request_completion(turn.ledger_changed);
        }
        self.admit_completed_challenge(state, log)
    }

    fn correct_task_calls(
        &mut self,
        errors: Vec<String>,
        made_progress: bool,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let correction = self.budget.reject(
            log,
            self.runtime.transcript,
            "Challenge task calls need correction",
            &errors,
            made_progress,
        )?;
        self.conversation.push(correction);
        self.repair_next();
        Ok(TurnProgress::Retry)
    }

    fn expand_requested_evidence(
        &mut self,
        turn: EvidenceTurn,
        state: &mut CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<(), ReviewServiceError> {
        let request_turn = case_evidence_turn(turn.requests);
        let target = self
            .preferred_target
            .as_ref()
            .ok_or_else(|| failed("challenge target was not selected"))?;
        let context = challenge_admission_context(
            &self.challenger_run_id,
            target,
            self.planned,
            state,
            &self.review,
        );
        let requests = admitted_evidence_requests(
            self.session.consume_turn(request_turn, context),
            |admitted| match admitted {
                ChallengeTurn::EvidenceRequested(requests) => Some(requests),
                ChallengeTurn::Complete(_) => None,
            },
            "challenge evidence task unexpectedly completed",
        )?;
        RequestedEvidence::new(self.planned, self.runtime, requests, turn.calls).append_to(
            state,
            &mut self.conversation,
            log,
        )?;
        self.review = run_reviewer(
            ReviewerRequest {
                planned: self.planned,
                runtime: self.runtime,
                reviewer_run_id: self.reviewer_route.run_id.clone(),
                role: ReviewRole::SingleReviewer,
                preferred_target: Some(self.reviewer_route.target.clone()),
                evidence_policy: ReviewerEvidencePolicy::Disabled,
            },
            state,
            log,
        )?
        .review;
        self.reset_identity_epoch(state)?;
        Ok(())
    }

    fn request_completion(
        &mut self,
        made_progress: bool,
    ) -> Result<TurnProgress, ReviewServiceError> {
        if self.budget.record_incomplete(made_progress) {
            return Err(failed(
                "challenge task ledger made no progress for four consecutive turns",
            ));
        }
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: render_task_correction(
                    "Challenge completion is still required",
                    &["complete_case_challenge must follow every target disposition".to_string()],
                ),
            });
        self.followup = CaseCallFollowup::AcceptedProgress;
        self.pass = 4;
        Ok(TurnProgress::Retry)
    }

    fn admit_completed_challenge(
        &mut self,
        state: &CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let turn = challenge_turn_from_ledger(&self.ledger)?;
        let target = self
            .preferred_target
            .as_ref()
            .ok_or_else(|| failed("challenge target was not selected"))?;
        let context = challenge_admission_context(
            &self.challenger_run_id,
            target,
            self.planned,
            state,
            &self.review,
        );
        match self.session.consume_turn(turn, context) {
            Ok(ChallengeTurn::Complete(challenge)) => {
                record_challenge_task_audit(
                    log,
                    &self.ledger,
                    &challenge.reviewer_review_id,
                    &challenge.challenger_run_id,
                );
                self.complete_outcome(*challenge, state, log)
            }
            Ok(ChallengeTurn::EvidenceRequested(_)) => {
                Err(failed("completed challenge ledger requested evidence"))
            }
            Err(error) => self.correct_semantic_admission(error.to_string(), log),
        }
    }

    fn complete_outcome(
        &self,
        challenge: crate::ChallengeRecord,
        state: &CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let response = respond_to_challenge(
            ResponseRequest {
                planned: self.planned,
                runtime: self.runtime,
                review: &self.review,
                reviewer_route: self.reviewer_route,
                challenge: &challenge,
            },
            state,
            log,
        )?;
        Ok(TurnProgress::Complete(Box::new(ChallengeOutcome {
            challenge,
            response,
        })))
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
                    "The completed challenge ledger failed semantic admission",
                    &[validation_error],
                ),
            });
        self.repair_next();
        Ok(TurnProgress::Retry)
    }

    fn reset_identity_epoch(
        &mut self,
        state: &CaseEvidenceState,
    ) -> Result<(), ReviewServiceError> {
        self.session = ChallengeSession::new(&self.planned.case, &state.packet)
            .map_err(|error| failed(error.to_string()))?;
        self.ledger = replace_challenge_identity_epoch(
            &mut self.conversation,
            challenge_task_ledger(&self.review, self.target_ids.as_ref()),
        );
        self.pass = 2;
        self.budget.reset_after_evidence();
        self.followup = CaseCallFollowup::None;
        Ok(())
    }

    fn repair_next(&mut self) {
        self.followup = CaseCallFollowup::HostCorrection;
        self.pass = 4;
    }
}

fn challenge_admission_context<'a>(
    challenger_run_id: &'a StableId,
    target: &'a crate::RouteTarget,
    planned: &'a ReviewPlanCase,
    state: &'a CaseEvidenceState,
    review: &'a ReviewerCaseReview,
) -> ChallengeAdmissionContext<'a> {
    ChallengeAdmissionContext {
        challenger_run_id,
        route_target: target,
        case: &planned.case,
        packet: &state.packet,
        reviewer_review: review,
    }
}

fn last_request_id(log: &CallLog) -> &str {
    log.calls
        .last()
        .map(|record| record.call.request_id.as_str())
        .unwrap_or("unknown-request")
}

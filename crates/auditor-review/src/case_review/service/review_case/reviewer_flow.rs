use super::{task_application::TaskApplication, *};

pub(super) fn run_reviewer(
    request: ReviewerRequest<'_>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<ReviewerPassOutput, ReviewServiceError> {
    ReviewerFlow::new(request, state)?.run(state, log)
}

pub(super) struct ReviewerPassOutput {
    pub(super) review: ReviewerCaseReview,
    pub(super) target: crate::RouteTarget,
    pub(super) evidence_requests: Vec<CaseEvidenceRequest>,
}

pub(super) struct EnsembleReviewerPassA<'a> {
    flow: ReviewerFlow<'a>,
    pub(super) output: ReviewerPassOutput,
}

pub(super) fn run_ensemble_reviewer_pass_a<'a>(
    request: ReviewerRequest<'a>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<EnsembleReviewerPassA<'a>, ReviewServiceError> {
    let mut flow = ReviewerFlow::new(request, state)?;
    let output = flow.run_phase(state, log)?;
    Ok(EnsembleReviewerPassA { flow, output })
}

pub(super) fn resume_ensemble_reviewer(
    mut suspended: EnsembleReviewerPassA<'_>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<ReviewerPassOutput, ReviewServiceError> {
    suspended.flow.resume_with_shared_evidence(state)?;
    suspended.flow.run_phase(state, log)
}

struct ReviewerFlow<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    reviewer_run_id: StableId,
    role: ReviewRole,
    preferred_target: Option<crate::RouteTarget>,
    evidence_policy: ReviewerEvidencePolicy,
    shadow_pass_a: bool,
    captured_evidence_requests: Vec<CaseEvidenceRequest>,
    session: SingleReviewerSession,
    ledger: ReviewerTaskLedger,
    conversation: Vec<NativeConversationMessage>,
    budget: TaskTurnBudget,
    pass: u8,
    followup: CaseCallFollowup,
    evidence_boundary_audit_delivered: bool,
    recommendation_coverage_audit_delivered: bool,
}

struct AppliedTurn {
    target: crate::RouteTarget,
    resolved: Vec<crate::ProbeResult>,
    ledger_changed: bool,
    application: TaskApplication,
}

struct EvidenceTurn {
    target: crate::RouteTarget,
    resolved: Vec<crate::ProbeResult>,
    requests: Vec<CaseEvidenceRequest>,
    calls: Vec<NativeToolCall>,
}

enum TurnProgress {
    Retry,
    Complete(Box<ReviewerCaseReview>, crate::RouteTarget),
}

impl<'a> ReviewerFlow<'a> {
    fn new(
        request: ReviewerRequest<'a>,
        state: &CaseEvidenceState,
    ) -> Result<Self, ReviewServiceError> {
        let ReviewerRequest {
            planned,
            runtime,
            reviewer_run_id,
            role,
            preferred_target,
            evidence_policy,
        } = request;
        let session = SingleReviewerSession::new(&planned.case, &state.packet)
            .map_err(|error| failed(error.to_string()))?;
        let ledger = ReviewerTaskLedger::new(reviewer_requirements(planned, state)?);
        Ok(Self {
            planned,
            runtime,
            reviewer_run_id,
            role,
            preferred_target,
            evidence_policy,
            shadow_pass_a: evidence_policy == ReviewerEvidencePolicy::DeferForUnion,
            captured_evidence_requests: Vec::new(),
            session,
            ledger,
            conversation: Vec::new(),
            budget: TaskTurnBudget::default(),
            pass: if evidence_policy == ReviewerEvidencePolicy::Disabled {
                2
            } else {
                1
            },
            followup: CaseCallFollowup::None,
            evidence_boundary_audit_delivered: false,
            recommendation_coverage_audit_delivered: false,
        })
    }

    fn run(
        mut self,
        state: &mut CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<ReviewerPassOutput, ReviewServiceError> {
        self.run_phase(state, log)
    }

    fn run_phase(
        &mut self,
        state: &mut CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<ReviewerPassOutput, ReviewServiceError> {
        loop {
            let turn = self.call_and_apply(state, log)?;
            if let TurnProgress::Complete(review, target) = self.handle(turn, state, log)? {
                return Ok(ReviewerPassOutput {
                    review: *review,
                    target,
                    evidence_requests: self.captured_evidence_requests.clone(),
                });
            }
        }
    }

    fn resume_with_shared_evidence(
        &mut self,
        state: &CaseEvidenceState,
    ) -> Result<(), ReviewServiceError> {
        self.session = SingleReviewerSession::new(&self.planned.case, &state.packet)
            .map_err(|error| failed(error.to_string()))?;
        self.evidence_policy = ReviewerEvidencePolicy::Disabled;
        self.shadow_pass_a = false;
        self.pass = 2;
        self.followup = CaseCallFollowup::None;
        self.budget.reset_after_evidence();
        self.evidence_boundary_audit_delivered = false;
        self.recommendation_coverage_audit_delivered = false;
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: "The host has resolved the blind reviewers' allowed evidence union. Reconsider your admitted Pass A ledger against the complete shared packet now shown in the case context. Preserve correct records; use native revision, withdrawal, or reopen tools for changes; then call complete_case_review last. No further evidence request is permitted."
                    .to_string(),
            });
        Ok(())
    }

    fn call_and_apply(
        &mut self,
        state: &CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<AppliedTurn, ReviewServiceError> {
        let resolved = resolved_results(&state.deterministic_results);
        let unresolved = unresolved_definitions(&state.definitions, &resolved);
        let context =
            CaseProviderContext::compile(&self.planned.case, &state.packet, &resolved, &unresolved)
                .map_err(failed)?;
        let probe_contracts = context.model_probe_contracts.clone();
        let output = execute_role_call(
            RoleRequest {
                planned: self.planned,
                runtime: self.runtime,
                role: self.role,
                pass: self.pass,
                followup_attempt: self.budget.followup_turns,
                followup: self.followup,
                case_context: context,
                reviewer_review: None,
                challenge_target_ids: None,
                preferred_target: self.preferred_target.clone(),
                evidence_round_allowed: self.evidence_policy != ReviewerEvidencePolicy::Disabled,
                conversation: self.conversation.clone(),
            },
            state,
            log,
        )?;
        let target = output.target;
        self.preferred_target = Some(target.clone());
        let native_turn =
            record_native_assistant_turn(output.value, &mut self.conversation, "case-task")?;
        let ledger_before = self.ledger.clone();
        let application = apply_reviewer_task_calls(
            &mut self.ledger,
            ReviewerInput {
                calls: &native_turn.tool_calls,
                case: &self.planned.case,
                state,
                probe_contracts: &probe_contracts,
                request_id: last_request_id(log),
            },
        );
        Ok(AppliedTurn {
            target,
            resolved,
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
        let AppliedTurn {
            target,
            resolved,
            ledger_changed,
            application:
                TaskApplication {
                    evidence_requests,
                    evidence_calls,
                    tool_results,
                    errors,
                },
        } = turn;
        self.conversation.extend(tool_results);
        if !errors.is_empty() {
            return self.correct_task_calls(errors, ledger_changed, log);
        }
        if !evidence_requests.is_empty() {
            self.expand_requested_evidence(
                EvidenceTurn {
                    target,
                    resolved,
                    requests: evidence_requests,
                    calls: evidence_calls,
                },
                state,
                log,
            )?;
            return Ok(TurnProgress::Retry);
        }
        if self.ledger.all_probe_answers_admitted()
            && self.ledger.needs_evidence_boundary_audit()
            && !self.evidence_boundary_audit_delivered
        {
            return Ok(self.request_evidence_boundary_audit());
        }
        if !self.ledger.is_complete() {
            return self.request_completion(ledger_changed);
        }
        if self.evidence_boundary_audit_delivered
            && !self.recommendation_coverage_audit_delivered
            && !self
                .ledger
                .recommendations()
                .values()
                .any(|record| record.active)
        {
            let conclusion = reviewer_conclusion_from_ledger(
                &self.ledger,
                &self.planned.case,
                &state.definitions,
                &resolved,
            )?;
            if let Some(family) = recommendation_family_for_conclusion(conclusion) {
                return Ok(self.request_recommendation_coverage_audit(conclusion, family));
            }
        }
        self.admit_completed_review(target, &resolved, state, log)
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
            "Reviewer task calls need correction",
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
        if self.evidence_policy == ReviewerEvidencePolicy::DeferForUnion {
            self.captured_evidence_requests.extend(turn.requests);
            self.captured_evidence_requests.sort_by(|left, right| {
                (&left.probe_id, &left.target_id, &left.decision_relevance).cmp(&(
                    &right.probe_id,
                    &right.target_id,
                    &right.decision_relevance,
                ))
            });
            self.captured_evidence_requests.dedup();
            self.conversation.extend(turn.calls.iter().map(|call| {
                task_tool_result(
                    call,
                    "Accepted",
                    "The request was queued for the blind shared-evidence union. Complete Pass A from the initial packet; union results are withheld until Pass B.",
                )
            }));
            self.evidence_policy = ReviewerEvidencePolicy::Disabled;
            self.followup = CaseCallFollowup::AcceptedProgress;
            self.budget.record_progress_followup();
            return Ok(());
        }
        let request_turn = case_evidence_turn(turn.requests);
        let requests =
            self.admit_evidence_request(request_turn, &turn.target, &turn.resolved, state)?;
        RequestedEvidence::new(self.planned, self.runtime, requests, turn.calls).append_to(
            state,
            &mut self.conversation,
            log,
        )?;
        self.pass = 2;
        self.followup = CaseCallFollowup::None;
        Ok(())
    }

    fn admit_evidence_request(
        &mut self,
        request: CaseProviderTurn,
        target: &crate::RouteTarget,
        resolved: &[crate::ProbeResult],
        state: &CaseEvidenceState,
    ) -> Result<Vec<CaseEvidenceRequest>, ReviewServiceError> {
        let context = reviewer_admission_context(
            &self.reviewer_run_id,
            self.planned,
            target,
            resolved,
            state,
        );
        match self.session.consume_turn(request, context) {
            Ok(SingleReviewerTurn::EvidenceRequested(requests)) => Ok(requests),
            Ok(SingleReviewerTurn::Complete(_)) => {
                Err(failed("evidence task unexpectedly completed the review"))
            }
            Err(error) => Err(failed(error.to_string())),
        }
    }

    fn request_completion(
        &mut self,
        made_progress: bool,
    ) -> Result<TurnProgress, ReviewServiceError> {
        if self.budget.record_incomplete(made_progress) {
            return Err(failed(
                "reviewer task ledger made no progress for four consecutive turns",
            ));
        }
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: render_task_correction(
                    "Reviewer completion is still required",
                    &[
                        "complete_case_review must be the last call after all tasks are covered"
                            .to_string(),
                    ],
                ),
            });
        self.followup = CaseCallFollowup::AcceptedProgress;
        self.pass = 4;
        Ok(TurnProgress::Retry)
    }

    fn request_evidence_boundary_audit(&mut self) -> TurnProgress {
        self.evidence_boundary_audit_delivered = true;
        self.budget.record_progress_followup();
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: render_task_correction(
                    "Reviewer evidence-boundary audit is required",
                    &[
                        "The admitted probe ledger mixes definitive yes/no answers with unknown or conflicting answers. Before completing, re-read every definitive answer against that probe's positive or negative boundary and its cited evidence."
                            .to_string(),
                        "Capability or possibility is not proof: may, can, could, potentially, a suggestive name, mutable access, or an opaque callable does not establish mutation, different inputs or state, a consumer freshness requirement, or retention risk. If the callable body, an explicit behavioral contract, or runtime evidence does not establish the selected boundary, use revise_case_probe_answer to change the answer to unknown."
                            .to_string(),
                        "Revise any dependent claims or recommendations made invalid by an answer revision. Then call complete_case_review last. If no revision is necessary, call complete_case_review again to confirm the audited ledger."
                            .to_string(),
                    ],
                ),
            });
        self.followup = CaseCallFollowup::AcceptedProgress;
        self.pass = 4;
        TurnProgress::Retry
    }

    fn request_recommendation_coverage_audit(
        &mut self,
        conclusion: crate::CaseConclusion,
        family: &'static str,
    ) -> TurnProgress {
        self.recommendation_coverage_audit_delivered = true;
        self.budget.record_progress_followup();
        self.conversation
            .push(NativeConversationMessage::UserContinuation {
                content_markdown: render_task_correction(
                    "Reviewer recommendation coverage audit is required",
                    &[
                        format!(
                            "The host-derived conclusion is `{}`. Its mapped action family is `{family}`, but the admitted ledger has no active recommendation.",
                            case_conclusion_name(conclusion)
                        ),
                        format!(
                            "Call record_case_recommendation with a new recommendation_ref and family `{family}`, grounding the action and its verification in admitted evidence, then call complete_case_review last. Do not repeat accepted probe or claim create calls."
                        ),
                        if self.ledger.claims().values().any(|claim| claim.active) {
                            "Its premise_claim_refs must cite active claim refs from the ledger.".to_string()
                        } else {
                            "The ledger has no active claim, and premise_claim_refs must cite one: first call record_case_claim with kind `recommendation_premise`, a new claim_ref, and the probes that bound the action, then cite that claim_ref.".to_string()
                        },
                        "This action-bearing conclusion cannot be admitted without its mapped recommendation. If the evidence does not support that conclusion and action, revise the relevant probe answers or claims first."
                            .to_string(),
                    ],
                ),
            });
        self.followup = CaseCallFollowup::AcceptedProgress;
        self.pass = 4;
        TurnProgress::Retry
    }

    fn admit_completed_review(
        &mut self,
        target: crate::RouteTarget,
        resolved: &[crate::ProbeResult],
        state: &CaseEvidenceState,
        log: &mut CallLog,
    ) -> Result<TurnProgress, ReviewServiceError> {
        let turn = reviewer_turn_from_ledger(
            &self.ledger,
            &self.planned.case,
            &state.definitions,
            resolved,
        )?;
        let context = reviewer_admission_context(
            &self.reviewer_run_id,
            self.planned,
            &target,
            resolved,
            state,
        );
        let admitted = if self.shadow_pass_a {
            let mut session = SingleReviewerSession::new(&self.planned.case, &state.packet)
                .map_err(|error| failed(error.to_string()))?;
            session.consume_turn(turn, context)
        } else {
            self.session.consume_turn(turn, context)
        };
        match admitted {
            Ok(SingleReviewerTurn::Complete(review)) => {
                if !self.shadow_pass_a {
                    record_reviewer_task_audit(log, &self.ledger);
                }
                Ok(TurnProgress::Complete(review, target))
            }
            Ok(SingleReviewerTurn::EvidenceRequested(_)) => {
                Err(failed("completed reviewer ledger requested evidence"))
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
                    "The completed reviewer ledger failed semantic admission",
                    &[
                        validation_error,
                        "If the derived conclusion requires a missing recommendation, call record_case_recommendation with a new recommendation_ref and the required family, then call complete_case_review last."
                            .to_string(),
                        "If an admitted recommendation needs correction, call revise_case_recommendation with its current revision and a complete replacement. Do not repeat record_case_recommendation for the same recommendation_ref."
                            .to_string(),
                    ],
                ),
            });
        self.repair_next();
        Ok(TurnProgress::Retry)
    }

    fn repair_next(&mut self) {
        self.followup = CaseCallFollowup::HostCorrection;
        self.pass = 4;
    }
}

fn recommendation_family_for_conclusion(conclusion: crate::CaseConclusion) -> Option<&'static str> {
    crate::case_review::recommendation::required_recommendation_family(conclusion)
        .map(recommendation_family_name)
}

fn recommendation_family_name(family: crate::RecommendationFamily) -> &'static str {
    match family {
        crate::RecommendationFamily::LifecycleReownership => "lifecycle_reownership",
        crate::RecommendationFamily::ExplicitRetentionAvoidance => "explicit_retention_avoidance",
        crate::RecommendationFamily::MeasurementBeforeChange => "measurement_before_change",
        crate::RecommendationFamily::SharedRootRemediation => "shared_root_remediation",
        crate::RecommendationFamily::PreserveIndependentActions => "preserve_independent_actions",
        crate::RecommendationFamily::ConsolidatedRemediation => "consolidated_remediation",
        crate::RecommendationFamily::SequencedRemediation => "sequenced_remediation",
        crate::RecommendationFamily::NarrowedRemediation => "narrowed_remediation",
        crate::RecommendationFamily::DeferredRemediation => "deferred_remediation",
    }
}

fn case_conclusion_name(conclusion: crate::CaseConclusion) -> String {
    serde_json::to_value(conclusion)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{conclusion:?}"))
}

fn reviewer_admission_context<'a>(
    reviewer_run_id: &'a StableId,
    planned: &'a ReviewPlanCase,
    target: &'a crate::RouteTarget,
    resolved: &'a [crate::ProbeResult],
    state: &'a CaseEvidenceState,
) -> ReviewerAdmissionContext<'a> {
    ReviewerAdmissionContext {
        reviewer_run_id,
        route_target: target,
        case: &planned.case,
        packet: &state.packet,
        probe_definitions: &state.definitions,
        deterministic_probe_results: resolved,
    }
}

fn reviewer_requirements(
    planned: &ReviewPlanCase,
    state: &CaseEvidenceState,
) -> Result<ReviewerTaskRequirements, ReviewServiceError> {
    let resolved = resolved_results(&state.deterministic_results);
    let unresolved = unresolved_definitions(&state.definitions, &resolved);
    let mut claim_slots = planned
        .case
        .claim_slots
        .iter()
        .map(enum_name)
        .collect::<Result<BTreeSet<_>, _>>()?;
    claim_slots.remove("lifecycle_conclusion");
    Ok(ReviewerTaskRequirements {
        probe_ids: unresolved
            .iter()
            .map(|probe| probe.probe_id.to_string())
            .collect(),
        claim_slots,
        recommendation_families: planned
            .case
            .recommendation_families
            .iter()
            .map(enum_name)
            .collect::<Result<_, _>>()?,
    })
}

fn last_request_id(log: &CallLog) -> &str {
    log.calls
        .last()
        .map(|record| record.call.request_id.as_str())
        .unwrap_or("unknown-request")
}

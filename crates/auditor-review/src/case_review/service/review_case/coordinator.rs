use super::*;

struct ProviderOutcome {
    review: ReviewerCaseReview,
    challenge: Option<ChallengeOutcome>,
    finalization: FinalizedCase,
}

struct CompletionParts {
    results: Vec<crate::ProbeResult>,
    review: Option<ReviewerCaseReview>,
    challenge: Option<ChallengeOutcome>,
    finalization: FinalizedCase,
}

pub(in crate::case_review::service) fn execute_planned_case(
    planned: &ReviewPlanCase,
    runtime: CaseExecutionRuntime<'_>,
) -> Result<CompletedCase, ReviewServiceError> {
    validate_runnable_case(planned)?;
    let mut state = initial_evidence_state(planned);
    let mut log = CallLog::default();
    let production = execute_provider_flow(planned, &runtime, &mut state, &mut log);
    let (shadow, shadow_log) = if planned.routing.selection.mode == DeliberationMode::Ensemble {
        let mut shadow_log = CallLog::default();
        let shadow = run_shadow_ensemble(planned, &runtime, &mut shadow_log);
        (Some(shadow), shadow_log)
    } else {
        (None, CallLog::default())
    };
    let mut completed = match production {
        Ok(outcome) => Ok(complete_success(state, log, outcome)),
        Err(ReviewServiceError::Cancelled) => Err(ReviewServiceError::Cancelled),
        Err(ReviewServiceError::Failed(message)) => complete_failure(planned, state, log, &message),
    }?;
    if let Some(shadow) = shadow {
        match shadow {
            Ok(success) => {
                completed.ensemble_run = Some(success.run);
                completed.ensemble_evidence_packets = success.packets;
                completed.ensemble_challenges = success.challenges;
            }
            Err(failure) => completed.ensemble_failure = Some(failure),
        }
        absorb_shadow_log(&mut completed, shadow_log);
    }
    Ok(completed)
}

fn validate_runnable_case(planned: &ReviewPlanCase) -> Result<(), ReviewServiceError> {
    if matches!(
        planned.routing.selection.mode,
        DeliberationMode::SingleReviewer | DeliberationMode::Ensemble
    ) && planned.routing.closure.is_none()
    {
        return Ok(());
    }
    Err(failed(format!(
        "review execution requires a runnable single-reviewer or shadow-ensemble case; {} planned {:?}",
        planned.case.case_id, planned.routing.selection.mode
    )))
}

struct ShadowEnsembleSuccess {
    run: crate::EnsembleCaseRun,
    packets: Vec<crate::EvidencePacket>,
    challenges: Vec<crate::EnsembleChallengeExecution>,
}

fn run_shadow_ensemble(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    log: &mut CallLog,
) -> Result<ShadowEnsembleSuccess, crate::EnsembleCaseFailure> {
    let initial = initial_evidence_state(planned);
    let mut suspended = Vec::new();
    let mut pass_a = Vec::new();
    let mut completed_slots = Vec::new();
    for (slot, role, prefix) in [
        (
            crate::EnsembleReviewerSlot::ReviewerA,
            ReviewRole::ReviewerA,
            "reviewer-a-run",
        ),
        (
            crate::EnsembleReviewerSlot::ReviewerB,
            ReviewRole::ReviewerB,
            "reviewer-b-run",
        ),
        (
            crate::EnsembleReviewerSlot::ReviewerC,
            ReviewRole::ReviewerC,
            "reviewer-c-run",
        ),
    ] {
        let mut reviewer_state = initial.clone();
        let reviewer_run_id = run_id(prefix, &planned.case.case_id).map_err(|_| {
            ensemble_failure(
                planned,
                crate::EnsembleFailureStage::PassA,
                &completed_slots,
                "reviewer_run_identity_failed",
            )
        })?;
        let pass = run_ensemble_reviewer_pass_a(
            ReviewerRequest {
                planned,
                runtime,
                reviewer_run_id,
                role,
                preferred_target: None,
                evidence_policy: ReviewerEvidencePolicy::DeferForUnion,
            },
            &mut reviewer_state,
            log,
        )
        .map_err(|_| {
            ensemble_failure(
                planned,
                crate::EnsembleFailureStage::PassA,
                &completed_slots,
                "reviewer_pass_a_incomplete",
            )
        })?;
        pass_a.push(crate::EnsembleReviewerPass {
            reviewer: slot,
            pass: 1,
            packet_digest: initial.packet.packet_digest.clone(),
            route_target: pass.output.target.clone(),
            prompt_identity: runtime.prompts.bundle_id.clone(),
            evidence_requests: pass.output.evidence_requests.clone(),
            review: pass.output.review.clone(),
        });
        completed_slots.push(slot);
        suspended.push((slot, pass));
    }

    let mut shared = shared_evidence(planned, runtime, &initial, &pass_a)
        .map_err(|(stage, code)| ensemble_failure(planned, stage, &completed_slots, code))?;
    let mut pass_b = Vec::new();
    for (slot, reviewer) in suspended {
        let pass = resume_ensemble_reviewer(reviewer, &mut shared, log).map_err(|_| {
            ensemble_failure(
                planned,
                crate::EnsembleFailureStage::PassB,
                &completed_slots,
                "reviewer_pass_b_incomplete",
            )
        })?;
        pass_b.push(crate::EnsembleReviewerPass {
            reviewer: slot,
            pass: 2,
            packet_digest: shared.packet.packet_digest.clone(),
            route_target: pass.target,
            prompt_identity: runtime.prompts.bundle_id.clone(),
            evidence_requests: Vec::new(),
            review: pass.review,
        });
    }
    let run = crate::EnsembleCaseRun::build(
        planned.case.case_id.clone(),
        initial.packet.packet_digest,
        shared.packet.packet_digest.clone(),
        pass_a,
        pass_b,
    )
    .map_err(|_| {
        ensemble_failure(
            planned,
            crate::EnsembleFailureStage::PassB,
            &completed_slots,
            "ensemble_admission_failed",
        )
    })?;
    let comparison =
        crate::compare_ensemble_run(&run, &shared.deterministic_results).map_err(|_| {
            ensemble_failure(
                planned,
                crate::EnsembleFailureStage::PassB,
                &completed_slots,
                "ensemble_comparison_failed",
            )
        })?;
    let challenges = run_comparison_challenges(
        ComparisonChallengeContext {
            planned,
            runtime,
            shared: &shared,
            run: &run,
            comparison: &comparison,
        },
        log,
    );
    let packets = (shared.packet.packet_digest != planned.evidence_packet.packet_digest)
        .then_some(shared.packet)
        .into_iter()
        .collect();
    Ok(ShadowEnsembleSuccess {
        run,
        packets,
        challenges,
    })
}

/// The evidence state both ensemble passes share: the initial packet,
/// expanded with the union of pass-A evidence requests.
fn shared_evidence(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    initial: &CaseEvidenceState,
    pass_a: &[crate::EnsembleReviewerPass],
) -> Result<CaseEvidenceState, (crate::EnsembleFailureStage, &'static str)> {
    let probe_ids = probe_ids(&initial.definitions);
    let neighborhood = &initial.packet.allowed_neighborhood;
    let target_ids = neighborhood
        .map_symbol_ids
        .iter()
        .cloned()
        .chain(
            neighborhood
                .finding_ids
                .iter()
                .map(|id| format!("finding:{id}")),
        )
        .collect::<BTreeSet<_>>();
    let reviewer_requests = pass_a.iter().flat_map(|pass| {
        pass.evidence_requests
            .iter()
            .cloned()
            .map(move |request| crate::ReviewerEvidenceRequest {
                reviewer: pass.reviewer,
                request,
            })
    });
    let union = crate::plan_evidence_union(
        reviewer_requests,
        crate::EvidenceUnionBoundary {
            case_id: &planned.case.case_id,
            probe_ids: &probe_ids,
            target_ids: &target_ids,
            maximum_requests: initial.packet.budget.maximum_lookup_requests,
        },
    )
    .map_err(|_| {
        (
            crate::EnsembleFailureStage::EvidenceUnion,
            "evidence_union_rejected",
        )
    })?;
    let mut shared = initial.clone();
    if !union.requests.is_empty() {
        let requests = union
            .requests
            .into_iter()
            .map(|request| request.request)
            .collect();
        expand_union_evidence(planned, runtime, &mut shared, requests).map_err(|_| {
            (
                crate::EnsembleFailureStage::SharedPacket,
                "shared_packet_expansion_failed",
            )
        })?;
    }
    Ok(shared)
}

struct ComparisonChallengeContext<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    shared: &'a CaseEvidenceState,
    run: &'a crate::EnsembleCaseRun,
    comparison: &'a crate::EnsembleComparison,
}

fn run_comparison_challenges(
    context: ComparisonChallengeContext<'_>,
    log: &mut CallLog,
) -> Vec<crate::EnsembleChallengeExecution> {
    let ComparisonChallengeContext {
        planned,
        runtime,
        shared,
        run,
        comparison,
    } = context;
    let comparison_target_ids = comparison
        .challenge_targets
        .iter()
        .map(|target| target.target_id.clone())
        .collect::<BTreeSet<_>>();
    let mut executions = Vec::new();
    for pass in &run.pass_b {
        let target_ids = comparison_target_ids
            .intersection(&owned_record_ids(&pass.review))
            .cloned()
            .collect::<BTreeSet<_>>();
        if target_ids.is_empty() {
            continue;
        }
        let route = ReviewerRoute {
            run_id: pass.review.reviewer_run_id.clone(),
            target: pass.route_target.clone(),
        };
        let mut state = shared.clone();
        let result = run_challenge(
            ChallengeRequest {
                planned,
                runtime,
                review: pass.review.clone(),
                reviewer_route: &route,
                target_ids: Some(target_ids.clone()),
                challenger_run_prefix: challenger_prefix(pass.reviewer),
                evidence_requests_allowed: false,
            },
            &mut state,
            log,
        );
        let review_id = pass.review.review_id.clone();
        let targets = target_ids.into_iter().collect();
        let execution = match result {
            Ok((review, outcome))
                if review == pass.review && keeps_comparison_families(comparison, &outcome) =>
            {
                crate::EnsembleChallengeExecution::complete(
                    comparison,
                    pass.reviewer,
                    review_id,
                    targets,
                    outcome,
                )
            }
            other => crate::EnsembleChallengeExecution::failed(
                comparison,
                pass.reviewer,
                review_id,
                targets,
                challenge_failure_code(&other, &pass.review),
            ),
        };
        executions.push(execution);
    }
    executions
}

/// The claim and recommendation ids a review owns.
fn owned_record_ids(review: &ReviewerCaseReview) -> BTreeSet<crate::StableId> {
    review
        .claims
        .iter()
        .map(|claim| claim.claim_id.clone())
        .chain(
            review
                .recommendations
                .iter()
                .map(|recommendation| recommendation.recommendation_id.clone()),
        )
        .collect()
}

fn challenger_prefix(slot: crate::EnsembleReviewerSlot) -> &'static str {
    match slot {
        crate::EnsembleReviewerSlot::ReviewerA => "ensemble-challenger-a-run",
        crate::EnsembleReviewerSlot::ReviewerB => "ensemble-challenger-b-run",
        crate::EnsembleReviewerSlot::ReviewerC => "ensemble-challenger-c-run",
    }
}

/// Whether a challenge's replacement recommendations stay within the
/// compared recommendation families.
fn keeps_comparison_families(
    comparison: &crate::EnsembleComparison,
    outcome: &ChallengeOutcome,
) -> bool {
    crate::validate_comparison_recommendation_families(
        &comparison.recommendation_families,
        &outcome.replacement_recommendations(),
    )
    .is_ok()
}

/// Why a comparison challenge did not complete.
fn challenge_failure_code<O>(
    result: &Result<(ReviewerCaseReview, O), ReviewServiceError>,
    review: &ReviewerCaseReview,
) -> &'static str {
    match result {
        Ok((returned, _)) if returned == review => "comparison_challenge_introduced_family",
        Ok(_) => "comparison_challenge_mutated_review",
        Err(ReviewServiceError::Cancelled) => "comparison_challenge_cancelled",
        Err(ReviewServiceError::Failed(_)) => "comparison_challenge_failed",
    }
}

fn ensemble_failure(
    planned: &ReviewPlanCase,
    stage: crate::EnsembleFailureStage,
    completed_slots: &[crate::EnsembleReviewerSlot],
    detail_code: &str,
) -> crate::EnsembleCaseFailure {
    crate::EnsembleCaseFailure::new(
        planned.case.case_id.clone(),
        stage,
        completed_slots.to_vec(),
        detail_code,
    )
    .expect("static ensemble failure code is valid")
}

fn absorb_shadow_log(completed: &mut CompletedCase, mut log: CallLog) {
    completed.calls.append(&mut log.calls);
    completed.egress.append(&mut log.egress);
    completed
        .claim_slot_dispositions
        .append(&mut log.claim_slot_dispositions);
    completed
        .recommendation_family_dispositions
        .append(&mut log.recommendation_family_dispositions);
    completed
        .challenge_target_dispositions
        .append(&mut log.challenge_target_dispositions);
    completed
        .semantic_revisions
        .append(&mut log.semantic_revisions);
    completed
        .projection_audits
        .append(&mut log.projection_audits);
}

fn initial_evidence_state(planned: &ReviewPlanCase) -> CaseEvidenceState {
    CaseEvidenceState {
        packet: planned.evidence_packet.clone(),
        packet_history: Vec::new(),
        definitions: planned.probe_definitions.clone(),
        deterministic_results: planned.deterministic_probe_results.clone(),
        lookup_items: planned
            .evidence_packet
            .items
            .iter()
            .filter(|item| item.kind == crate::EvidenceItemKind::SourceSpan)
            .cloned()
            .collect(),
    }
}

fn execute_provider_flow(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<ProviderOutcome, ReviewServiceError> {
    let reviewer_run_id = run_id("reviewer-run", &planned.case.case_id)?;
    let reviewer = run_reviewer(
        ReviewerRequest {
            planned,
            runtime,
            reviewer_run_id: reviewer_run_id.clone(),
            role: ReviewRole::SingleReviewer,
            preferred_target: None,
            evidence_policy: ReviewerEvidencePolicy::ResolveImmediately,
        },
        state,
        log,
    )?;
    let review = reviewer.review;
    let target = reviewer.target;
    let route = ReviewerRoute {
        run_id: reviewer_run_id,
        target,
    };
    let (review, challenge) = challenge_when_required(
        ChallengeRequest {
            planned,
            runtime,
            review,
            reviewer_route: &route,
            target_ids: None,
            challenger_run_prefix: "challenger-run",
            evidence_requests_allowed: true,
        },
        state,
        log,
    )?;
    let finalization = finalize_review(planned, runtime, state, &review, challenge.as_ref())?;
    Ok(ProviderOutcome {
        review,
        challenge,
        finalization,
    })
}

fn challenge_when_required(
    request: ChallengeRequest<'_>,
    state: &mut CaseEvidenceState,
    log: &mut CallLog,
) -> Result<(ReviewerCaseReview, Option<ChallengeOutcome>), ReviewServiceError> {
    if !request
        .review
        .recommendations
        .iter()
        .any(requires_mandatory_challenge)
    {
        return Ok((request.review, None));
    }
    checkpoint(request.runtime.control, ReviewStage::Challenging)?;
    let (review, outcome) = run_challenge(request, state, log)?;
    Ok((review, Some(outcome)))
}

fn finalize_review(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &CaseEvidenceState,
    review: &ReviewerCaseReview,
    challenge: Option<&ChallengeOutcome>,
) -> Result<FinalizedCase, ReviewServiceError> {
    checkpoint(runtime.control, ReviewStage::Finalizing)?;
    let allowed_probe_ids = probe_ids(&state.definitions);
    FinalizedCase::finalize(CaseFinalizationContext {
        case: &planned.case,
        review,
        challenge: challenge.map(|outcome| &outcome.challenge),
        response: challenge.and_then(|outcome| outcome.response.as_ref()),
        allowed_probe_ids: &allowed_probe_ids,
    })
    .map_err(|error| failed(error.to_string()))
}

fn complete_success(
    state: CaseEvidenceState,
    log: CallLog,
    outcome: ProviderOutcome,
) -> CompletedCase {
    let results = merge_probe_results(
        &state.deterministic_results,
        &outcome.review.model_resolved_probes,
    );
    completed_case(
        state,
        log,
        CompletionParts {
            results,
            review: Some(outcome.review),
            challenge: outcome.challenge,
            finalization: outcome.finalization,
        },
    )
}

fn complete_failure(
    planned: &ReviewPlanCase,
    state: CaseEvidenceState,
    log: CallLog,
    message: &str,
) -> Result<CompletedCase, ReviewServiceError> {
    let provider_failures = log
        .calls
        .iter()
        .filter_map(|call| call.failure_class)
        .collect::<Vec<_>>();
    let failure = classify_case_failure(message, &provider_failures);
    let closure = CaseClosure::failed(
        planned.case.case_id.clone(),
        failure,
        case_failure_detail_codes(failure, message, &provider_failures),
    );
    let finalization =
        FinalizedCase::failed(&planned.case, closure).map_err(|error| failed(error.to_string()))?;
    let results = state.deterministic_results.clone();
    Ok(completed_case(
        state,
        log,
        CompletionParts {
            results,
            review: None,
            challenge: None,
            finalization,
        },
    ))
}

fn completed_case(state: CaseEvidenceState, log: CallLog, parts: CompletionParts) -> CompletedCase {
    CompletedCase {
        packet: state.packet,
        packet_history: state.packet_history,
        definitions: state.definitions,
        results: parts.results,
        review: parts.review,
        ensemble_run: None,
        ensemble_failure: None,
        ensemble_evidence_packets: Vec::new(),
        ensemble_challenges: Vec::new(),
        challenge: parts.challenge,
        finalization: parts.finalization,
        calls: log.calls,
        egress: log.egress,
        claim_slot_dispositions: log.claim_slot_dispositions,
        recommendation_family_dispositions: log.recommendation_family_dispositions,
        challenge_target_dispositions: log.challenge_target_dispositions,
        semantic_revisions: log.semantic_revisions,
        projection_audits: log.projection_audits,
    }
}

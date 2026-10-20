use std::collections::BTreeSet;

use auditor_review::{
    bound_challenge_response_payload_schema, CaseFinalizationContext, CaseQualificationBinding,
    CaseQualificationRecord, ChallengeAdmissionContext, ChallengeError, ChallengeObjection,
    ChallengeOutcome, ChallengeRecord, ChallengeRecordPayload, ChallengeResponse,
    ChallengeResponseAdmission, ChallengeResponsePayload, ChallengeSession, ChallengeStatus,
    ChallengeTargetType, ChallengeTurn, ClaimStatus, EngineeringReviewV2, EngineeringReviewV2Draft,
    FinalRecommendationDisposition, FinalizedCase, Materiality, ObjectionReason, ObjectionResponse,
    ProviderFailureClass, ResponseAction, ReviewRole, RouteTarget, SemanticQualificationCase,
    StableId, PROVIDER_ADAPTER_VERSION, REVIEW_QUALIFICATION_V2_SCHEMA_VERSION,
};

use super::spec09_tests::{review_payload, review_route_target, reviewer_run_id, ReviewFixture};
use super::*;

fn challenger_run_id() -> StableId {
    StableId::from_identity(
        "challenger-run",
        1,
        &json!({"fixture": "spec-10-challenger"}),
    )
    .unwrap()
}

fn challenger_target() -> RouteTarget {
    RouteTarget {
        provider_id: "fixture-challenger".to_string(),
        model: "fixture-challenger-model".to_string(),
    }
}

fn qualification(
    fixture: &ReviewFixture,
    target: RouteTarget,
    role: ReviewRole,
) -> CaseQualificationRecord {
    let mut probe_kinds = fixture
        .definitions
        .iter()
        .map(|definition| definition.kind)
        .collect::<Vec<_>>();
    probe_kinds.sort();
    probe_kinds.dedup();
    let binding = CaseQualificationBinding {
        target,
        role,
        case_kind: fixture.case.case_kind,
        probe_kinds,
        prompt_bundle_id: StableId::from_identity(
            "review-prompt-bundle",
            2,
            &json!({"fixture": "spec-10"}),
        )
        .unwrap(),
        tool_schema_digest: auditor_review::case_tool_schema_digest(),
        renderer_version: auditor_review::MODEL_MARKDOWN_RENDERER_VERSION,
        adapter_version: PROVIDER_ADAPTER_VERSION,
    };
    CaseQualificationRecord {
        schema_version: REVIEW_QUALIFICATION_V2_SCHEMA_VERSION,
        qualification_id: binding.qualification_id(),
        binding,
        transport_valid: true,
        native_protocol_valid: true,
        semantic_output_valid: true,
        semantic_cases_passed: SemanticQualificationCase::ALL.to_vec(),
        failure_class: None,
        failure_detail: None,
        qualified_at_unix_seconds: 1,
    }
}

fn challenged_case(
    fixture: &ReviewFixture,
    action: ResponseAction,
) -> (
    auditor_review::ReviewerCaseReview,
    ChallengeOutcome,
    FinalizedCase,
) {
    let review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    let target_id = review.recommendations[0].recommendation_id.clone();
    let objection_id = StableId::from_identity(
        "challenge-objection",
        1,
        &json!({
            "case_id": fixture.case.case_id,
            "reason": ObjectionReason::OverclaimedBenefit,
            "target_id": target_id,
            "target_type": ChallengeTargetType::Recommendation,
        }),
    )
    .unwrap();
    let challenge = ChallengeRecord::admit(
        ChallengeRecordPayload {
            status: ChallengeStatus::ObjectionsRaised,
            objections: vec![ChallengeObjection {
                objection_id: objection_id.clone(),
                target_type: ChallengeTargetType::Recommendation,
                target_id,
                reason: ObjectionReason::OverclaimedBenefit,
                evidence_ids: vec![fixture.definitions[0].probe_id.to_string()],
                materiality: Materiality::High,
                required_resolution: "Ground the benefit or withdraw the recommendation."
                    .to_string(),
            }],
            common_mode_concerns: vec![
                "Reviewer and challenger must not share unsupported assumptions.".to_string(),
            ],
        },
        ChallengeAdmissionContext {
            challenger_run_id: &challenger_run_id(),
            route_target: &challenger_target(),
            case: &fixture.case,
            packet: &fixture.packet,
            reviewer_review: &review,
        },
    )
    .unwrap();
    let response = ChallengeResponse::admit(
        ChallengeResponsePayload {
            responses: vec![ObjectionResponse {
                objection_id,
                action,
                rationale: "The response is bounded to the named benefit objection.".to_string(),
                evidence_probe_ids: vec![fixture.definitions[0].probe_id.clone()],
                replacement_recommendation: None,
            }],
        },
        ChallengeResponseAdmission::for_review(
            reviewer_run_id(),
            &fixture.case,
            &challenge,
            &review,
            &fixture
                .definitions
                .iter()
                .map(|definition| definition.probe_id.clone())
                .collect::<BTreeSet<_>>(),
        ),
    )
    .unwrap();
    let outcome = ChallengeOutcome {
        challenge,
        response: Some(response),
    };
    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: Some(&outcome.challenge),
        response: outcome.response.as_ref(),
        allowed_probe_ids: &fixture
            .definitions
            .iter()
            .map(|definition| definition.probe_id.clone())
            .collect(),
    })
    .unwrap();
    (review, outcome, finalized)
}

fn artifact(fixture: &ReviewFixture) -> EngineeringReviewV2 {
    let (review, challenge, finalized) =
        challenged_case(fixture, ResponseAction::MaintainWithEvidence);
    let agenda = AgendaPlanner
        .plan_with_foundation_policy(&fixture.input)
        .unwrap();
    let mut unused_failed_qualification =
        qualification(fixture, review_route_target(), ReviewRole::ReviewerA);
    unused_failed_qualification.transport_valid = false;
    unused_failed_qualification.semantic_output_valid = false;
    unused_failed_qualification.native_protocol_valid = false;
    unused_failed_qualification.semantic_cases_passed.clear();
    unused_failed_qualification.failure_class = Some(ProviderFailureClass::InvalidOutput);
    unused_failed_qualification.failure_detail =
        Some("fixture qualification intentionally failed".to_string());
    EngineeringReviewV2::build(
        &fixture.input,
        EngineeringReviewV2Draft {
            agenda,
            cases: vec![(*fixture.case).clone()],
            evidence_packets: vec![fixture.packet.clone()],
            evidence_packet_history: Vec::new(),
            probe_definitions: fixture.definitions.clone(),
            probe_results: fixture.results.clone(),
            reviewer_runs: vec![review.clone()],
            ensemble_runs: Vec::new(),
            ensemble_failures: Vec::new(),
            ensemble_evidence_packets: Vec::new(),
            ensemble_challenges: Vec::new(),
            claims: review.claims.clone(),
            recommendations: review.recommendations.clone(),
            challenges: vec![challenge],
            execution_truth: vec![finalized],
            qualification_records: vec![
                qualification(fixture, review_route_target(), ReviewRole::SingleReviewer),
                qualification(fixture, challenger_target(), ReviewRole::Challenger),
                unused_failed_qualification,
            ],
            provider_records: Vec::new(),
            egress_manifests: Vec::new(),
            claim_slot_dispositions: Vec::new(),
            recommendation_family_dispositions: Vec::new(),
            challenge_target_dispositions: Vec::new(),
            semantic_revisions: Vec::new(),
            projection_audits: Vec::new(),
        },
    )
    .unwrap()
}

#[test]
fn challenge_catches_benefit_and_withdrawal_cannot_survive_finalization() {
    let fixture = ReviewFixture::new();
    let (_, outcome, finalized) = challenged_case(&fixture, ResponseAction::Withdraw);
    assert_eq!(outcome.challenge.objections.len(), 1);
    assert!(finalized.accepted_recommendations.is_empty());
    assert_eq!(
        finalized.recommendation_dispositions[0].disposition,
        FinalRecommendationDisposition::Withdrawn
    );
}

#[test]
fn finalization_rejects_lifecycle_reownership_after_a_negative_conclusion() {
    let fixture = ReviewFixture::new();
    let (mut review, outcome, _) = challenged_case(&fixture, ResponseAction::MaintainWithEvidence);
    review.case_conclusion = auditor_review::CaseConclusion::RepetitionJustified;
    review.case_position = auditor_review::CasePosition::NotSubstantiated;
    review.proposed_closure = CaseClosure::terminal(
        fixture.case.case_id.clone(),
        CaseClosureReason::NotSubstantiated,
        Vec::new(),
    )
    .unwrap();

    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: Some(&outcome.challenge),
        response: outcome.response.as_ref(),
        allowed_probe_ids: &fixture
            .definitions
            .iter()
            .map(|definition| definition.probe_id.clone())
            .collect(),
    })
    .unwrap();

    assert!(finalized.accepted_recommendations.is_empty());
    assert_eq!(
        finalized.recommendation_dispositions[0].disposition,
        FinalRecommendationDisposition::Rejected
    );
}

#[test]
fn challenge_tool_is_exact_and_request_turn_requires_recompiled_packet() {
    let objection = auditor_review::CaseTool::RecordCaseObjection.parameters();
    assert_eq!(objection["additionalProperties"], false);
    assert!(objection["properties"].get("case_id").is_none());
    assert!(objection["properties"].get("objections").is_none());

    let fixture = ReviewFixture::new();
    let review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    let mut session = ChallengeSession::new(&fixture.case, &fixture.packet).unwrap();
    let request = CaseEvidenceRequest {
        case_id: fixture.case.case_id.clone(),
        probe_id: fixture.definitions[0].probe_id.clone(),
        request_kind: EvidenceLookupRequestKind::FindingSource,
        target_id: format!("finding:{}", fixture.case.finding_ids[0]),
        decision_relevance: "Challenge the lifecycle source boundary.".to_string(),
    };
    let run_id = challenger_run_id();
    let target = challenger_target();
    let context = || ChallengeAdmissionContext {
        challenger_run_id: &run_id,
        route_target: &target,
        case: &fixture.case,
        packet: &fixture.packet,
        reviewer_review: &review,
    };
    assert!(matches!(
        session
            .consume_turn(
                CaseProviderTurn {
                    evidence_requests: vec![request],
                    case_review: None,
                    challenge: None,
                },
                context(),
            )
            .unwrap(),
        ChallengeTurn::EvidenceRequested(_)
    ));
    assert_eq!(
        session.consume_turn(
            CaseProviderTurn {
                evidence_requests: Vec::new(),
                case_review: None,
                challenge: Some(
                    serde_json::to_value(ChallengeRecordPayload {
                        status: ChallengeStatus::Cleared,
                        objections: Vec::new(),
                        common_mode_concerns: Vec::new(),
                    })
                    .unwrap(),
                ),
            },
            context(),
        ),
        Err(auditor_review::ChallengeError::EvidenceNotRecompiled)
    );
}

#[test]
fn missing_or_blocked_mandatory_challenge_keeps_lifecycle_action_blocked() {
    let fixture = ReviewFixture::new();
    let review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    let allowed = fixture
        .definitions
        .iter()
        .map(|definition| definition.probe_id.clone())
        .collect::<BTreeSet<_>>();
    let missing = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: None,
        response: None,
        allowed_probe_ids: &allowed,
    })
    .unwrap();
    assert!(missing.accepted_recommendations.is_empty());
    assert_eq!(
        missing.recommendation_dispositions[0].disposition,
        FinalRecommendationDisposition::Blocked
    );

    let blocked = ChallengeRecord::admit(
        ChallengeRecordPayload {
            status: ChallengeStatus::Blocked,
            objections: Vec::new(),
            common_mode_concerns: vec![
                "Shared unknown evidence prevents safe lifecycle validation.".to_string(),
            ],
        },
        ChallengeAdmissionContext {
            challenger_run_id: &challenger_run_id(),
            route_target: &challenger_target(),
            case: &fixture.case,
            packet: &fixture.packet,
            reviewer_review: &review,
        },
    )
    .unwrap();
    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: Some(&blocked),
        response: None,
        allowed_probe_ids: &allowed,
    })
    .unwrap();
    assert_eq!(
        finalized.closure.final_state,
        auditor_review::FinalCaseState::BlockedByMissingEvidence
    );
}

#[test]
fn bound_challenge_response_rejects_recommendation_narrowing_for_claim_objections() {
    let fixture = ReviewFixture::new();
    let review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    let target_id = review.claims[0].claim_id.clone();
    let objection_id = StableId::from_identity(
        "challenge-objection",
        1,
        &json!({
            "case_id": fixture.case.case_id,
            "reason": ObjectionReason::UnsupportedAssumption,
            "target_id": target_id,
            "target_type": ChallengeTargetType::Claim,
        }),
    )
    .unwrap();
    let challenge = ChallengeRecord::admit(
        ChallengeRecordPayload {
            status: ChallengeStatus::ObjectionsRaised,
            objections: vec![ChallengeObjection {
                objection_id: objection_id.clone(),
                target_type: ChallengeTargetType::Claim,
                target_id,
                reason: ObjectionReason::UnsupportedAssumption,
                evidence_ids: vec![fixture.definitions[0].probe_id.to_string()],
                materiality: Materiality::High,
                required_resolution: "Concede, substantiate, withdraw, or mark unresolved."
                    .to_string(),
            }],
            common_mode_concerns: Vec::new(),
        },
        ChallengeAdmissionContext {
            challenger_run_id: &challenger_run_id(),
            route_target: &challenger_target(),
            case: &fixture.case,
            packet: &fixture.packet,
            reviewer_review: &review,
        },
    )
    .unwrap();
    let allowed = fixture
        .definitions
        .iter()
        .map(|definition| definition.probe_id.clone())
        .collect::<BTreeSet<_>>();
    let schema = bound_challenge_response_payload_schema(&challenge, &review, &allowed);
    let encoded = serde_json::to_string(&schema).unwrap();
    assert!(!encoded.contains("\"narrow\""));
    let error = ChallengeResponse::admit(
        ChallengeResponsePayload {
            responses: vec![ObjectionResponse {
                objection_id,
                action: ResponseAction::Narrow,
                rationale: "Narrow the claim to the visible source span.".to_string(),
                evidence_probe_ids: vec![fixture.definitions[0].probe_id.clone()],
                replacement_recommendation: None,
            }],
        },
        ChallengeResponseAdmission::for_review(
            reviewer_run_id(),
            &fixture.case,
            &challenge,
            &review,
            &allowed,
        ),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ChallengeError::InvalidResponse(message)
            if message.contains("narrow is valid only for a recommendation-target objection")
                && message.contains("narrow requires replacement_recommendation")
    ));
}

#[test]
fn unrelated_blocked_claim_does_not_override_an_accepted_action() {
    let fixture = ReviewFixture::new();
    let (mut review, outcome, _) = challenged_case(&fixture, ResponseAction::MaintainWithEvidence);
    let blocked = auditor_review::EngineeringClaim::from_draft(
        fixture.case.case_id.clone(),
        ClaimDraft {
            slot: fixture.case.claim_slots[3],
            kind: auditor_review::EngineeringClaimKind::Actionability,
            proposition: "An optional measurement remains unresolved.".to_string(),
            status: ClaimStatus::Blocked,
            supporting_probe_ids: vec![fixture.definitions[0].probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: Vec::new(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Low,
        },
    )
    .unwrap();
    review.claims.push(blocked);
    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: Some(&outcome.challenge),
        response: outcome.response.as_ref(),
        allowed_probe_ids: &fixture
            .definitions
            .iter()
            .map(|definition| definition.probe_id.clone())
            .collect(),
    })
    .unwrap();
    assert_eq!(finalized.accepted_recommendations.len(), 1);
    assert_eq!(
        finalized.closure.reason,
        CaseClosureReason::ResolvedWithUncertainty
    );
}

#[test]
fn optional_blocked_claim_does_not_override_a_settled_negative_conclusion() {
    let fixture = ReviewFixture::new();
    let mut review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    review.case_conclusion = auditor_review::CaseConclusion::ImpactImmaterial;
    review.case_position = auditor_review::CasePosition::NotSubstantiated;
    review.proposed_closure = CaseClosure::terminal(
        fixture.case.case_id.clone(),
        CaseClosureReason::NotSubstantiated,
        vec!["Optional material-cost evidence remains unavailable.".to_string()],
    )
    .unwrap();
    review.recommendations.clear();
    review.preferred_recommendation_id = None;
    let blocked = auditor_review::EngineeringClaim::from_draft(
        fixture.case.case_id.clone(),
        ClaimDraft {
            slot: fixture.case.claim_slots[3],
            kind: auditor_review::EngineeringClaimKind::ExpectedBenefit,
            proposition: "Optional material-cost evidence remains unavailable.".to_string(),
            status: ClaimStatus::Blocked,
            supporting_probe_ids: vec![fixture.definitions[0].probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: Vec::new(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Low,
        },
    )
    .unwrap();
    review.claims.push(blocked);

    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: None,
        response: None,
        allowed_probe_ids: &fixture
            .definitions
            .iter()
            .map(|definition| definition.probe_id.clone())
            .collect(),
    })
    .unwrap();

    assert!(finalized.accepted_recommendations.is_empty());
    assert!(finalized
        .claim_dispositions
        .iter()
        .any(|record| record.disposition == auditor_review::FinalClaimDisposition::Blocked));
    assert_eq!(
        finalized.closure.reason,
        CaseClosureReason::NotSubstantiated
    );
}

#[test]
fn conceded_premise_objection_invalidates_downstream_recommendation() {
    let fixture = ReviewFixture::new();
    let review = fixture
        .admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ))
        .unwrap();
    let premise_id = review
        .claims
        .iter()
        .find(|claim| claim.kind == auditor_review::EngineeringClaimKind::RecommendationPremise)
        .unwrap()
        .claim_id
        .clone();
    let objection_id = StableId::from_identity(
        "challenge-objection",
        1,
        &json!({
            "case_id": fixture.case.case_id,
            "reason": ObjectionReason::UnsupportedAssumption,
            "target_id": premise_id,
            "target_type": ChallengeTargetType::Claim,
        }),
    )
    .unwrap();
    let challenge = ChallengeRecord::admit(
        ChallengeRecordPayload {
            status: ChallengeStatus::ObjectionsRaised,
            objections: vec![ChallengeObjection {
                objection_id: objection_id.clone(),
                target_type: ChallengeTargetType::Claim,
                target_id: premise_id,
                reason: ObjectionReason::UnsupportedAssumption,
                evidence_ids: vec![fixture.packet.items[0].id.clone()],
                materiality: Materiality::High,
                required_resolution: "Concede or substantiate the recommendation premise."
                    .to_string(),
            }],
            common_mode_concerns: Vec::new(),
        },
        ChallengeAdmissionContext {
            challenger_run_id: &challenger_run_id(),
            route_target: &challenger_target(),
            case: &fixture.case,
            packet: &fixture.packet,
            reviewer_review: &review,
        },
    )
    .unwrap();
    let allowed = fixture
        .definitions
        .iter()
        .map(|definition| definition.probe_id.clone())
        .collect::<BTreeSet<_>>();
    let response = ChallengeResponse::admit(
        ChallengeResponsePayload {
            responses: vec![ObjectionResponse {
                objection_id,
                action: ResponseAction::Concede,
                rationale: "The cited evidence does not substantiate the premise.".to_string(),
                evidence_probe_ids: Vec::new(),
                replacement_recommendation: None,
            }],
        },
        ChallengeResponseAdmission::for_review(
            reviewer_run_id(),
            &fixture.case,
            &challenge,
            &review,
            &allowed,
        ),
    )
    .unwrap();
    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: Some(&challenge),
        response: Some(&response),
        allowed_probe_ids: &allowed,
    })
    .unwrap();
    assert!(finalized.accepted_recommendations.is_empty());
    assert_eq!(
        finalized.recommendation_dispositions[0].disposition,
        FinalRecommendationDisposition::Rejected
    );
}

#[test]
fn engineering_review_v2_round_trips_and_verifies_without_transport() {
    let fixture = ReviewFixture::new();
    let artifact = artifact(&fixture);
    let text = serde_json::to_string_pretty(&artifact).unwrap();
    let decoded: EngineeringReviewV2 = serde_json::from_str(&text).unwrap();
    let verification =
        auditor_review::verify_engineering_review_v2(&decoded, &fixture.input).unwrap();
    assert!(verification.verified);
    assert_eq!(decoded.schema_version, 2);
    assert!(decoded.ensemble_comparisons.is_empty());
    assert_eq!(decoded.coverage.terminal_case_count, 1);
}

#[test]
fn blocked_review_requires_a_measurement_recommendation() {
    let fixture = ReviewFixture::new_unknown();
    assert!(matches!(
        fixture.admit(auditor_review::ReviewerCaseReviewPayload {
            case_conclusion: auditor_review::CaseConclusion::InsufficientEvidence,
            case_position: auditor_review::CasePosition::EvidenceBlocked,
            model_resolved_probes: Vec::new(),
            claims: Vec::new(),
            recommendations: Vec::new(),
            preferred_recommendation_id: None,
            unresolved_gaps: vec!["Shared evidence remains unknown.".to_string()],
            proposed_closure: CaseClosureReason::BlockedByMissingEvidence,
        }),
        Err(SingleReviewerError::InvalidRecommendations(detail))
            if detail.contains("requires at least one MeasurementBeforeChange recommendation")
    ));
}

#[test]
fn measurement_recommendation_accepts_the_blocked_gap_it_resolves() {
    let fixture = ReviewFixture::new_unknown();
    let (probe_id, slot) = fixture
        .results
        .iter()
        .filter(|result| result.resolver.kind == ResolverKind::UnknownFallback)
        .find_map(|result| {
            fixture
                .definitions
                .iter()
                .find(|definition| definition.probe_id == result.probe_id)
                .and_then(|definition| definition.blocked_claim_slots.first().copied())
                .map(|slot| (result.probe_id.clone(), slot))
        })
        .expect("missing-evidence fixture has a claim-blocking probe");
    let premise = auditor_review::EngineeringClaim::from_draft(
        fixture.case.case_id.clone(),
        ClaimDraft {
            slot,
            kind: auditor_review::EngineeringClaimKind::RecommendationPremise,
            proposition:
                "The unresolved lifecycle boundary must be measured before ownership changes."
                    .to_string(),
            status: ClaimStatus::Blocked,
            supporting_probe_ids: vec![probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: Vec::new(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap();
    let recommendation = ContextualRecommendation::from_draft(
        fixture.case.case_id.clone(),
        RecommendationDraft {
            family: RecommendationFamily::MeasurementBeforeChange,
            summary: "Measure the unresolved lifecycle boundary before changing ownership."
                .to_string(),
            scope_ids: vec![fixture.case.finding_ids[0].clone()],
            premise_claim_ids: vec![premise.claim_id.clone()],
            affected_finding_ids: fixture.case.finding_ids.clone(),
            detector_advice_relationship: DetectorAdviceRelationship::Narrows,
            preconditions: vec!["The unresolved behavior can be inspected or traced.".to_string()],
            expected_benefits: vec![ExpectedBenefit {
                description: "Resolves whether the ownership change is safe.".to_string(),
                evidence_mode: BenefitEvidenceMode::Unknown,
                numeric_value: None,
                unit: None,
                extrapolation_model: None,
                evidence_probe_ids: vec![probe_id.clone()],
            }],
            risks: vec!["Measurement may confirm the current placement is required.".to_string()],
            costs: vec!["Requires focused inspection or instrumentation.".to_string()],
            alternatives: Vec::new(),
            no_alternative_reason: None,
            verification_steps: vec![VerificationStep {
                method: "Inspect or trace the unresolved behavior.".to_string(),
                expected_observation: "The blocked probe resolves to a definitive boundary."
                    .to_string(),
                evidence_probe_ids: vec![probe_id],
            }],
            materiality: Materiality::Low,
            materiality_basis: "No bound runtime measurement establishes material cost."
                .to_string(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap();
    let review = fixture
        .admit(auditor_review::ReviewerCaseReviewPayload {
            case_conclusion: auditor_review::CaseConclusion::InsufficientEvidence,
            case_position: auditor_review::CasePosition::EvidenceBlocked,
            model_resolved_probes: Vec::new(),
            claims: vec![premise],
            recommendations: vec![recommendation.clone()],
            preferred_recommendation_id: Some(recommendation.recommendation_id.clone()),
            unresolved_gaps: vec!["The lifecycle boundary remains unresolved.".to_string()],
            proposed_closure: CaseClosureReason::BlockedByMissingEvidence,
        })
        .unwrap();
    let finalized = FinalizedCase::finalize(CaseFinalizationContext {
        case: &fixture.case,
        review: &review,
        challenge: None,
        response: None,
        allowed_probe_ids: &fixture
            .definitions
            .iter()
            .map(|definition| definition.probe_id.clone())
            .collect(),
    })
    .unwrap();

    assert_eq!(
        finalized.recommendation_dispositions[0].disposition,
        FinalRecommendationDisposition::Accepted
    );
    assert_eq!(finalized.accepted_recommendations, vec![recommendation]);
    assert_eq!(
        finalized.closure.final_state,
        auditor_review::FinalCaseState::BlockedByMissingEvidence
    );
}

#[test]
fn offline_verifier_rejects_mutation_over_budget_and_invalid_challenge() {
    let fixture = ReviewFixture::new();

    let mut mutated = artifact(&fixture);
    mutated.finding_dispositions[0].finding_id = "mutated-finding".to_string();
    mutated.review_id = mutated.expected_id().unwrap();
    assert!(auditor_review::verify_engineering_review_v2(&mutated, &fixture.input).is_err());

    let mut over_budget = artifact(&fixture);
    over_budget.evidence_packets[0].byte_count =
        over_budget.evidence_packets[0].budget.hard_packet_bytes + 1;
    over_budget.review_id = over_budget.expected_id().unwrap();
    assert!(auditor_review::verify_engineering_review_v2(&over_budget, &fixture.input).is_err());

    let mut invalid_challenge = artifact(&fixture);
    invalid_challenge.challenges[0].challenge.objections[0].target_id =
        StableId::from_identity("contextual-recommendation", 1, &json!({"absent": true})).unwrap();
    invalid_challenge.review_id = invalid_challenge.expected_id().unwrap();
    assert!(
        auditor_review::verify_engineering_review_v2(&invalid_challenge, &fixture.input).is_err()
    );

    let mut invalid_qualification = artifact(&fixture);
    invalid_qualification.qualification_records[0].qualification_id =
        StableId::from_identity("review-qualification", 2, &json!({"invalid": true})).unwrap();
    invalid_qualification.review_id = invalid_qualification.expected_id().unwrap();
    assert!(matches!(
        auditor_review::verify_engineering_review_v2(&invalid_qualification, &fixture.input),
        Err(auditor_review::ReviewVerificationError::Provider(_))
    ));

    let mut cyclic = artifact(&fixture);
    let dependency = cyclic
        .claims
        .iter()
        .find(|claim| claim.kind == auditor_review::EngineeringClaimKind::RecommendationPremise)
        .unwrap()
        .claim_id
        .clone();
    cyclic
        .claims
        .iter_mut()
        .find(|claim| claim.kind == auditor_review::EngineeringClaimKind::RootCause)
        .unwrap()
        .content
        .dependency_claim_ids = vec![dependency.clone()];
    cyclic.reviewer_runs[0]
        .claims
        .iter_mut()
        .find(|claim| claim.kind == auditor_review::EngineeringClaimKind::RootCause)
        .unwrap()
        .content
        .dependency_claim_ids = vec![dependency];
    cyclic.review_id = cyclic.expected_id().unwrap();
    assert!(auditor_review::verify_engineering_review_v2(&cyclic, &fixture.input).is_err());
}

#[test]
fn v2_publication_is_report_last_and_stage_failure_leaves_no_partials() {
    let fixture = ReviewFixture::new();
    let artifact = artifact(&fixture);
    let input_report_json = std::fs::read_to_string(&fixture.fixture.report).unwrap();
    let reviewed = fixture.fixture.report.with_file_name("reviewed-v2.json");
    let output =
        auditor_review::publish_engineering_review_v2(auditor_review::ReviewV2PublishRequest {
            input_report_path: &fixture.fixture.report,
            reviewed_report_path: &reviewed,
            input_report_json: &input_report_json,
            input: &fixture.input,
            artifact: &artifact,
        })
        .unwrap();
    assert!(output.artifact_path.is_file());
    assert!(output.markdown_path.is_file());
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&reviewed).unwrap()).unwrap();
    assert_eq!(report["engineering_review"]["schema_version"], 2);
    assert_eq!(report["engineering_review"]["case_count"], 1);
    auditor_report::validate_report_json(&serde_json::to_string(&report).unwrap()).unwrap();

    let failing = fixture.fixture.report.with_file_name("blocked-output");
    std::fs::create_dir_all(&failing).unwrap();
    let before = std::fs::read_to_string(&fixture.fixture.report).unwrap();
    assert!(auditor_review::publish_engineering_review_v2(
        auditor_review::ReviewV2PublishRequest {
            input_report_path: &fixture.fixture.report,
            reviewed_report_path: &failing,
            input_report_json: &input_report_json,
            input: &fixture.input,
            artifact: &artifact,
        },
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(&fixture.fixture.report).unwrap(),
        before
    );
    assert!(!fixture
        .fixture
        .report
        .with_file_name("blocked-output.review-v2.md")
        .exists());
    assert!(!std::fs::read_dir(fixture.fixture.report.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("blocked-output.engineering-review-v2.")));
}

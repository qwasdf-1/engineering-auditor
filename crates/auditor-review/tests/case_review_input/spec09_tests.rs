use super::*;

pub(super) fn reviewer_run_id() -> auditor_review::StableId {
    auditor_review::StableId::from_identity(
        "reviewer-run",
        1,
        &json!({"fixture": "spec-09-single-reviewer"}),
    )
    .unwrap()
}

pub(super) fn review_route_target() -> RouteTarget {
    RouteTarget {
        provider_id: "fixture-provider".to_string(),
        model: "fixture-model".to_string(),
    }
}

pub(super) fn review_payload(
    case: &auditor_review::EngineeringCase,
    definitions: &[ProbeDefinition],
    premise_status: ClaimStatus,
    invalid_numeric_benefit: bool,
) -> ReviewerCaseReviewPayload {
    let probe_id = definitions[0].probe_id.clone();
    let root = auditor_review::EngineeringClaim::from_draft(
        case.case_id.clone(),
        ClaimDraft {
            slot: case.claim_slots[0],
            kind: auditor_review::EngineeringClaimKind::RootCause,
            proposition: "Repeated work is rooted below its stable lifetime.".to_string(),
            status: ClaimStatus::Substantiated,
            supporting_probe_ids: vec![probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: Vec::new(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::High,
        },
    )
    .unwrap();
    let actionability = auditor_review::EngineeringClaim::from_draft(
        case.case_id.clone(),
        ClaimDraft {
            slot: case.claim_slots[1],
            kind: auditor_review::EngineeringClaimKind::Actionability,
            proposition: "The wider owner can hold the work without changing semantics."
                .to_string(),
            status: ClaimStatus::Substantiated,
            supporting_probe_ids: vec![probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: vec![root.claim_id.clone()],
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap();
    let premise = auditor_review::EngineeringClaim::from_draft(
        case.case_id.clone(),
        ClaimDraft {
            slot: case.claim_slots[2],
            kind: auditor_review::EngineeringClaimKind::RecommendationPremise,
            proposition: "Reownership is feasible within the selected case boundary.".to_string(),
            status: premise_status,
            supporting_probe_ids: vec![probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: vec![actionability.claim_id.clone()],
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap();
    let recommendation = ContextualRecommendation::from_draft(
        case.case_id.clone(),
        RecommendationDraft {
            family: RecommendationFamily::LifecycleReownership,
            summary: "Move the stable work to the wider lifecycle owner.".to_string(),
            scope_ids: vec![case.subject_ids[0].clone()],
            premise_claim_ids: vec![premise.claim_id.clone()],
            affected_finding_ids: case.finding_ids.clone(),
            detector_advice_relationship: DetectorAdviceRelationship::Consolidates,
            preconditions: vec!["Preserve the current mutation boundary.".to_string()],
            expected_benefits: vec![ExpectedBenefit {
                description: "Avoid repeated stable work.".to_string(),
                evidence_mode: if invalid_numeric_benefit {
                    BenefitEvidenceMode::QualitativeOnly
                } else {
                    BenefitEvidenceMode::StructuralInference
                },
                numeric_value: invalid_numeric_benefit.then(|| "42".to_string()),
                unit: invalid_numeric_benefit.then(|| "milliseconds".to_string()),
                extrapolation_model: None,
                evidence_probe_ids: vec![probe_id.clone()],
            }],
            risks: vec!["A wider owner may retain state longer.".to_string()],
            costs: vec!["Update construction and invalidation wiring.".to_string()],
            alternatives: vec![RecommendationAlternative {
                summary: "Retain local ownership and avoid the repeated path explicitly."
                    .to_string(),
                tradeoffs: vec!["Keeps ownership local but preserves more wiring.".to_string()],
            }],
            no_alternative_reason: None,
            verification_steps: vec![VerificationStep {
                method: "Run focused tests and compare invocation counts.".to_string(),
                expected_observation: "Semantics match and repeated work is absent.".to_string(),
                evidence_probe_ids: vec![probe_id],
            }],
            materiality: Materiality::Medium,
            materiality_basis: "The finding is repeated on a selected production path.".to_string(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap();
    ReviewerCaseReviewPayload {
        case_conclusion: auditor_review::CaseConclusion::LifecycleMismatchSubstantiated,
        case_position: CasePosition::Substantiated,
        model_resolved_probes: Vec::new(),
        claims: vec![root, actionability, premise],
        preferred_recommendation_id: Some(recommendation.recommendation_id.clone()),
        recommendations: vec![recommendation],
        unresolved_gaps: vec!["Runtime magnitude was not measured.".to_string()],
        proposed_closure: CaseClosureReason::ResolvedWithUncertainty,
    }
}

struct LifecycleFixtureProbeResolver {
    mutation: bool,
}

impl ProbeResolver for LifecycleFixtureProbeResolver {
    fn id(&self) -> &'static str {
        "positive-lifecycle-fixture"
    }

    fn version(&self) -> u32 {
        1
    }

    fn supports(&self, probe: &ProbeDefinition) -> bool {
        matches!(
            probe.slot.as_str(),
            "repetition"
                | "input_equivalence"
                | "intervening_mutation"
                | "consumer_distinction"
                | "wider_owner"
                | "invalidation_feasibility"
                | "retention_risk"
                | "material_impact"
        )
    }

    fn resolve(
        &self,
        _case: &auditor_review::EngineeringCase,
        probe: &ProbeDefinition,
        _packet: &auditor_review::EvidencePacket,
    ) -> Result<ProbeResolution, ProbeResolutionError> {
        let answer = match probe.slot.as_str() {
            "repetition" | "input_equivalence" | "wider_owner" | "invalidation_feasibility" => {
                ProbeAnswer::Yes
            }
            "intervening_mutation" | "consumer_distinction" | "retention_risk" => ProbeAnswer::No,
            "material_impact" => ProbeAnswer::Unknown,
            slot => {
                return Err(ProbeResolutionError::UnsupportedProbe(format!(
                    "unexpected fixture probe slot {slot}"
                )))
            }
        };
        let answer = match (self.mutation, probe.slot.as_str()) {
            (true, "input_equivalence") => ProbeAnswer::No,
            (true, "intervening_mutation" | "consumer_distinction") => ProbeAnswer::Yes,
            _ => answer,
        };
        Ok(ProbeResolution::new(
            answer,
            Vec::new(),
            format!("positive lifecycle fixture answer for {}", probe.slot),
            Vec::new(),
        ))
    }
}

pub(super) struct ReviewFixture {
    pub(super) fixture: Fixture,
    pub(super) input: auditor_review::ReviewInput,
    pub(super) case: Box<auditor_review::EngineeringCase>,
    pub(super) packet: auditor_review::EvidencePacket,
    pub(super) definitions: Vec<ProbeDefinition>,
    pub(super) results: Vec<auditor_review::ProbeResult>,
}

impl ReviewFixture {
    pub(super) fn new() -> Self {
        Self::with_fixture_answers(Some(false))
    }

    pub(super) fn new_mutation() -> Self {
        Self::with_fixture_answers(Some(true))
    }

    pub(super) fn new_unknown() -> Self {
        Self::with_fixture_answers(None)
    }

    fn with_fixture_answers(mutation: Option<bool>) -> Self {
        let fixture = Fixture::new();
        let (templates, prompts, routes) = contracts();
        let input = verify(
            &fixture,
            Some(&templates),
            Some(&prompts),
            Some(&routes),
            &[],
            &[],
            None,
        )
        .unwrap();
        let case = instantiated_lifecycle_case(&input);
        let packet = EvidenceCompiler.compile(&case, &input).unwrap();
        let definitions = ProbePlanner
            .plan(
                &case,
                templates.entries[0].template.as_ref().unwrap(),
                &packet,
                &[],
            )
            .unwrap();
        let resolver = if let Some(mutation) = mutation {
            ProbeResolverRegistry::new(vec![ResolverRegistration::new(
                ResolverKind::ExistingDeterministicFact,
                Box::new(LifecycleFixtureProbeResolver { mutation }),
            )])
            .unwrap()
        } else {
            ProbeResolverRegistry::built_in().unwrap()
        };
        let results = definitions
            .iter()
            .map(|definition| resolver.resolve(&case, definition, &packet).unwrap())
            .collect();
        Self {
            fixture,
            input,
            case,
            packet,
            definitions,
            results,
        }
    }

    pub(super) fn admit(
        &self,
        payload: ReviewerCaseReviewPayload,
    ) -> Result<ReviewerCaseReview, SingleReviewerError> {
        let run_id = reviewer_run_id();
        let target = review_route_target();
        ReviewerCaseReview::admit(
            payload,
            ReviewerAdmissionContext {
                reviewer_run_id: &run_id,
                route_target: &target,
                case: &self.case,
                packet: &self.packet,
                probe_definitions: &self.definitions,
                deterministic_probe_results: &self.results,
            },
        )
    }
}

#[test]
fn admits_useful_results_and_rejects_missing_action_for_actionable_conclusion() {
    let fixture = ReviewFixture::new();
    let payload = || {
        review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        )
    };
    let first = fixture.admit(payload()).unwrap();
    let second = fixture.admit(payload()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.case_id, fixture.case.case_id);
    assert_eq!(first.route_target, review_route_target());

    let mut direct_premise = payload();
    let mut recommendation = direct_premise.recommendations[0].to_draft();
    recommendation.premise_claim_ids = vec![direct_premise.claims[0].claim_id.clone()];
    let recommendation =
        ContextualRecommendation::from_draft(fixture.case.case_id.clone(), recommendation).unwrap();
    direct_premise.preferred_recommendation_id = Some(recommendation.recommendation_id.clone());
    direct_premise.recommendations = vec![recommendation];
    fixture
        .admit(direct_premise)
        .expect("a non-invalidated root-cause claim may directly support a recommendation");

    let mut no_recommendation_payload = payload();
    no_recommendation_payload.recommendations.clear();
    no_recommendation_payload.preferred_recommendation_id = None;
    assert!(matches!(
        fixture.admit(no_recommendation_payload),
        Err(SingleReviewerError::InvalidRecommendations(detail))
            if detail.contains("requires at least one LifecycleReownership recommendation")
    ));
}

#[test]
fn rejects_a_compatible_position_when_the_conclusion_disagrees_with_admitted_probes() {
    let fixture = ReviewFixture::new();
    let mut payload = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    payload.case_conclusion = auditor_review::CaseConclusion::RepetitionJustified;
    payload.case_position = CasePosition::NotSubstantiated;
    payload.proposed_closure = CaseClosureReason::NotSubstantiated;
    let mut recommendation = payload.recommendations[0].to_draft();
    recommendation.family = RecommendationFamily::ExplicitRetentionAvoidance;
    let recommendation =
        ContextualRecommendation::from_draft(fixture.case.case_id.clone(), recommendation).unwrap();
    payload.preferred_recommendation_id = Some(recommendation.recommendation_id.clone());
    payload.recommendations = vec![recommendation];

    assert!(matches!(
        fixture.admit(payload),
        Err(SingleReviewerError::InvalidCaseConclusion(_))
    ));
}

#[test]
fn rejects_lifecycle_reownership_when_mutation_justifies_repetition() {
    let fixture = ReviewFixture::new_mutation();
    let mut payload = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    payload.case_conclusion = auditor_review::CaseConclusion::RepetitionJustified;
    payload.case_position = CasePosition::NotSubstantiated;
    payload.proposed_closure = CaseClosureReason::NotSubstantiated;

    assert!(matches!(
        fixture.admit(payload),
        Err(SingleReviewerError::InvalidRecommendations(detail))
            if detail.contains("cannot admit recommendation family")
    ));
}

#[test]
fn rejects_invalid_claim_recommendation_and_probe_authority() {
    let fixture = ReviewFixture::new();
    let mut blocked = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    let unknown_probe_id = fixture
        .results
        .iter()
        .find(|result| result.answer == ProbeAnswer::Unknown)
        .unwrap()
        .probe_id
        .clone();
    let blocked_premise = auditor_review::EngineeringClaim::from_draft(
        fixture.case.case_id.clone(),
        ClaimDraft {
            slot: fixture.case.claim_slots[2],
            kind: auditor_review::EngineeringClaimKind::RecommendationPremise,
            proposition: "Reownership feasibility is blocked by missing evidence.".to_string(),
            status: ClaimStatus::Blocked,
            supporting_probe_ids: vec![unknown_probe_id],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: vec![blocked.claims[1].claim_id.clone()],
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Low,
        },
    )
    .unwrap();
    blocked.claims[2] = blocked_premise.clone();
    let mut blocked_recommendation = blocked.recommendations[0].to_draft();
    blocked_recommendation.premise_claim_ids = vec![blocked_premise.claim_id];
    let replacement =
        ContextualRecommendation::from_draft(fixture.case.case_id.clone(), blocked_recommendation)
            .unwrap();
    blocked.preferred_recommendation_id = Some(replacement.recommendation_id.clone());
    blocked.recommendations = vec![replacement];
    assert!(matches!(
        fixture.admit(blocked),
        Err(SingleReviewerError::InvalidRecommendations(_))
    ));
    let mut missing_alternative = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    let mut recommendation_draft = missing_alternative.recommendations[0].to_draft();
    recommendation_draft.alternatives.clear();
    recommendation_draft.no_alternative_reason = None;
    let replacement =
        ContextualRecommendation::from_draft(fixture.case.case_id.clone(), recommendation_draft)
            .unwrap();
    missing_alternative.preferred_recommendation_id = Some(replacement.recommendation_id.clone());
    missing_alternative.recommendations = vec![replacement];
    assert!(matches!(
        fixture.admit(missing_alternative),
        Err(SingleReviewerError::InvalidRecommendations(_))
    ));
    assert!(matches!(
        fixture.admit(review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            true,
        )),
        Err(SingleReviewerError::InvalidRecommendations(_))
    ));
    let mut mutation = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    mutation.model_resolved_probes = vec![fixture.results[0].clone()];
    assert!(matches!(
        fixture.admit(mutation),
        Err(SingleReviewerError::InvalidModelProbe(_))
    ));

    let dangling = auditor_review::EngineeringClaim::from_draft(
        fixture.case.case_id.clone(),
        ClaimDraft {
            slot: fixture.case.claim_slots[1],
            kind: auditor_review::EngineeringClaimKind::Actionability,
            proposition: "A dangling premise must never be admitted.".to_string(),
            status: ClaimStatus::Substantiated,
            supporting_probe_ids: vec![fixture.definitions[0].probe_id.clone()],
            counter_probe_ids: Vec::new(),
            dependency_claim_ids: vec![auditor_review::StableId::from_identity(
                "engineering-claim",
                1,
                &json!({"missing": true}),
            )
            .unwrap()],
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Low,
        },
    )
    .unwrap();
    assert!(matches!(
        fixture.admit(ReviewerCaseReviewPayload {
            case_conclusion: auditor_review::CaseConclusion::RepetitionJustified,
            case_position: CasePosition::NotSubstantiated,
            model_resolved_probes: Vec::new(),
            claims: vec![dangling],
            recommendations: Vec::new(),
            preferred_recommendation_id: None,
            unresolved_gaps: Vec::new(),
            proposed_closure: CaseClosureReason::NotSubstantiated,
        }),
        Err(SingleReviewerError::InvalidClaims(_))
    ));

    let mut forbidden = serde_json::to_value(review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    ))
    .unwrap();
    forbidden["recommendations"][0]["family"] = json!("forbidden_redesign");
    assert!(serde_json::from_value::<ReviewerCaseReviewPayload>(forbidden).is_err());

    let mut mutated_finding = review_payload(
        &fixture.case,
        &fixture.definitions,
        ClaimStatus::Substantiated,
        false,
    );
    let mut recommendation_draft = mutated_finding.recommendations[0].to_draft();
    recommendation_draft.affected_finding_ids = vec!["finding-outside-case".to_string()];
    let replacement =
        ContextualRecommendation::from_draft(fixture.case.case_id.clone(), recommendation_draft)
            .unwrap();
    mutated_finding.preferred_recommendation_id = Some(replacement.recommendation_id.clone());
    mutated_finding.recommendations = vec![replacement];
    assert!(matches!(
        fixture.admit(mutated_finding),
        Err(SingleReviewerError::InvalidRecommendations(_))
    ));
}

#[test]
fn review_tool_schema_is_exact_and_excludes_host_metadata() {
    let claim = auditor_review::CaseTool::RecordCaseClaim.parameters();
    assert_eq!(claim["additionalProperties"], false);
    assert!(claim["properties"].get("case_id").is_none());
    assert!(claim["properties"]["supporting_probe_ids"]
        .get("minItems")
        .is_none());
    let recommendation = auditor_review::CaseTool::RecordCaseRecommendation.parameters();
    assert_eq!(recommendation["additionalProperties"], false);
    assert!(recommendation["properties"].get("case_id").is_none());
    let benefit = &recommendation["properties"]["expected_benefits"]["items"];
    let variants = benefit["oneOf"].as_array().expect("typed benefit union");
    assert_eq!(variants.len(), 5);
    let structural = variants
        .iter()
        .find(|variant| {
            variant["properties"]["evidence_mode"]["enum"] == json!(["structural_inference"])
        })
        .expect("structural benefit variant");
    assert!(structural["properties"].get("numeric_value").is_none());
    let measured = variants
        .iter()
        .find(|variant| {
            variant["properties"]["evidence_mode"]["enum"] == json!(["direct_measurement"])
        })
        .expect("measured benefit variant");
    assert!(measured["required"]
        .as_array()
        .unwrap()
        .contains(&json!("numeric_value")));
}

#[test]
fn request_turn_requires_new_evidence_recompile_but_allows_an_idempotent_complete_read() {
    let fixture = ReviewFixture::new_unknown();
    let run_id = reviewer_run_id();
    let target = review_route_target();
    let request = CaseEvidenceRequest {
        case_id: fixture.case.case_id.clone(),
        probe_id: fixture.definitions[0].probe_id.clone(),
        request_kind: EvidenceLookupRequestKind::FindingSource,
        target_id: format!("finding:{}", fixture.case.finding_ids[0]),
        decision_relevance: "Confirm the selected source role.".to_string(),
    };
    let mut session = SingleReviewerSession::new(&fixture.case, &fixture.packet).unwrap();
    let context = |packet| ReviewerAdmissionContext {
        reviewer_run_id: &run_id,
        route_target: &target,
        case: &fixture.case,
        packet,
        probe_definitions: &fixture.definitions,
        deterministic_probe_results: &fixture.results,
    };
    assert!(matches!(
        session
            .consume_turn(
                CaseProviderTurn {
                    evidence_requests: vec![request.clone()],
                    case_review: None,
                    challenge: None,
                },
                context(&fixture.packet),
            )
            .unwrap(),
        SingleReviewerTurn::EvidenceRequested(_)
    ));
    assert_eq!(
        session.consume_turn(
            CaseProviderTurn {
                evidence_requests: Vec::new(),
                case_review: Some(wire_blocked_review(&fixture)),
                challenge: None,
            },
            context(&fixture.packet),
        ),
        Err(SingleReviewerError::EvidenceNotRecompiled)
    );

    let mut lookup = EvidenceLookup::new(
        &fixture.fixture.repo,
        &fixture.case,
        &fixture.input,
        EvidencePacketBudget::default(),
    )
    .unwrap();
    let lookup_result = lookup
        .execute(&EvidenceLookupRequest {
            case_id: request.case_id.clone(),
            probe_id: request.probe_id.clone(),
            kind: request.request_kind,
            target_id: request.target_id.clone(),
            decision_relevance: request.decision_relevance.clone(),
            hops: 0,
        })
        .unwrap();
    let expanded = EvidenceCompiler
        .compile_with_lookup_items(&fixture.case, &fixture.input, &lookup_result.items)
        .unwrap();
    assert!(matches!(
        session
            .consume_turn(
                CaseProviderTurn {
                    evidence_requests: Vec::new(),
                    case_review: Some(wire_blocked_review(&fixture)),
                    challenge: None,
                },
                context(&expanded),
            )
            .unwrap(),
        SingleReviewerTurn::Complete(_)
    ));

    let mut idempotent_session = SingleReviewerSession::new(&fixture.case, &expanded).unwrap();
    assert!(matches!(
        idempotent_session
            .consume_turn(
                CaseProviderTurn {
                    evidence_requests: vec![request],
                    case_review: None,
                    challenge: None,
                },
                context(&expanded),
            )
            .unwrap(),
        SingleReviewerTurn::EvidenceRequested(_)
    ));
    assert!(matches!(
        idempotent_session
            .consume_turn(
                CaseProviderTurn {
                    evidence_requests: Vec::new(),
                    case_review: Some(wire_blocked_review(&fixture)),
                    challenge: None,
                },
                context(&expanded),
            )
            .unwrap(),
        SingleReviewerTurn::Complete(_)
    ));
}

fn wire_blocked_review(fixture: &ReviewFixture) -> Value {
    let (premise_probe_id, premise_slot) = fixture
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
        .expect("blocked review fixture has a claim-blocking unresolved probe");
    json!({
        "case_conclusion": "insufficient_evidence",
        "case_position": "evidence_blocked",
        "model_resolved_probes": [],
        "claims": [{
            "claim_ref": "measurement-premise",
            "slot": premise_slot,
            "kind": "recommendation_premise",
            "proposition": "The unresolved lifecycle boundary requires focused evidence before changing ownership.",
            "status": "blocked",
            "supporting_probe_ids": [premise_probe_id.clone()],
            "counter_probe_ids": [],
            "dependency_claim_refs": [],
            "assumptions": [],
            "confidence": "medium"
        }],
        "recommendations": [{
            "recommendation_ref": "measure-before-change",
            "family": "measurement_before_change",
            "summary": "Resolve the missing lifecycle evidence before changing ownership.",
            "scope_ids": [fixture.case.finding_ids[0]],
            "premise_claim_refs": ["measurement-premise"],
            "affected_finding_ids": [fixture.case.finding_ids[0]],
            "detector_advice_relationship": "narrows",
            "preconditions": ["The missing behavior can be inspected or measured."],
            "expected_benefits": [],
            "risks": ["The measurement may confirm that the current ownership is required."],
            "costs": ["Requires focused inspection or instrumentation."],
            "alternatives": [],
            "verification_steps": [{
                "method": "Inspect or trace the unresolved lifecycle behavior.",
                "expected_observation": "The evidence resolves the blocked lifecycle boundary.",
                "evidence_probe_ids": [premise_probe_id]
            }],
            "materiality": "low",
            "materiality_basis": "Material impact is not established by the bounded evidence.",
            "confidence": "medium"
        }],
        "preferred_recommendation_ref": "measure-before-change",
        "unresolved_gaps": ["Required evidence remains unavailable."],
        "proposed_closure": "blocked_by_missing_evidence"
    })
}

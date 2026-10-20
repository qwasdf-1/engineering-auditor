use super::*;

pub(super) struct RoleRequest<'a> {
    pub(super) planned: &'a ReviewPlanCase,
    pub(super) runtime: &'a CaseExecutionRuntime<'a>,
    pub(super) role: ReviewRole,
    pub(super) pass: u8,
    pub(super) followup_attempt: u8,
    pub(super) followup: CaseCallFollowup,
    pub(super) case_context: CaseProviderContext,
    pub(super) reviewer_review: Option<&'a ReviewerCaseReview>,
    pub(super) challenge_target_ids: Option<&'a BTreeSet<StableId>>,
    pub(super) preferred_target: Option<crate::RouteTarget>,
    pub(super) evidence_round_allowed: bool,
    pub(super) conversation: Vec<NativeConversationMessage>,
}

pub(super) struct RoleTurnConfig {
    pub(super) base_instruction: String,
    pub(super) stage: ReviewStage,
    pub(super) stage_name: &'static str,
}

struct PreparedCall {
    provider_context: Value,
    context_markdown: String,
    tool_parameter_overrides: BTreeMap<String, Value>,
    evidence_requests_allowed: bool,
    config: RoleTurnConfig,
    instruction: String,
}

pub(super) fn execute_role_call(
    request: RoleRequest<'_>,
    state: &CaseEvidenceState,
    log: &mut CallLog,
) -> Result<CaseCallOutput, ReviewServiceError> {
    let prepared = prepare_call(&request, log)?;
    execute_logged_case_call(
        CaseCallRequest {
            case: &request.planned.case,
            packet: &state.packet,
            role: request.role,
            pass: request.pass,
            stage: prepared.config.stage,
            stage_name: prepared.config.stage_name,
            instruction: prepared.instruction,
            context: prepared.provider_context,
            context_markdown: prepared.context_markdown,
            conversation: with_followup_note(request.conversation, request.followup),
            output_schema_name: "case_task_calls",
            output_schema: json!({"items": {"type": "object"}, "type": "array"}),
            tool_parameter_overrides: prepared.tool_parameter_overrides,
            evidence_requests_allowed: prepared.evidence_requests_allowed,
            preferred_target: request.preferred_target,
            followup_attempt: request.followup_attempt,
            followup: request.followup,
        },
        call_runtime(request.runtime),
        log,
    )
}

fn prepare_call(
    request: &RoleRequest<'_>,
    log: &mut CallLog,
) -> Result<PreparedCall, ReviewServiceError> {
    let (provider_context, context_markdown) = render_context(request, log)?;
    let tool_parameter_overrides = task_schemas(request)?;
    let first_pass = request.pass == 1;
    let config = role_turn_config(
        request.runtime.prompts,
        request.role,
        first_pass,
        request.followup,
    )?;
    let instruction = config.base_instruction.clone();
    Ok(PreparedCall {
        provider_context,
        context_markdown,
        tool_parameter_overrides,
        evidence_requests_allowed: evidence_requests_allowed(request, first_pass),
        config,
        instruction,
    })
}

fn render_context(
    request: &RoleRequest<'_>,
    log: &mut CallLog,
) -> Result<(Value, String), ReviewServiceError> {
    let projection = request
        .case_context
        .render_markdown("Engineering review case")
        .map_err(failed)?;
    log.projection_audits.push(projection.audit);
    let mut context_markdown = projection.markdown;
    let mut provider_context = json!({"case_context": request.case_context});
    if let Some(review) = request.reviewer_review {
        append_review_context(review, &mut provider_context, &mut context_markdown, log)?;
    }
    Ok((provider_context, context_markdown))
}

fn append_review_context(
    review: &ReviewerCaseReview,
    provider_context: &mut Value,
    context_markdown: &mut String,
    log: &mut CallLog,
) -> Result<(), ReviewServiceError> {
    let review_document = reviewer_review_context(review);
    provider_context["reviewer_review"] = review_document.clone();
    let digest = sha256_digest_bytes(
        &serde_json::to_vec(&review_document).map_err(|error| failed(error.to_string()))?,
    );
    let projection = render_structured_model_document(
        "Admitted reviewer result",
        &review_document,
        digest,
        &[
            "reviewer_review.review_id",
            "reviewer_review.route_target",
            "reviewer_review.reviewer_run_id",
        ],
    )
    .map_err(|error| failed(error.to_string()))?;
    log.projection_audits.push(projection.audit);
    context_markdown.push('\n');
    context_markdown.push_str(&projection.markdown);
    Ok(())
}

fn task_schemas(request: &RoleRequest<'_>) -> Result<BTreeMap<String, Value>, ReviewServiceError> {
    let mut overrides = BTreeMap::new();
    if request.role == ReviewRole::Challenger {
        if let Some(review) = request.reviewer_review {
            bind_challenge_task_schemas(
                review,
                request.challenge_target_ids,
                &request.case_context,
                &mut overrides,
            );
        }
        return Ok(overrides);
    }
    overrides.insert(
        crate::TOOL_ANSWER_CASE_PROBE.to_string(),
        probe_answer_tool_schema(&request.case_context.model_probe_contracts),
    );
    bind_reviewer_task_schemas(&request.case_context, &request.planned.case, &mut overrides)?;
    Ok(overrides)
}

fn evidence_requests_allowed(request: &RoleRequest<'_>, first_pass: bool) -> bool {
    let lookup = &request.case_context.evidence.lookup_authority;
    let has_target = !lookup.finding_source_target_ids.is_empty()
        || !lookup.symbol_neighborhood_target_ids.is_empty();
    request.followup == CaseCallFollowup::None
        && request.evidence_round_allowed
        && first_pass
        && has_target
}

fn bind_reviewer_task_schemas(
    context: &CaseProviderContext,
    case: &EngineeringCase,
    overrides: &mut BTreeMap<String, Value>,
) -> Result<(), ReviewServiceError> {
    let probe_ids = context
        .model_probe_contracts
        .iter()
        .map(|probe| probe.probe_id.to_string())
        .chain(
            context
                .host_resolved_probes
                .iter()
                .map(|probe| probe.probe_id.to_string()),
        )
        .collect::<Vec<_>>();
    let claim_slots = serialized_array(&context.decision.claim_slots, "claim slots")?
        .into_iter()
        .filter(|slot| slot.as_str() != Some("lifecycle_conclusion"))
        .collect::<Vec<_>>();
    let recommendation_families = serialized_array(
        &context.decision.recommendation_families,
        "recommendation families",
    )?
    .into_iter()
    .filter_map(|family| family.get("family").cloned())
    .collect::<Vec<_>>();

    let mut claim = crate::CaseTool::RecordCaseClaim.parameters();
    claim["properties"]["slot"] = json!({"enum": claim_slots});
    claim["properties"]["supporting_probe_ids"]["items"] = json!({"enum": probe_ids});
    claim["properties"]["counter_probe_ids"]["items"] = json!({"enum": probe_ids});
    overrides.insert(crate::TOOL_RECORD_CASE_CLAIM.to_string(), claim);

    let mut recommendation = crate::CaseTool::RecordCaseRecommendation.parameters();
    bind_recommendation_schema(
        &mut recommendation,
        &recommendation_families,
        &context.decision.recommendation_scope_ids,
        &case.finding_ids,
        &probe_ids,
    );
    overrides.insert(
        crate::TOOL_RECORD_CASE_RECOMMENDATION.to_string(),
        recommendation,
    );

    let mut revision = crate::CaseTool::ReviseCaseRecommendation.parameters();
    bind_recommendation_schema(
        &mut revision["properties"]["replacement"],
        &recommendation_families,
        &context.decision.recommendation_scope_ids,
        &case.finding_ids,
        &probe_ids,
    );
    overrides.insert(crate::TOOL_REVISE_CASE_RECOMMENDATION.to_string(), revision);
    Ok(())
}

fn serialized_array(value: &impl Serialize, name: &str) -> Result<Vec<Value>, ReviewServiceError> {
    let serialized = serde_json::to_value(value)
        .map_err(|error| failed(format!("{name} could not be serialized: {error}")))?;
    serialized
        .as_array()
        .cloned()
        .ok_or_else(|| failed(format!("{name} did not serialize as an array")))
}

fn bind_recommendation_schema(
    schema: &mut Value,
    recommendation_families: &[Value],
    scope_ids: &[String],
    finding_ids: &[String],
    probe_ids: &[String],
) {
    schema["properties"]["family"] = json!({"enum": recommendation_families});
    schema["properties"]["scope_ids"]["items"] = json!({"enum": scope_ids});
    schema["properties"]["affected_finding_ids"]["items"] = json!({"enum": finding_ids});
    let benefit_variants = schema
        .pointer_mut("/properties/expected_benefits/items/oneOf")
        .and_then(Value::as_array_mut)
        .expect("expected-benefit schema is an exact oneOf union");
    for variant in benefit_variants {
        variant["properties"]["evidence_probe_ids"]["items"] = json!({"enum": probe_ids});
    }
    schema["properties"]["verification_steps"]["items"]["properties"]["evidence_probe_ids"]
        ["items"] = json!({"enum": probe_ids});
}

fn bind_challenge_task_schemas(
    review: &ReviewerCaseReview,
    target_filter: Option<&BTreeSet<StableId>>,
    context: &CaseProviderContext,
    overrides: &mut BTreeMap<String, Value>,
) {
    let targets = crate::case_review::challenge::challenge_targets(review)
        .into_iter()
        .filter(|(target_id, _)| target_filter.is_none_or(|filter| filter.contains(target_id)))
        .map(|(target_id, target_type)| {
            let target_type = match target_type {
                crate::ChallengeTargetType::Claim => "claim",
                crate::ChallengeTargetType::Recommendation => "recommendation",
            };
            (target_type, target_id.to_string())
        })
        .collect::<Vec<_>>();
    let evidence_ids = context
        .evidence
        .items
        .iter()
        .map(|item| item.id.clone())
        .chain(crate::case_review::challenge::reviewer_cited_probe_ids(
            review,
        ))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let target_types = targets
        .iter()
        .map(|(target_type, _)| *target_type)
        .collect::<BTreeSet<_>>();
    let target_ids = targets
        .iter()
        .map(|(_, target_id)| target_id)
        .collect::<Vec<_>>();
    let mut objection = crate::CaseTool::RecordCaseObjection.parameters();
    objection["properties"]["target_type"] = json!({"enum": target_types});
    objection["properties"]["target_id"] = json!({"enum": target_ids});
    objection["properties"]["evidence_ids"]["items"] = json!({"enum": evidence_ids});
    objection["properties"]["evidence_ids"]["minItems"] = Value::from(1);
    objection["properties"]["evidence_ids"]["uniqueItems"] = Value::Bool(true);
    overrides.insert(crate::TOOL_RECORD_CASE_OBJECTION.to_string(), objection);
}

use serde_json::Value;

use crate::{
    AgendaPlanner, CaseInstantiationResult, CaseTemplateRegistry, ClosureState, DecisionMagnitude,
    DeliberationRouter, DeterministicDisposition, EvidenceCompiler, EvidenceLookup,
    ModeAvailability, ProbeAnswer, ProbeLookupPlanner, ProbePlanner, ProbeResolverRegistry,
    RoutingFacts, RoutingInput,
};

use super::{
    failed, load::load_inputs, CaseInspection, ReviewInputSources, ReviewPlanCase,
    ReviewPlanOutput, ReviewPlanRequest, ReviewServiceError, REVIEW_PLAN_SCHEMA_VERSION,
};

pub fn plan_engineering_review(
    request: &ReviewPlanRequest,
) -> Result<ReviewPlanOutput, ReviewServiceError> {
    let loaded = load_inputs(request, None)?;
    let agenda = AgendaPlanner
        .plan_with_foundation_policy(&loaded.input)
        .map_err(|error| failed(error.to_string()))?;
    let registry = CaseTemplateRegistry::from_input(&loaded.input)
        .map_err(|error| failed(error.to_string()))?;
    let resolver = ProbeResolverRegistry::built_in().map_err(|error| failed(error.to_string()))?;
    let planning = PlanCaseContext {
        input: &loaded.input,
        bundle: registry.bundle(),
        resolver: &resolver,
        repo_root: &loaded.source_request.repo_root,
        shadow_ensemble_enabled: loaded.routes.ensemble.is_some(),
    };
    let mut cases = Vec::new();
    let mut unavailable_cases = Vec::new();
    for result in registry
        .instantiate_agenda(&agenda, &loaded.input)
        .map_err(|error| failed(error.to_string()))?
    {
        match result {
            CaseInstantiationResult::TemplateNotEnabled(record) => unavailable_cases.push(record),
            CaseInstantiationResult::Instantiated(case) => {
                cases.push(plan_case(&planning, *case)?);
            }
        }
    }
    cases.sort_by(|left, right| left.case.case_id.cmp(&right.case.case_id));
    unavailable_cases.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    let mut output = ReviewPlanOutput {
        schema_version: REVIEW_PLAN_SCHEMA_VERSION,
        plan_id: crate::StableId::from_identity(
            "review-plan",
            REVIEW_PLAN_SCHEMA_VERSION,
            &Value::Null,
        )
        .map_err(|error| failed(error.to_string()))?,
        input_binding: loaded.input.binding().clone(),
        sources: ReviewInputSources::from(request),
        agenda,
        cases,
        unavailable_cases,
    };
    output.plan_id = output.expected_id()?;
    output.validate()?;
    Ok(output)
}

struct PlanCaseContext<'a> {
    input: &'a crate::ReviewInput,
    bundle: &'a crate::CaseTemplateBundle,
    resolver: &'a ProbeResolverRegistry,
    repo_root: &'a std::path::Path,
    shadow_ensemble_enabled: bool,
}

fn plan_case(
    planning: &PlanCaseContext<'_>,
    case: crate::EngineeringCase,
) -> Result<ReviewPlanCase, ReviewServiceError> {
    let PlanCaseContext {
        input,
        bundle,
        resolver,
        repo_root,
        shadow_ensemble_enabled,
    } = planning;
    let seed_packet = EvidenceCompiler
        .compile(&case, input)
        .map_err(|error| failed(error.to_string()))?;
    let initial_source_items = EvidenceLookup::new(repo_root, &case, input, seed_packet.budget)
        .and_then(|mut lookup| lookup.initial_finding_sources())
        .map_err(|error| failed(error.to_string()))?;
    let packet = EvidenceCompiler
        .compile_with_budget_and_lookup_items(
            &case,
            input,
            seed_packet.budget,
            &initial_source_items,
        )
        .map_err(|error| failed(error.to_string()))?;
    let template = bundle
        .entry(case.case_kind)
        .and_then(|entry| entry.template.as_ref())
        .ok_or_else(|| failed("planned case has no enabled template"))?;
    let definitions = ProbePlanner
        .plan(&case, template, &packet, &[])
        .map_err(|error| failed(error.to_string()))?;
    let deterministic_results = definitions
        .iter()
        .map(|definition| resolver.resolve(&case, definition, &packet))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| failed(error.to_string()))?;
    let lookup_plan = ProbeLookupPlanner
        .plan(&case, &definitions, &packet)
        .map_err(|error| failed(error.to_string()))?;
    let facts = routing_facts(
        &definitions,
        &deterministic_results,
        &packet,
        *shadow_ensemble_enabled,
    );
    let routing = DeliberationRouter::new(ModeAvailability::with_shadow_ensemble(
        *shadow_ensemble_enabled,
    ))
    .route(RoutingInput {
        case: &case,
        packet: &packet,
        definitions: &definitions,
        results: &deterministic_results,
        facts: &facts,
    })
    .map_err(|error| failed(error.to_string()))?;
    Ok(ReviewPlanCase {
        case,
        evidence_packet: packet,
        probe_definitions: definitions,
        deterministic_probe_results: deterministic_results,
        lookup_plan,
        routing,
    })
}

fn routing_facts(
    definitions: &[crate::ProbeDefinition],
    results: &[crate::ProbeResult],
    packet: &crate::EvidencePacket,
    shadow_ensemble_enabled: bool,
) -> RoutingFacts {
    let repetition = definitions
        .iter()
        .find(|definition| definition.slot == "repetition")
        .and_then(|definition| {
            results
                .iter()
                .find(|result| result.probe_id == definition.probe_id)
        })
        .map(|result| result.answer);
    let deterministic_disposition = if matches!(
        repetition,
        Some(ProbeAnswer::No | ProbeAnswer::NotApplicable)
    ) {
        DeterministicDisposition::NotSubstantiated
    } else {
        DeterministicDisposition::Unresolved
    };
    RoutingFacts {
        deterministic_disposition,
        predicate_evidence: Vec::new(),
        operator_intent_known: true,
        local_and_reversible: true,
        credible_action_families: 1,
        higher_order_inference_required: deterministic_disposition
            == DeterministicDisposition::Unresolved,
        consequence: if shadow_ensemble_enabled {
            DecisionMagnitude::Medium
        } else {
            DecisionMagnitude::Low
        },
        uncertainty: if shadow_ensemble_enabled {
            DecisionMagnitude::Medium
        } else if packet.closure.state == ClosureState::Closed {
            DecisionMagnitude::Low
        } else {
            DecisionMagnitude::Medium
        },
        scope: if shadow_ensemble_enabled {
            DecisionMagnitude::Medium
        } else {
            DecisionMagnitude::Low
        },
        magnitude_evidence_ids: if shadow_ensemble_enabled {
            packet
                .items
                .first()
                .map(|item| vec![item.id.clone()])
                .unwrap_or_default()
        } else {
            Vec::new()
        },
    }
}

pub fn inspect_engineering_case(
    plan: &ReviewPlanOutput,
    case_id: &crate::StableId,
) -> Result<CaseInspection, ReviewServiceError> {
    plan.validate()?;
    let planned = plan
        .cases
        .iter()
        .find(|planned| &planned.case.case_id == case_id)
        .ok_or_else(|| failed(format!("unknown review case: {case_id}")))?;
    Ok(CaseInspection {
        plan_id: plan.plan_id.clone(),
        case: planned.case.clone(),
        finding_count: planned.case.finding_ids.len(),
        evidence_item_count: planned.evidence_packet.items.len(),
        evidence_bytes: planned.evidence_packet.byte_count,
        closure_state: planned.evidence_packet.closure.state,
        probe_definitions: planned.probe_definitions.clone(),
        deterministic_probe_results: planned.deterministic_probe_results.clone(),
        allowed_lookups: planned.lookup_plan.clone(),
        mode: planned.routing.selection.clone(),
        route_closure: planned.routing.closure.clone(),
    })
}

pub fn render_engineering_case_context(
    plan: &ReviewPlanOutput,
    case_id: &crate::StableId,
) -> Result<crate::ModelProjection, ReviewServiceError> {
    plan.validate()?;
    let planned = plan
        .cases
        .iter()
        .find(|planned| &planned.case.case_id == case_id)
        .ok_or_else(|| failed(format!("unknown review case: {case_id}")))?;
    let resolved = planned
        .deterministic_probe_results
        .iter()
        .filter(|result| result.resolver.kind != crate::ResolverKind::UnknownFallback)
        .cloned()
        .collect::<Vec<_>>();
    let resolved_ids = resolved
        .iter()
        .map(|result| &result.probe_id)
        .collect::<std::collections::BTreeSet<_>>();
    let unresolved = planned
        .probe_definitions
        .iter()
        .filter(|probe| !resolved_ids.contains(&probe.probe_id))
        .cloned()
        .collect::<Vec<_>>();
    crate::CaseProviderContext::compile(
        &planned.case,
        &planned.evidence_packet,
        &resolved,
        &unresolved,
    )
    .and_then(|context| context.render_markdown("Engineering review case"))
    .map_err(failed)
}

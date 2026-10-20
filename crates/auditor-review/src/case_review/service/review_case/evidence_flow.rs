use super::*;

pub(super) struct RequestedEvidence<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    requests: Vec<CaseEvidenceRequest>,
    calls: Vec<NativeToolCall>,
}

impl<'a> RequestedEvidence<'a> {
    pub(super) fn new(
        planned: &'a ReviewPlanCase,
        runtime: &'a CaseExecutionRuntime<'a>,
        requests: Vec<CaseEvidenceRequest>,
        calls: Vec<NativeToolCall>,
    ) -> Self {
        Self {
            planned,
            runtime,
            requests,
            calls,
        }
    }

    pub(super) fn append_to(
        self,
        state: &mut CaseEvidenceState,
        conversation: &mut Vec<NativeConversationMessage>,
        log: &mut CallLog,
    ) -> Result<(), ReviewServiceError> {
        let results = expand_union_evidence(self.planned, self.runtime, state, self.requests)?;
        conversation.extend(render_evidence_tool_results(&self.calls, &results, log)?);
        Ok(())
    }
}

pub(super) fn case_evidence_turn(requests: Vec<CaseEvidenceRequest>) -> CaseProviderTurn {
    CaseProviderTurn {
        evidence_requests: requests,
        case_review: None,
        challenge: None,
    }
}

pub(super) fn admitted_evidence_requests<T, E: ToString>(
    admission: Result<T, E>,
    requests: impl FnOnce(T) -> Option<Vec<CaseEvidenceRequest>>,
    unexpected: &str,
) -> Result<Vec<CaseEvidenceRequest>, ReviewServiceError> {
    let admitted = admission.map_err(|error| failed(error.to_string()))?;
    requests(admitted).ok_or_else(|| failed(unexpected))
}

pub(super) fn expand_union_evidence(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &mut CaseEvidenceState,
    requests: Vec<CaseEvidenceRequest>,
) -> Result<Vec<EvidenceLookupResult>, ReviewServiceError> {
    checkpoint(runtime.control, ReviewStage::CompilingEvidence)?;
    let results = perform_lookups(planned, runtime, state, requests)?;
    recompile_packet(planned, runtime, state)?;
    refresh_probes(planned, runtime, state)?;
    Ok(results)
}

fn perform_lookups(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &mut CaseEvidenceState,
    requests: Vec<CaseEvidenceRequest>,
) -> Result<Vec<EvidenceLookupResult>, ReviewServiceError> {
    let mut lookup = EvidenceLookup::new(
        &runtime.loaded.source_request.repo_root,
        &planned.case,
        &runtime.loaded.input,
        state.packet.budget,
    )
    .map_err(|error| failed(error.to_string()))?;
    let mut available_items = state.packet.items.clone();
    available_items.extend(state.lookup_items.clone());
    let mut results = Vec::with_capacity(requests.len());
    for request in requests {
        let result = resolve_request(&mut lookup, request, &available_items)?;
        available_items.extend(result.items.clone());
        state.lookup_items.extend(result.items.clone());
        results.push(result);
    }
    state
        .lookup_items
        .sort_by(|left, right| left.id.cmp(&right.id));
    state
        .lookup_items
        .dedup_by(|left, right| left.id == right.id);
    Ok(results)
}

fn resolve_request(
    lookup: &mut EvidenceLookup<'_>,
    request: CaseEvidenceRequest,
    available_items: &[EvidenceItem],
) -> Result<EvidenceLookupResult, ReviewServiceError> {
    let satisfied_items = satisfied_evidence_items(available_items, &request);
    let lookup_request = EvidenceLookupRequest {
        case_id: request.case_id,
        probe_id: request.probe_id,
        kind: request.request_kind,
        target_id: request.target_id,
        decision_relevance: request.decision_relevance,
        hops: 0,
    };
    if let Some(items) = satisfied_items {
        return EvidenceLookupResult::from_request(&lookup_request, items)
            .map_err(|error| failed(error.to_string()));
    }
    lookup
        .execute(&lookup_request)
        .map_err(|error| failed(error.to_string()))
}

fn recompile_packet(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &mut CaseEvidenceState,
) -> Result<(), ReviewServiceError> {
    let expanded = EvidenceCompiler
        .compile_with_budget_and_lookup_items(
            &planned.case,
            &runtime.loaded.input,
            state.packet.budget,
            &state.lookup_items,
        )
        .map_err(|error| failed(error.to_string()))?;
    if expanded.packet_digest != state.packet.packet_digest {
        state.packet_history.push(state.packet.clone());
    }
    state.packet = expanded;
    Ok(())
}

fn refresh_probes(
    planned: &ReviewPlanCase,
    runtime: &CaseExecutionRuntime<'_>,
    state: &mut CaseEvidenceState,
) -> Result<(), ReviewServiceError> {
    let templates = CaseTemplateBundle::from_value(
        runtime
            .loaded
            .input
            .case_template_bundle()
            .semantic_snapshot(),
    )
    .map_err(failed)?;
    let template = templates
        .entry(planned.case.case_kind)
        .and_then(|entry| entry.template.as_ref())
        .ok_or_else(|| failed("recompiled case has no enabled template"))?;
    state.definitions = ProbePlanner
        .plan(&planned.case, template, &state.packet, &[])
        .map_err(|error| failed(error.to_string()))?;
    let resolver = ProbeResolverRegistry::built_in().map_err(|error| failed(error.to_string()))?;
    state.deterministic_results = state
        .definitions
        .iter()
        .map(|definition| resolver.resolve(&planned.case, definition, &state.packet))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| failed(error.to_string()))?;
    Ok(())
}

fn render_evidence_tool_results(
    calls: &[NativeToolCall],
    results: &[EvidenceLookupResult],
    log: &mut CallLog,
) -> Result<Vec<NativeConversationMessage>, ReviewServiceError> {
    if calls.len() != results.len() {
        return Err(failed(format!(
            "evidence tool/result count mismatch: {} calls, {} results",
            calls.len(),
            results.len()
        )));
    }
    calls
        .iter()
        .zip(results)
        .map(|(call, result)| {
            let projection = render_evidence_items_markdown(
                &format!("Evidence lookup result {}", result.request_id),
                &result.items,
            )
            .map_err(failed)?;
            log.projection_audits.push(projection.audit);
            Ok(NativeConversationMessage::ToolResult {
                call_id: call.call_id.clone(),
                tool_name: call.name.clone(),
                content_markdown: projection.markdown,
            })
        })
        .collect()
}

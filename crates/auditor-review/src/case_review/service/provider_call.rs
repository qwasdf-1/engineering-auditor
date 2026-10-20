use serde_json::Value;

use crate::{
    case_tools_for_role, challenge_response_tools, AssessmentModeRecorded, BuiltinAdapter,
    CallDispatch, CaseBudgetLedger, CaseProviderCallRecord, CaseQualificationRecord,
    EgressManifest, NativeConversationMessage, ProviderFailureClass, ProviderRequest,
    RepairContext, ReviewControl, ReviewRole, ReviewRoutesV2, ReviewStage, RouteTarget,
    TranscriptRecorder, Transport, V2ProviderChain,
};

use super::{
    checkpoint, failed,
    native_repair::{native_output_repair, NativeOutputRepair},
    provider_record::{
        complete_failed_attempt, complete_successful_attempt, egress_manifest, now_millis,
        AttemptAvailability, AttemptRecordInput, FailedAttempt,
    },
    ReviewServiceError,
};
use crate::provider::{wait_before_no_model_output_retry, MAX_NO_MODEL_OUTPUT_RETRIES};

const MAX_SCHEMA_REPAIR_ATTEMPTS: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CaseCallFollowup {
    None,
    AcceptedProgress,
    HostCorrection,
}

impl CaseCallFollowup {
    fn is_followup(self) -> bool {
        self != Self::None
    }

    fn is_host_correction(self) -> bool {
        self == Self::HostCorrection
    }
}

pub(super) struct CaseCallOutput {
    pub value: Value,
    pub target: RouteTarget,
    pub calls: Vec<CaseProviderCallRecord>,
    pub egress: Vec<EgressManifest>,
}

pub(super) struct CaseCallFailure {
    pub error: ReviewServiceError,
    pub calls: Vec<CaseProviderCallRecord>,
    pub egress: Vec<EgressManifest>,
}

pub(super) struct CaseCallRequest<'a> {
    pub case: &'a crate::EngineeringCase,
    pub packet: &'a crate::EvidencePacket,
    pub role: ReviewRole,
    pub pass: u8,
    pub stage: ReviewStage,
    pub stage_name: &'static str,
    pub instruction: String,
    pub context: Value,
    pub context_markdown: String,
    pub conversation: Vec<NativeConversationMessage>,
    pub output_schema_name: &'static str,
    pub output_schema: Value,
    pub tool_parameter_overrides: std::collections::BTreeMap<String, Value>,
    pub evidence_requests_allowed: bool,
    pub preferred_target: Option<RouteTarget>,
    pub followup_attempt: u8,
    pub followup: CaseCallFollowup,
}

pub(super) struct CaseCallRuntime<'a> {
    pub routes: &'a ReviewRoutesV2,
    pub qualifications: &'a [CaseQualificationRecord],
    pub ledger: &'a CaseBudgetLedger,
    pub transport: &'a dyn Transport,
    pub consent: crate::HostedConsent,
    pub control: &'a dyn ReviewControl,
    pub transcript: &'a TranscriptRecorder,
    pub transcript_enabled: bool,
}

#[derive(Default)]
struct CaseCallState {
    calls: Vec<CaseProviderCallRecord>,
    egress: Vec<EgressManifest>,
    failures: Vec<String>,
}

struct QualifiedTarget<'a> {
    route: &'a RouteTarget,
    provider: &'a crate::ProviderConfig,
    tool_mode: bool,
}

enum TargetStep {
    Complete(CaseCallOutput),
    Retry,
    Repair(RepairContext),
    NativeRepair(NativeOutputRepair),
    Stop,
}

struct TargetAttemptProgress {
    case_tools: Option<Vec<crate::ToolSpec>>,
    retries_remaining: u8,
    repair_context: Option<RepairContext>,
    schema_repair_attempt: u8,
    malformed_argument_repair: bool,
    native_conversation: Vec<NativeConversationMessage>,
}

struct TargetAttemptContext<'a, 'case_data, 'runtime_data, 'target_data> {
    request: &'a CaseCallRequest<'case_data>,
    runtime: &'a CaseCallRuntime<'runtime_data>,
    target: &'a QualifiedTarget<'target_data>,
    native_task_protocol: bool,
}

struct PreparedProviderRequest {
    request: ProviderRequest,
    input_bytes: usize,
    reserved_output_tokens: usize,
}

impl TargetAttemptProgress {
    fn new(request: &CaseCallRequest<'_>, target: &QualifiedTarget<'_>) -> Self {
        Self {
            case_tools: tools_for_request(request, target.tool_mode),
            retries_remaining: MAX_NO_MODEL_OUTPUT_RETRIES,
            repair_context: None,
            schema_repair_attempt: 0,
            malformed_argument_repair: false,
            native_conversation: request.conversation.clone(),
        }
    }

    fn record_retry(&mut self) {
        let retry_number = MAX_NO_MODEL_OUTPUT_RETRIES - self.retries_remaining;
        self.retries_remaining -= 1;
        wait_before_no_model_output_retry(retry_number);
    }

    fn record_schema_repair(&mut self, context: RepairContext) {
        self.schema_repair_attempt = self.schema_repair_attempt.saturating_add(1);
        self.repair_context = Some(context);
    }

    fn record_native_repair(&mut self, repair: NativeOutputRepair, request: &CaseCallRequest<'_>) {
        match repair {
            NativeOutputRepair::Replay(messages) => self.native_conversation.extend(messages),
            NativeOutputRepair::FreshCorrection(correction) => {
                self.native_conversation = request.conversation.clone();
                self.native_conversation.push(correction);
                self.malformed_argument_repair = true;
            }
        }
        self.schema_repair_attempt = self.schema_repair_attempt.saturating_add(1);
        if let Some(tools) = self.case_tools.as_mut() {
            enable_followup_tools(tools, request);
        }
    }
}

pub(super) fn execute_case_call(
    request: CaseCallRequest<'_>,
    runtime: CaseCallRuntime<'_>,
) -> Result<CaseCallOutput, CaseCallFailure> {
    let chain = route_chain(request.role, runtime.routes)?;
    let mut state = CaseCallState {
        calls: Vec::new(),
        egress: Vec::new(),
        failures: Vec::new(),
    };
    let targets = request
        .preferred_target
        .iter()
        .chain(
            chain
                .targets
                .iter()
                .filter(|target| Some(*target) != request.preferred_target.as_ref()),
        )
        .collect::<Vec<_>>();
    for target in targets {
        let Some(qualification) = qualification_for(
            runtime.qualifications,
            target,
            request.role,
            request.case.case_kind,
        ) else {
            state.failures.push(format!(
                "{}:{} is not qualified",
                target.provider_id, target.model
            ));
            continue;
        };
        let Some(provider) = runtime.routes.provider(&target.provider_id) else {
            return Err(case_call_failure(
                state,
                failed("case route references an unknown provider"),
            ));
        };
        let tool_mode = qualification.native_protocol_valid;
        if !tool_mode {
            state.failures.push(format!(
                "{}:{} does not support the required native tool protocol",
                target.provider_id, target.model
            ));
            continue;
        }
        let qualified = QualifiedTarget {
            route: target,
            provider,
            tool_mode,
        };
        match execute_qualified_target(&request, &runtime, qualified, &mut state) {
            Ok(Some(output)) => return Ok(output),
            Ok(None) => {}
            Err(error) => return Err(case_call_failure(state, error)),
        }
    }
    let error = failed(format!(
        "no qualified case target completed {}: {}",
        request.stage_name,
        state.failures.join("; ")
    ));
    Err(case_call_failure(state, error))
}

fn route_chain(
    role: ReviewRole,
    routes: &ReviewRoutesV2,
) -> Result<&V2ProviderChain, CaseCallFailure> {
    match role {
        ReviewRole::SingleReviewer => Ok(&routes.single_reviewer),
        ReviewRole::ReviewerA | ReviewRole::ReviewerB | ReviewRole::ReviewerC => {
            ensemble_route_chain(role, routes)
        }
        ReviewRole::Challenger => Ok(&routes.challenger),
        ReviewRole::AlternativeComparator => {
            routes.alternative_comparator.as_ref().ok_or_else(|| {
                case_call_failure(
                    CaseCallState::default(),
                    failed("alternative comparator route is absent"),
                )
            })
        }
    }
}

fn ensemble_route_chain(
    role: ReviewRole,
    routes: &ReviewRoutesV2,
) -> Result<&V2ProviderChain, CaseCallFailure> {
    let ensemble = routes.ensemble.as_ref().ok_or_else(|| {
        case_call_failure(
            CaseCallState::default(),
            failed("requested ensemble reviewer route is absent"),
        )
    })?;
    match role {
        ReviewRole::ReviewerA => Ok(&ensemble.reviewer_a),
        ReviewRole::ReviewerB => Ok(&ensemble.reviewer_b),
        ReviewRole::ReviewerC => Ok(&ensemble.reviewer_c),
        _ => Err(case_call_failure(
            CaseCallState::default(),
            failed("ensemble route dispatch requires an ensemble reviewer"),
        )),
    }
}

fn execute_qualified_target(
    request: &CaseCallRequest<'_>,
    runtime: &CaseCallRuntime<'_>,
    target: QualifiedTarget<'_>,
    state: &mut CaseCallState,
) -> Result<Option<CaseCallOutput>, ReviewServiceError> {
    let native_task_protocol = matches!(
        request.output_schema_name,
        "case_task_calls" | "challenge_response_tasks"
    );
    let mut progress = TargetAttemptProgress::new(request, &target);
    let context = TargetAttemptContext {
        request,
        runtime,
        target: &target,
        native_task_protocol,
    };
    loop {
        match execute_target_attempt(&context, state, &progress)? {
            TargetStep::Complete(output) => return Ok(Some(output)),
            TargetStep::Retry => progress.record_retry(),
            TargetStep::Repair(context) => progress.record_schema_repair(context),
            TargetStep::NativeRepair(repair) => progress.record_native_repair(repair, request),
            TargetStep::Stop => return Ok(None),
        }
    }
}

fn tools_for_request(
    request: &CaseCallRequest<'_>,
    tool_mode: bool,
) -> Option<Vec<crate::ToolSpec>> {
    tool_mode.then(|| {
        let mut tools = if request.output_schema_name == "challenge_response_tasks" {
            challenge_response_tools(true, request.followup.is_followup())
        } else {
            case_tools_for_role(
                request.role,
                true,
                request.evidence_requests_allowed,
                request.followup.is_followup(),
            )
        };
        for tool in &mut tools {
            if let Some(parameters) = request.tool_parameter_overrides.get(tool.name) {
                tool.parameters = parameters.clone();
            }
        }
        tools
    })
}

fn execute_target_attempt(
    context: &TargetAttemptContext<'_, '_, '_, '_>,
    state: &mut CaseCallState,
    progress: &TargetAttemptProgress,
) -> Result<TargetStep, ReviewServiceError> {
    let repair_attempts = u8::from(context.request.followup.is_host_correction())
        .saturating_add(progress.schema_repair_attempt);
    let dispatch = case_dispatch(
        progress.repair_context.as_ref(),
        progress.case_tools.as_ref(),
        progress.native_conversation.clone(),
    )?;
    checkpoint(context.runtime.control, context.request.stage)?;
    let prepared = prepare_provider_request(context, progress)?;
    let started = now_millis();
    state
        .egress
        .push(egress_manifest(context.request, prepared.input_bytes));
    let result = BuiltinAdapter::new(
        context.target.provider,
        context.runtime.transport,
        context.runtime.consent,
    )
    .execute_dispatched(
        &context.target.route.model,
        &prepared.request,
        &dispatch,
        context.runtime.transcript,
        context.runtime.transcript_enabled,
    );
    let attempt = AttemptRecordInput {
        provider: context.target.provider,
        target: context.target.route,
        ledger: context.runtime.ledger,
        case_request: context.request,
        provider_request: &prepared.request,
        assessment_mode: AssessmentModeRecorded::from_mode(dispatch.assessment_mode),
        repair_attempts,
        input_bytes: prepared.input_bytes,
        reserved_output_tokens: prepared.reserved_output_tokens,
        started,
        finished: now_millis().max(started),
    };
    match result {
        Ok(response) => {
            complete_successful_attempt(response, attempt, &mut state.calls, &mut state.egress)
                .map(TargetStep::Complete)
        }
        Err(failure) => failed_target_step(context, failure, attempt, progress, state),
    }
}

fn prepare_provider_request(
    context: &TargetAttemptContext<'_, '_, '_, '_>,
    progress: &TargetAttemptProgress,
) -> Result<PreparedProviderRequest, ReviewServiceError> {
    let request = context.request;
    let budget = context.runtime.routes.budget_for(request.case.case_kind);
    let input_bytes = request.context_markdown.len()
        + request.instruction.len()
        + progress
            .native_conversation
            .iter()
            .map(native_message_bytes)
            .sum::<usize>();
    let reserved_output_tokens = context
        .runtime
        .ledger
        .reserve(
            &request.case.case_id,
            request.case.case_kind,
            input_bytes,
            budget.max_output_tokens,
            context
                .target
                .provider
                .estimated_cost_microusd_per_request
                .unwrap_or(0),
        )
        .map_err(|error| failed(error.to_string()))?;
    let request_payload = ProviderRequest {
        request_id: case_request_id(
            request,
            progress.schema_repair_attempt,
            progress.malformed_argument_repair,
        ),
        stage: request.stage_name.to_string(),
        system_instruction: request.instruction.clone(),
        context: request.context.clone(),
        context_markdown: Some(request.context_markdown.clone()),
        output_schema_name: request.output_schema_name.to_string(),
        output_schema: request.output_schema.clone(),
        max_input_bytes: budget.max_input_bytes,
        max_output_tokens: reserved_output_tokens,
        timeout_ms: budget.timeout_ms,
    };
    Ok(PreparedProviderRequest {
        request: request_payload,
        input_bytes,
        reserved_output_tokens,
    })
}

fn failed_target_step(
    context: &TargetAttemptContext<'_, '_, '_, '_>,
    failure: crate::ProviderFailure,
    attempt: AttemptRecordInput<'_, '_>,
    progress: &TargetAttemptProgress,
    state: &mut CaseCallState,
) -> Result<TargetStep, ReviewServiceError> {
    let native_repair = native_repair_for_failure(
        context,
        &failure,
        progress.schema_repair_attempt,
        progress.case_tools.as_deref(),
    );
    let outcome = complete_failed_attempt(
        failure,
        attempt,
        AttemptAvailability {
            retry: progress.retries_remaining > 0,
            repair: !context.native_task_protocol
                && progress.schema_repair_attempt < MAX_SCHEMA_REPAIR_ATTEMPTS,
        },
        &mut state.calls,
        &mut state.failures,
    )?;
    Ok(match outcome {
        FailedAttempt::Retry => TargetStep::Retry,
        FailedAttempt::Repair(context) => TargetStep::Repair(context),
        FailedAttempt::Stop => native_repair.map_or(TargetStep::Stop, TargetStep::NativeRepair),
    })
}

fn native_repair_for_failure(
    context: &TargetAttemptContext<'_, '_, '_, '_>,
    failure: &crate::ProviderFailure,
    repair_attempt: u8,
    tools: Option<&[crate::ToolSpec]>,
) -> Option<NativeOutputRepair> {
    if !context.native_task_protocol
        || failure.class != ProviderFailureClass::InvalidOutput
        || repair_attempt >= MAX_SCHEMA_REPAIR_ATTEMPTS
    {
        return None;
    }
    failure
        .prior_assistant_output
        .as_deref()
        .and_then(|raw| {
            crate::provider::parse_native_assistant_output(
                context
                    .target
                    .provider
                    .protocol_for(&context.target.route.model),
                raw,
            )
        })
        .map(|assistant| native_output_repair(assistant, &failure.message, tools))
}

fn enable_followup_tools(tools: &mut Vec<crate::ToolSpec>, request: &CaseCallRequest<'_>) {
    tools.retain(|tool| tool.name != crate::TOOL_REQUEST_CASE_EVIDENCE);
    let additions = match request.output_schema_name {
        "case_task_calls" => case_tools_for_role(request.role, true, false, true),
        "challenge_response_tasks" => challenge_response_tools(true, true),
        _ => Vec::new(),
    };
    for tool in additions {
        if !tools.iter().any(|existing| existing.name == tool.name) {
            tools.push(tool);
        }
    }
}

fn case_request_id(
    request: &CaseCallRequest<'_>,
    schema_repair_attempt: u8,
    malformed_argument_repair: bool,
) -> String {
    let mut id = format!(
        "{}:{:?}:{}",
        request.case.case_id, request.role, request.pass
    );
    match request.followup {
        CaseCallFollowup::None => {}
        CaseCallFollowup::AcceptedProgress => {
            id.push_str(&format!(":continuation-{}", request.followup_attempt))
        }
        CaseCallFollowup::HostCorrection => {
            id.push_str(&format!(":host-correction-{}", request.followup_attempt))
        }
    }
    if schema_repair_attempt > 0 {
        id.push_str(&format!(":schema-repair-{schema_repair_attempt}"));
    }
    if malformed_argument_repair {
        id.push_str(":malformed-native-arguments");
    }
    id
}

fn case_dispatch<'a>(
    repair: Option<&'a RepairContext>,
    tools: Option<&Vec<crate::ToolSpec>>,
    conversation: Vec<NativeConversationMessage>,
) -> Result<CallDispatch<'a>, ReviewServiceError> {
    let dispatch = match (repair, tools) {
        (Some(repair), Some(tools)) => {
            CallDispatch::repair_tool_call_with_tools(repair, tools.clone())
        }
        (Some(_), None) => {
            return Err(failed(
                "case repair requires the original provider-native tool set",
            ))
        }
        (None, Some(tools)) => CallDispatch::tool_call(tools.clone()),
        (None, None) => return Err(failed("case dispatch requires a provider-native tool set")),
    };
    Ok(dispatch.with_conversation(conversation))
}

fn native_message_bytes(message: &NativeConversationMessage) -> usize {
    match message {
        NativeConversationMessage::Assistant { text, tool_calls } => {
            text.as_ref().map_or(0, String::len)
                + tool_calls
                    .iter()
                    .map(|call| call.call_id.len() + call.name.len() + call.raw_arguments.len())
                    .sum::<usize>()
        }
        NativeConversationMessage::ToolResult {
            call_id,
            tool_name,
            content_markdown,
        } => call_id.len() + tool_name.len() + content_markdown.len(),
        NativeConversationMessage::UserCorrection { content_markdown }
        | NativeConversationMessage::UserContinuation { content_markdown } => {
            content_markdown.len()
        }
    }
}

fn case_call_failure(state: CaseCallState, error: ReviewServiceError) -> CaseCallFailure {
    CaseCallFailure {
        error,
        calls: state.calls,
        egress: state.egress,
    }
}

fn qualification_for<'a>(
    records: &'a [CaseQualificationRecord],
    target: &RouteTarget,
    role: ReviewRole,
    kind: crate::CaseKind,
) -> Option<&'a CaseQualificationRecord> {
    records.iter().find(|record| {
        record.is_qualified()
            && record.binding.target == *target
            && record.binding.role == role
            && record.binding.case_kind == kind
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_schema_repair_closes_every_replayed_call_before_correction() {
        let assistant = NativeConversationMessage::Assistant {
            text: Some("partial native turn".to_string()),
            tool_calls: vec![
                crate::NativeToolCall::new("call-1", "record_case_claim", json!({}), "{}"),
                crate::NativeToolCall::new(
                    "call-2",
                    "record_case_recommendation",
                    json!({"complete": true}),
                    "{\"complete\":true}",
                ),
            ],
        };

        let NativeOutputRepair::Replay(messages) =
            native_output_repair(assistant.clone(), "expected object", None)
        else {
            panic!("valid native arguments must preserve the assistant tool-call turn")
        };
        assert_eq!(messages[0], assistant);
        assert!(matches!(
            &messages[1],
            NativeConversationMessage::ToolResult { call_id, tool_name, content_markdown }
                if call_id == "call-1"
                    && tool_name == "record_case_claim"
                    && content_markdown.contains("not recorded")
        ));
        assert!(matches!(
            &messages[2],
            NativeConversationMessage::ToolResult { call_id, tool_name, content_markdown }
                if call_id == "call-2"
                    && tool_name == "record_case_recommendation"
                    && content_markdown.contains("not recorded")
        ));
        assert!(matches!(
            &messages[3],
            NativeConversationMessage::UserCorrection { content_markdown }
                if content_markdown.contains("expected object")
                    && content_markdown.contains("No call from that assistant turn was recorded")
        ));
    }

    #[test]
    fn malformed_native_arguments_use_fresh_readable_correction() {
        let assistant = NativeConversationMessage::Assistant {
            text: Some("attempted final turn".to_string()),
            tool_calls: vec![
                crate::NativeToolCall::new(
                    "call-broken",
                    "record_case_recommendation",
                    Value::Null,
                    "{\"recommendation_ref\":",
                ),
                crate::NativeToolCall::new(
                    "call-valid",
                    "complete_case_review",
                    json!({"preferred_recommendation_ref": null, "unresolved_gaps": []}),
                    "{\"preferred_recommendation_ref\":null,\"unresolved_gaps\":[]}",
                ),
            ],
        };
        let tools = vec![crate::ToolSpec {
            name: "record_case_recommendation",
            description: "Record a recommendation",
            parameters: json!({
                "type": "object",
                "properties": {
                    "family": {"enum": ["lifecycle_reownership", "measurement_before_change"]},
                    "recommendation_ref": {"type": "string"}
                },
                "required": ["recommendation_ref", "family"]
            }),
            strict: true,
        }];

        let NativeOutputRepair::FreshCorrection(message) = native_output_repair(
            assistant,
            "provider output requires type object",
            Some(&tools),
        ) else {
            panic!("malformed arguments must not be replayed");
        };
        assert!(matches!(
            message,
            NativeConversationMessage::UserCorrection { content_markdown }
                if content_markdown.contains("malformed-native-arguments")
                    && content_markdown.contains("call-broken")
                    && content_markdown.contains("Required argument fields")
                    && content_markdown.contains("Allowed values for `family`")
                    && content_markdown.contains("Resubmit every intended call")
                    && !content_markdown.contains("{\"recommendation_ref\"")
        ));
    }
}

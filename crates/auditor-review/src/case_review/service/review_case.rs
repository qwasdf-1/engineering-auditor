use std::collections::{BTreeMap, BTreeSet};

use auditor_core::sha256_digest_bytes;
use serde::Serialize;
use serde_json::{json, Value};

use crate::case_review::deliberation::{
    probe_answer_tool_schema, render_evidence_items_markdown, satisfied_evidence_items,
    validate_probe_answer_arguments, validate_recommendation_benefit_arguments,
    ProviderProbeDefinition,
};
use crate::{
    render_structured_model_document, CaseBudgetLedger, CaseClosure, CaseEvidenceRequest,
    CaseFailure, CaseFinalizationContext, CaseProviderContext, CaseProviderTurn,
    CaseTemplateBundle, ChallengeAdmissionContext, ChallengeOutcome, ChallengeResponse,
    ChallengeResponseAdmission, ChallengeSession, ChallengeTaskLedger, ChallengeTaskRequirements,
    ChallengeTurn, ContextualRecommendation, DeliberationMode, DeterministicDisposition,
    EngineeringCase, EvidenceCompiler, EvidenceItem, EvidenceLookup, EvidenceLookupRequest,
    EvidenceLookupResult, FinalizedCase, ModeReason, NativeConversationMessage, NativeToolCall,
    NativeToolTurn, ProbePlanner, ProbeResolverRegistry, RecommendationFamily, ResponseTaskLedger,
    ResponseTaskRequirements, ReviewControl, ReviewPlanCase, ReviewPromptBundleV2, ReviewRole,
    ReviewRoutesV2, ReviewStage, ReviewerAdmissionContext, ReviewerCaseReview, ReviewerTaskLedger,
    ReviewerTaskRequirements, SingleReviewerSession, SingleReviewerTurn, StableId, TaskLedgerError,
    TranscriptRecorder, Transport,
};

use super::provider_call::{
    execute_case_call, CaseCallFollowup, CaseCallOutput, CaseCallRequest, CaseCallRuntime,
};
use super::{checkpoint, failed, LoadedPlanInputs, ReviewServiceError};

mod challenge_tasks;
mod challenger_flow;
mod coordinator;
mod evidence_flow;
mod failure;
mod projection;
mod response_flow;
mod response_tasks;
mod reviewer_flow;
mod reviewer_tasks;
mod role_call;
mod task_application;
mod task_support;
mod turn_config;
mod turn_protocol;

use challenge_tasks::apply_challenge_task_calls;
use challenger_flow::{run_challenge, ChallengeRequest};
pub(super) use coordinator::execute_planned_case;
use evidence_flow::{
    admitted_evidence_requests, case_evidence_turn, expand_union_evidence, RequestedEvidence,
};
use failure::{case_failure_detail_codes, classify_case_failure};
use projection::{
    challenge_turn_from_ledger, response_value_from_ledger, reviewer_conclusion_from_ledger,
    reviewer_turn_from_ledger,
};
use response_flow::respond_to_challenge;
use response_tasks::apply_response_task_calls;
use reviewer_flow::{resume_ensemble_reviewer, run_ensemble_reviewer_pass_a, run_reviewer};
use reviewer_tasks::{apply_reviewer_task_calls, ReviewerInput};
use role_call::{execute_role_call, RoleRequest, RoleTurnConfig};
use task_support::{
    enum_name, mark_last_call_host_rejected, optional_string_field, recommendation_replacement,
    record_challenge_task_audit, record_response_task_audit, record_reviewer_task_audit,
    render_task_correction, revision_arguments, string_array_field, string_field, task_error,
    task_tool_result, transition_arguments,
};
use turn_config::{role_turn_config, with_followup_note};
use turn_protocol::record_native_assistant_turn;

pub(super) struct CompletedCase {
    pub packet: crate::EvidencePacket,
    pub packet_history: Vec<crate::EvidencePacket>,
    pub definitions: Vec<crate::ProbeDefinition>,
    pub results: Vec<crate::ProbeResult>,
    pub review: Option<ReviewerCaseReview>,
    pub ensemble_run: Option<crate::EnsembleCaseRun>,
    pub ensemble_failure: Option<crate::EnsembleCaseFailure>,
    pub ensemble_evidence_packets: Vec<crate::EvidencePacket>,
    pub ensemble_challenges: Vec<crate::EnsembleChallengeExecution>,
    pub challenge: Option<ChallengeOutcome>,
    pub finalization: FinalizedCase,
    pub calls: Vec<crate::CaseProviderCallRecord>,
    pub egress: Vec<crate::EgressManifest>,
    pub claim_slot_dispositions: Vec<crate::TaskDeclineDisposition>,
    pub recommendation_family_dispositions: Vec<crate::TaskDeclineDisposition>,
    pub challenge_target_dispositions: Vec<crate::ChallengeTargetDispositionRecord>,
    pub semantic_revisions: Vec<crate::SemanticRevisionRecord>,
    pub projection_audits: Vec<crate::ModelProjectionAudit>,
}

pub(super) struct CaseExecutionRuntime<'a> {
    pub loaded: &'a LoadedPlanInputs,
    pub routes: &'a ReviewRoutesV2,
    pub prompts: &'a ReviewPromptBundleV2,
    pub qualifications: &'a [crate::CaseQualificationRecord],
    pub ledger: &'a CaseBudgetLedger,
    pub transport: &'a dyn Transport,
    pub consent: crate::HostedConsent,
    pub control: &'a dyn ReviewControl,
    pub transcript: &'a TranscriptRecorder,
    pub transcript_enabled: bool,
}

#[derive(Clone)]
struct CaseEvidenceState {
    packet: crate::EvidencePacket,
    packet_history: Vec<crate::EvidencePacket>,
    definitions: Vec<crate::ProbeDefinition>,
    deterministic_results: Vec<crate::ProbeResult>,
    lookup_items: Vec<EvidenceItem>,
}

#[derive(Default)]
struct CallLog {
    calls: Vec<crate::CaseProviderCallRecord>,
    egress: Vec<crate::EgressManifest>,
    claim_slot_dispositions: Vec<crate::TaskDeclineDisposition>,
    recommendation_family_dispositions: Vec<crate::TaskDeclineDisposition>,
    challenge_target_dispositions: Vec<crate::ChallengeTargetDispositionRecord>,
    semantic_revisions: Vec<crate::SemanticRevisionRecord>,
    projection_audits: Vec<crate::ModelProjectionAudit>,
}

struct ReviewerRoute {
    run_id: StableId,
    target: crate::RouteTarget,
}

struct ReviewerRequest<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    reviewer_run_id: StableId,
    role: ReviewRole,
    preferred_target: Option<crate::RouteTarget>,
    evidence_policy: ReviewerEvidencePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewerEvidencePolicy {
    ResolveImmediately,
    DeferForUnion,
    Disabled,
}

struct ResponseRequest<'a> {
    planned: &'a ReviewPlanCase,
    runtime: &'a CaseExecutionRuntime<'a>,
    review: &'a ReviewerCaseReview,
    reviewer_route: &'a ReviewerRoute,
    challenge: &'a crate::ChallengeRecord,
}

const MAX_HOST_INVALID_ATTEMPTS: u8 = 3;
const MAX_CONSECUTIVE_NO_PROGRESS_TURNS: u8 = 3;

#[derive(Default)]
struct TaskTurnBudget {
    followup_turns: u8,
    consecutive_invalid_turns: u8,
    consecutive_no_progress_turns: u8,
}

impl TaskTurnBudget {
    fn record_progress_followup(&mut self) {
        self.followup_turns = self.followup_turns.saturating_add(1);
        self.consecutive_invalid_turns = 0;
        self.consecutive_no_progress_turns = 0;
    }

    fn record_invalid(&mut self, made_progress: bool) -> bool {
        self.followup_turns = self.followup_turns.saturating_add(1);
        if made_progress {
            self.consecutive_invalid_turns = 0;
        } else {
            self.consecutive_invalid_turns = self.consecutive_invalid_turns.saturating_add(1);
        }
        self.consecutive_invalid_turns > MAX_HOST_INVALID_ATTEMPTS
    }

    fn record_incomplete(&mut self, made_progress: bool) -> bool {
        self.followup_turns = self.followup_turns.saturating_add(1);
        self.consecutive_invalid_turns = 0;
        if made_progress {
            self.consecutive_no_progress_turns = 0;
        } else {
            self.consecutive_no_progress_turns =
                self.consecutive_no_progress_turns.saturating_add(1);
        }
        self.consecutive_no_progress_turns > MAX_CONSECUTIVE_NO_PROGRESS_TURNS
    }

    fn reset_after_evidence(&mut self) {
        *self = Self::default();
    }

    /// Marks the last task turn host-rejected; within the invalid-turn
    /// budget, returns the correction to send, else the failure.
    fn reject(
        &mut self,
        log: &mut CallLog,
        transcript: &TranscriptRecorder,
        title: &str,
        errors: &[String],
        made_progress: bool,
    ) -> Result<NativeConversationMessage, ReviewServiceError> {
        let validation_error = errors.join("; ");
        mark_last_call_host_rejected(log, transcript, &validation_error, !made_progress);
        if self.record_invalid(made_progress) {
            return Err(failed(validation_error));
        }
        Ok(NativeConversationMessage::UserCorrection {
            content_markdown: render_task_correction(title, errors),
        })
    }
}

#[cfg(test)]
mod task_turn_budget_tests {
    use super::*;

    #[test]
    fn response_schema_offers_narrow_only_for_recommendation_objections() {
        let base = crate::CaseTool::RecordObjectionResponse.parameters();
        let mut claim_only = base.clone();
        restrict_response_actions(&mut claim_only, false);
        assert!(!claim_only["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("narrow")));
        assert!(claim_only["properties"]
            .get("replacement_recommendation_ref")
            .is_none());
        let mut with_recommendation = base.clone();
        restrict_response_actions(&mut with_recommendation, true);
        assert_eq!(with_recommendation, base);
    }

    #[test]
    fn valid_progress_does_not_consume_invalid_or_no_progress_budget() {
        let mut budget = TaskTurnBudget::default();
        for _ in 0..12 {
            assert!(!budget.record_incomplete(true));
        }
        assert_eq!(budget.followup_turns, 12);
        assert_eq!(budget.consecutive_invalid_turns, 0);
        assert_eq!(budget.consecutive_no_progress_turns, 0);
    }

    #[test]
    fn invalid_and_no_progress_limits_are_independent() {
        let mut budget = TaskTurnBudget::default();
        for _ in 0..MAX_HOST_INVALID_ATTEMPTS {
            assert!(!budget.record_invalid(false));
        }
        assert!(budget.record_invalid(false));

        let mut budget = TaskTurnBudget::default();
        for _ in 0..MAX_HOST_INVALID_ATTEMPTS {
            assert!(!budget.record_invalid(false));
        }
        assert!(!budget.record_invalid(true));
        assert_eq!(budget.consecutive_invalid_turns, 0);

        let mut budget = TaskTurnBudget::default();
        for _ in 0..MAX_CONSECUTIVE_NO_PROGRESS_TURNS {
            assert!(!budget.record_incomplete(false));
        }
        assert!(budget.record_incomplete(false));
        assert!(!budget.record_incomplete(true));
    }
}

pub(super) fn complete_without_provider(
    planned: &ReviewPlanCase,
) -> Result<CompletedCase, ReviewServiceError> {
    let finalization = if let Some(closure) = &planned.routing.closure {
        FinalizedCase::failed(&planned.case, closure.clone())
    } else if planned.routing.selection.mode == DeliberationMode::Deterministic {
        FinalizedCase::deterministic(
            &planned.case,
            deterministic_disposition(&planned.routing.selection.reasons)?,
        )
    } else {
        return Err(failed(format!(
            "case {} requires an unavailable execution owner",
            planned.case.case_id
        )));
    }
    .map_err(|error| failed(error.to_string()))?;
    Ok(CompletedCase {
        packet: planned.evidence_packet.clone(),
        packet_history: Vec::new(),
        definitions: planned.probe_definitions.clone(),
        results: planned.deterministic_probe_results.clone(),
        review: None,
        ensemble_run: None,
        ensemble_failure: None,
        ensemble_evidence_packets: Vec::new(),
        ensemble_challenges: Vec::new(),
        challenge: None,
        finalization,
        calls: Vec::new(),
        egress: Vec::new(),
        claim_slot_dispositions: Vec::new(),
        recommendation_family_dispositions: Vec::new(),
        challenge_target_dispositions: Vec::new(),
        semantic_revisions: Vec::new(),
        projection_audits: Vec::new(),
    })
}

fn deterministic_disposition(
    reasons: &[ModeReason],
) -> Result<DeterministicDisposition, ReviewServiceError> {
    if reasons.contains(&ModeReason::NotSubstantiated) {
        Ok(DeterministicDisposition::NotSubstantiated)
    } else if reasons.contains(&ModeReason::EvidenceUnavailable) {
        Ok(DeterministicDisposition::EvidenceUnavailable)
    } else if reasons.contains(&ModeReason::ActionFullyDetermined) {
        Ok(DeterministicDisposition::ActionFullyDetermined)
    } else {
        Err(failed(
            "deterministic route lacks one auditable disposition reason",
        ))
    }
}

fn challenge_task_ledger(
    review: &ReviewerCaseReview,
    target_filter: Option<&BTreeSet<StableId>>,
) -> ChallengeTaskLedger {
    let targets = review
        .claims
        .iter()
        .map(|claim| (claim.claim_id.to_string(), "claim".to_string()))
        .chain(review.recommendations.iter().map(|recommendation| {
            (
                recommendation.recommendation_id.to_string(),
                "recommendation".to_string(),
            )
        }))
        .filter(|(target_id, _)| {
            target_filter.is_none_or(|filter| {
                StableId::parse(target_id).is_ok_and(|target_id| filter.contains(&target_id))
            })
        })
        .collect();
    ChallengeTaskLedger::new(ChallengeTaskRequirements { targets })
}

fn replace_challenge_identity_epoch(
    conversation: &mut Vec<NativeConversationMessage>,
    replacement_ledger: ChallengeTaskLedger,
) -> ChallengeTaskLedger {
    // Evidence expansion can change content-derived claim and recommendation IDs when the
    // reviewer is rerun. The immutable transcript retains the old epoch, but replaying its
    // assistant calls into the new provider request would expose stale targets alongside the
    // replacement review and tool schemas.
    conversation.clear();
    replacement_ledger
}

fn response_task_parameter_overrides(
    challenge: &crate::ChallengeRecord,
    review: &ReviewerCaseReview,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> BTreeMap<String, Value> {
    let bound =
        crate::bound_challenge_response_payload_schema(challenge, review, allowed_probe_ids);
    let branches = bound["properties"]["responses"]["items"]["oneOf"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut replacement_schema = None;
    for mut branch in branches {
        let embedded_replacement = branch["properties"]
            .as_object_mut()
            .and_then(|properties| properties.remove("replacement_recommendation"))
            .unwrap_or_else(|| json!({"type": "null"}));
        if embedded_replacement["type"] != "null" {
            replacement_schema.get_or_insert(embedded_replacement);
        }
    }

    let objection_ids = challenge
        .objections
        .iter()
        .map(|objection| objection.objection_id.to_string())
        .collect::<Vec<_>>();
    let mut response_schema = crate::CaseTool::RecordObjectionResponse.parameters();
    response_schema["properties"]["objection_id"] = json!({"enum": objection_ids});
    response_schema["properties"]["evidence_probe_ids"]["items"] =
        json!({"enum": allowed_probe_ids});
    restrict_response_actions(
        &mut response_schema,
        challenge
            .objections
            .iter()
            .any(|objection| objection.target_type == crate::ChallengeTargetType::Recommendation),
    );
    let mut overrides = BTreeMap::from([(
        crate::TOOL_RECORD_OBJECTION_RESPONSE.to_string(),
        response_schema.clone(),
    )]);
    if let Some(mut replacement) = replacement_schema {
        replacement["properties"]["recommendation_ref"] = json!({
            "pattern": "^[a-z][a-z0-9_-]{0,63}$",
            "type": "string"
        });
        if let Some(required) = replacement["required"].as_array_mut() {
            required.push(Value::String("recommendation_ref".to_string()));
        }
        overrides.insert(
            crate::TOOL_RECORD_REPLACEMENT_RECOMMENDATION.to_string(),
            replacement.clone(),
        );
        overrides.insert(
            crate::TOOL_REVISE_REPLACEMENT_RECOMMENDATION.to_string(),
            bound_revision_schema(
                "recommendation_ref",
                json!({
                    "pattern": "^[a-z][a-z0-9_-]{0,63}$",
                    "type": "string"
                }),
                replacement,
            ),
        );
    }
    overrides.insert(
        crate::TOOL_REVISE_OBJECTION_RESPONSE.to_string(),
        bound_revision_schema(
            "objection_id",
            json!({"enum": objection_ids}),
            response_schema,
        ),
    );
    overrides
}

/// Admission accepts `narrow`, and the replacement it carries, only for a
/// recommendation-target objection; without one, the response schema offers
/// neither, so it admits exactly what the host will.
fn restrict_response_actions(schema: &mut Value, recommendation_target: bool) {
    if recommendation_target {
        return;
    }
    schema["properties"]["action"] = json!({"enum": [
        "concede", "maintain_with_evidence", "withdraw", "mark_unresolved"
    ]});
    if let Some(properties) = schema["properties"].as_object_mut() {
        properties.remove("replacement_recommendation_ref");
    }
}

fn bound_revision_schema(
    reference_field: &str,
    reference_schema: Value,
    replacement_schema: Value,
) -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            reference_field: reference_schema,
            "expected_revision": {"type": "integer", "minimum": 1},
            "reason": {"type": "string", "minLength": 1},
            "replacement": replacement_schema
        },
        "required": [reference_field, "expected_revision", "reason", "replacement"],
        "type": "object"
    })
}

fn execute_logged_case_call(
    request: CaseCallRequest<'_>,
    runtime: CaseCallRuntime<'_>,
    log: &mut CallLog,
) -> Result<CaseCallOutput, ReviewServiceError> {
    match execute_case_call(request, runtime) {
        Ok(mut output) => {
            log.calls.append(&mut output.calls);
            log.egress.append(&mut output.egress);
            Ok(output)
        }
        Err(mut failure) => {
            log.calls.append(&mut failure.calls);
            log.egress.append(&mut failure.egress);
            Err(failure.error)
        }
    }
}

fn call_runtime<'a>(runtime: &'a CaseExecutionRuntime<'a>) -> CaseCallRuntime<'a> {
    CaseCallRuntime {
        routes: runtime.routes,
        qualifications: runtime.qualifications,
        ledger: runtime.ledger,
        transport: runtime.transport,
        consent: runtime.consent,
        control: runtime.control,
        transcript: runtime.transcript,
        transcript_enabled: runtime.transcript_enabled,
    }
}

fn resolved_results(results: &[crate::ProbeResult]) -> Vec<crate::ProbeResult> {
    results
        .iter()
        .filter(|result| result.resolver.kind != crate::ResolverKind::UnknownFallback)
        .cloned()
        .collect()
}

fn unresolved_definitions(
    definitions: &[crate::ProbeDefinition],
    resolved: &[crate::ProbeResult],
) -> Vec<crate::ProbeDefinition> {
    let ids = resolved
        .iter()
        .map(|result| &result.probe_id)
        .collect::<BTreeSet<_>>();
    definitions
        .iter()
        .filter(|definition| !ids.contains(&definition.probe_id))
        .cloned()
        .collect()
}

fn merge_probe_results(
    deterministic: &[crate::ProbeResult],
    model: &[crate::ProbeResult],
) -> Vec<crate::ProbeResult> {
    let model_ids = model
        .iter()
        .map(|result| &result.probe_id)
        .collect::<BTreeSet<_>>();
    let mut results = deterministic
        .iter()
        .filter(|result| !model_ids.contains(&result.probe_id))
        .cloned()
        .chain(model.iter().cloned())
        .collect::<Vec<_>>();
    results.sort_by(|left, right| left.probe_id.cmp(&right.probe_id));
    results
}

fn probe_ids(definitions: &[crate::ProbeDefinition]) -> BTreeSet<StableId> {
    definitions
        .iter()
        .map(|definition| definition.probe_id.clone())
        .collect()
}

fn reconsideration_instruction(prompts: &ReviewPromptBundleV2, role: ReviewRole) -> String {
    let role_instruction = if role == ReviewRole::Challenger {
        prompts.challenger_instruction.as_str()
    } else {
        prompts.reviewer_instruction.as_str()
    };
    [
        prompts.core_instruction.as_str(),
        prompts.shared_evidence_reconsideration_instruction.as_str(),
        role_instruction,
        prompts.bounded_response_instruction.as_str(),
    ]
    .join("\n\n")
}

fn requires_mandatory_challenge(recommendation: &ContextualRecommendation) -> bool {
    recommendation.family == RecommendationFamily::LifecycleReownership
}

fn reviewer_review_context(review: &ReviewerCaseReview) -> Value {
    json!({
        "case_conclusion": review.case_conclusion,
        "case_position": review.case_position,
        "model_resolved_probes": review.model_resolved_probes.iter().map(|result| json!({
            "probe_id": result.probe_id,
            "answer": result.answer,
            "evidence_ids": result.evidence_ids,
            "explanation": result.explanation,
            "limitations": result.limitations,
        })).collect::<Vec<_>>(),
        "claims": review.claims.iter().map(|claim| json!({
            "claim_id": claim.claim_id,
            "content": claim.content,
        })).collect::<Vec<_>>(),
        "recommendations": review.recommendations.iter().map(|recommendation| json!({
            "recommendation_id": recommendation.recommendation_id,
            "content": recommendation.content,
        })).collect::<Vec<_>>(),
        "preferred_recommendation_id": review.preferred_recommendation_id,
        "unresolved_gaps": review.unresolved_gaps,
        "proposed_closure": review.proposed_closure.reason,
    })
}

fn challenge_context(challenge: &crate::ChallengeRecord) -> Value {
    json!({
        "objections": challenge.objections,
        "common_mode_concerns": challenge.common_mode_concerns,
    })
}

fn run_id(prefix: &str, case_id: &StableId) -> Result<StableId, ReviewServiceError> {
    StableId::from_identity(prefix, 1, &json!({"case_id": case_id}))
        .map_err(|error| failed(error.to_string()))
}

#[cfg(test)]
mod failure_classification_tests {
    use super::*;

    #[test]
    fn challenge_rereview_replaces_stale_identity_epoch() {
        let mut conversation = vec![NativeConversationMessage::Assistant {
            text: Some("Disposition the old review target.".to_string()),
            tool_calls: vec![NativeToolCall::new(
                "call-old",
                crate::TOOL_RECORD_CASE_OBJECTION,
                json!({
                    "objection_ref": "old-objection",
                    "target_type": "claim",
                    "target_id": "claim-old",
                    "reason": "unsupported_assumption"
                }),
                r#"{"objection_ref":"old-objection","target_type":"claim","target_id":"claim-old","reason":"unsupported_assumption"}"#,
            )],
        }];
        let replacement = ChallengeTaskLedger::new(ChallengeTaskRequirements {
            targets: [("claim-new".to_string(), "claim".to_string())]
                .into_iter()
                .collect(),
        });

        let mut rebound = replace_challenge_identity_epoch(&mut conversation, replacement);

        assert!(conversation.is_empty());
        assert!(rebound
            .record_objection(
                "stale-objection",
                "claim",
                "claim-old",
                json!({"target_id": "claim-old"}),
                crate::TaskCallProvenance::new("request-old", "call-old"),
            )
            .is_err());
        rebound
            .complete()
            .expect("new target is implicitly cleared");
        assert_eq!(
            rebound.disposition_records(
                &StableId::from_identity("reviewer-case-review", 1, &json!({"test": true}))
                    .unwrap(),
                &StableId::from_identity("challenger-run", 1, &json!({"test": true})).unwrap(),
            )[0]
            .disposition,
            crate::ChallengeTargetDisposition::Cleared
        );
    }

    #[test]
    fn provider_output_failures_are_distinct_from_route_failures() {
        assert_eq!(
            classify_case_failure(
                "provider invocation failed",
                &[crate::ProviderFailureClass::InvalidOutput],
            ),
            CaseFailure::ModelOutputInvalid
        );
        assert_eq!(
            classify_case_failure(
                "provider invocation failed",
                &[crate::ProviderFailureClass::Timeout],
            ),
            CaseFailure::ModelRouteUnavailable
        );
    }

    #[test]
    fn challenge_failure_remains_the_primary_closure_reason() {
        assert_eq!(
            classify_case_failure(
                "challenge response was invalid",
                &[crate::ProviderFailureClass::InvalidOutput],
            ),
            CaseFailure::ChallengeUnresolved
        );
    }

    #[test]
    fn host_provider_budget_exhaustion_is_not_attributed_to_model_output() {
        assert_eq!(
            classify_case_failure(
                "case provider budget exhausted",
                &[crate::ProviderFailureClass::InvalidOutput],
            ),
            CaseFailure::ProviderBudgetExhausted
        );
        assert_eq!(
            classify_case_failure("review-wide provider budget exhausted", &[]),
            CaseFailure::ProviderBudgetExhausted
        );
    }

    #[test]
    fn invalid_provider_citation_is_not_misclassified_as_packet_corruption() {
        assert_eq!(
            classify_case_failure(
                "evidence citation `source-span:outside-packet` is absent from the packet",
                &[crate::ProviderFailureClass::InvalidOutput],
            ),
            CaseFailure::ModelOutputInvalid
        );
    }

    #[test]
    fn recommendation_absence_is_reconstructed_only_at_the_host_boundary() {
        let wire = json!({
            "recommendation_ref": "move-work",
            "alternatives": [{"summary": "retain", "tradeoffs": ["keeps repeated work"]}]
        });

        let reconstructed = task_support::reconstruct_recommendation_absence(&wire)
            .expect("recommendation arguments reconstruct");

        assert!(wire.get("no_alternative_reason").is_none());
        assert_eq!(reconstructed["no_alternative_reason"], Value::Null);
    }
}

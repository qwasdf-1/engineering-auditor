use super::*;

type FailureRule = for<'a> fn(&FailureContext<'a>) -> bool;

struct FailureContext<'a> {
    normalized_message: String,
    provider_failures: &'a [crate::ProviderFailureClass],
}

pub(super) fn classify_case_failure(
    message: &str,
    provider_failures: &[crate::ProviderFailureClass],
) -> CaseFailure {
    let context = FailureContext {
        normalized_message: message.to_ascii_lowercase(),
        provider_failures,
    };
    ordered_rules()
        .into_iter()
        .find_map(|(failure, matches)| matches(&context).then_some(failure))
        .unwrap_or(CaseFailure::ModelOutputInvalid)
}

fn ordered_rules() -> [(CaseFailure, FailureRule); 6] {
    [
        (CaseFailure::LookupBudgetExhausted, lookup_budget),
        (CaseFailure::ProviderBudgetExhausted, provider_budget),
        (CaseFailure::ChallengeUnresolved, challenge_unresolved),
        // This is a host-observed output/admission class, not a provider-quality verdict.
        // It must precede broad message heuristics so invalid citations and tool output are
        // not mislabeled as packet corruption or route unavailability.
        (CaseFailure::ModelOutputInvalid, invalid_provider_output),
        (CaseFailure::EvidencePacketInvalid, evidence_packet),
        (CaseFailure::ModelRouteUnavailable, route_unavailable),
    ]
}

fn lookup_budget(context: &FailureContext<'_>) -> bool {
    context.normalized_message.contains("lookup") && context.normalized_message.contains("budget")
}

fn provider_budget(context: &FailureContext<'_>) -> bool {
    context
        .normalized_message
        .contains("provider budget exhausted")
}

fn challenge_unresolved(context: &FailureContext<'_>) -> bool {
    context.normalized_message.contains("challenge")
}

fn evidence_packet(context: &FailureContext<'_>) -> bool {
    context.normalized_message.contains("evidence") && context.normalized_message.contains("packet")
}

fn route_unavailable(context: &FailureContext<'_>) -> bool {
    ["no qualified case target", "route", "provider"]
        .iter()
        .any(|needle| context.normalized_message.contains(needle))
}

fn invalid_provider_output(context: &FailureContext<'_>) -> bool {
    context.provider_failures.iter().any(|failure| {
        matches!(
            failure,
            crate::ProviderFailureClass::ContextOverflow
                | crate::ProviderFailureClass::Refusal
                | crate::ProviderFailureClass::Truncated
                | crate::ProviderFailureClass::InvalidOutput
        )
    })
}

const fn case_failure_detail(failure: CaseFailure) -> &'static str {
    match failure {
        CaseFailure::ModelRouteUnavailable => "model_route_unavailable",
        CaseFailure::ModelOutputInvalid => "model_output_invalid",
        CaseFailure::ProviderBudgetExhausted => "provider_budget_exhausted",
        CaseFailure::LookupBudgetExhausted => "lookup_budget_exhausted",
        CaseFailure::EvidencePacketInvalid => "evidence_packet_invalid",
        CaseFailure::ChallengeUnresolved => "challenge_unresolved",
        CaseFailure::OperatorInputRequired => "operator_input_required",
        CaseFailure::PrivacyPolicyBlocked => "privacy_policy_blocked",
        CaseFailure::ModeUnavailable => "mode_unavailable",
    }
}

pub(super) fn case_failure_detail_codes(
    failure: CaseFailure,
    message: &str,
    provider_failures: &[crate::ProviderFailureClass],
) -> Vec<String> {
    let mut details = vec![case_failure_detail(failure).to_string()];
    for (needle, code) in detail_code_rules() {
        if message.contains(needle) {
            details.push(code.to_string());
        }
    }
    details.extend(
        provider_failures
            .iter()
            .copied()
            .map(provider_failure_detail),
    );
    details.sort();
    details.dedup();
    details
}

fn detail_code_rules() -> [(&'static str, &'static str); 14] {
    [
        ("InvalidPayload", "invalid_payload"),
        ("stable id must use", "invalid_stable_id"),
        ("InvalidProbeSet", "invalid_probe_set"),
        ("InvalidModelProbe", "invalid_model_probe"),
        ("UnresolvedMandatoryProbe", "unresolved_mandatory_probe"),
        ("InvalidClaims", "invalid_claims"),
        ("InvalidRecommendations", "invalid_recommendations"),
        (
            "InvalidPreferredRecommendation",
            "invalid_preferred_recommendation",
        ),
        ("InvalidClosure", "invalid_closure"),
        ("OutOfCaseEvidenceRequest", "out_of_case_evidence_request"),
        ("EvidenceRoundExhausted", "evidence_round_exhausted"),
        ("EvidenceNotRecompiled", "evidence_not_recompiled"),
        ("InvalidObjection", "invalid_objection"),
        ("InvalidResponse", "invalid_response"),
    ]
}

fn provider_failure_detail(failure: crate::ProviderFailureClass) -> String {
    let value = serde_json::to_value(failure)
        .expect("provider failure classes always serialize as stable strings");
    format!(
        "provider_{}",
        value
            .as_str()
            .expect("provider failure classes serialize as strings")
    )
}

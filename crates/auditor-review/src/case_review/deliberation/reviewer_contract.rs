use std::collections::BTreeMap;

use crate::{EngineeringClaimKind, RecommendationFamily};

use super::reviewer::ProviderRecommendationFamily;

pub(super) const fn lookup_instruction() -> &'static str {
    "If a Lookup authority field is present, request_case_evidence may use only its listed targets: use a finding-source target for finding_source or a symbol-neighborhood target for symbol_neighborhood. If that field is absent, the supplied evidence is already complete for every authorized lookup target."
}

pub(super) const fn probe_evidence_instruction() -> &'static str {
    "Call answer_case_probe exactly once for every question listed under Model probe contracts, and never answer a Host-resolved probe. Evaluate probes independently: the same concrete fact may establish boundaries for more than one probe, so never answer no merely because another probe also covers that fact. For every probe, answer yes only when evidence establishes positive_boundary, and answer no only when evidence establishes negative_boundary; the boundary text is authoritative. Failure to establish the positive boundary is never evidence for the negative boundary. If the evidence, explanation, or limitations leave the relevant fact as may-or-may-not, conditional, or unconfirmed, answer unknown rather than yes or no. A question's admissible evidence IDs are alternatives, not a conjunctive checklist: cite only listed IDs that actually establish the selected boundary. If its list is empty or establishes neither boundary, answer unknown with no evidence IDs. A function or type signature establishes capability, not runtime behavior: FnMut(&mut T), mutable access, or invocation of an opaque callable does not prove that mutation occurs, values differ, a consumer requires fresh state, or retention is risky. Without the callable body, an explicit behavioral contract, or runtime evidence, those facts remain unknown. Do not substitute a semantically related item from another evidence class."
}

pub(super) const fn provider_recommendation_family(
    family: RecommendationFamily,
) -> ProviderRecommendationFamily {
    let use_when = match family {
        RecommendationFamily::LifecycleReownership => {
            "Use only for lifecycle_mismatch_substantiated: the current evidence establishes invariant repeated work at a concrete enclosing owner through equivalent inputs, no relevant intervening mutation or consumer distinction, feasible invalidation, and acceptable retention. Move that invariant work or reusable result to the owner. Do not use this family to propose a redesign that would make a currently invalid hoist safe."
        }
        RecommendationFamily::ExplicitRetentionAvoidance => {
            "Use only for repetition_justified or wider_ownership_infeasible. Keep recomputation or ownership local because retaining or hoisting the result is unsafe, infeasible, or intentionally avoided; do not use this family for a safe hoist to an identified enclosing owner."
        }
        RecommendationFamily::MeasurementBeforeChange => {
            "Use only for insufficient_evidence. Defer structural action until a named measurement or inspection resolves the evidence gap; do not substitute this family when the structural conclusion is already established but its numeric benefit is unknown."
        }
        RecommendationFamily::SharedRootRemediation => {
            "Use only for shared_root_cause or partial_overlap. Describe the bounded remediation shared by the causally overlapping findings, preserve any independently actionable remainder, and do not infer common cause from co-location or correlation alone."
        }
        RecommendationFamily::PreserveIndependentActions => {
            "Use only when findings or recommendations must remain independently actionable. Record their contextual relationship without deleting, merging, or weakening detector-local advice."
        }
        RecommendationFamily::ConsolidatedRemediation => {
            "Use only for consolidated_action. Combine compatible actions at the same abstraction level without adding a new action family or worsening an evidenced property."
        }
        RecommendationFamily::SequencedRemediation => {
            "Use only for sequenced_actions. Preserve each admitted action and state the evidence-backed prerequisite order."
        }
        RecommendationFamily::NarrowedRemediation => {
            "Use only for narrowed_action. Retain the necessary action after an evidenced supersession relationship makes another action unnecessary."
        }
        RecommendationFamily::DeferredRemediation => {
            "Use only for deferred_action. Name the evidence-backed condition or operator decision required before the otherwise contextual action should proceed."
        }
    };
    ProviderRecommendationFamily { family, use_when }
}

pub(super) const fn common_output_admission_instruction() -> &'static str {
    "Native task-call admission rules:\n\
     - The host derives the typed case conclusion, case position, challenge status, closure, and persistent identities from admitted task records. Do not attempt to supply those outcomes.\n\
     - Use create tools only for new local references. Use an offered revise tool with the current revision and a complete replacement when changing an admitted record.\n\
     - Omit an optional argument when it has no value. Never send null as a native tool argument."
}

pub(super) const fn reviewer_output_admission_instruction() -> &'static str {
    "Reviewer admission rules:\n\
     - supporting_probe_ids, counter_probe_ids, and evidence_probe_ids accept case-probe IDs only. Evidence item IDs belong only in answer_case_probe.evidence_ids.\n\
     - Claim dependencies must use earlier local claim_ref values and form an acyclic graph. A root claim needs at least one supporting_probe_id; a derived claim may use an empty supporting_probe_ids array when one or more dependency_claim_refs fully supply its support. Do not duplicate dependency probe IDs onto the derived claim. Choose kind from decision.claim_kind_contracts according to its definition; kind classifies the proposition and is separate from slot, so never copy a slot name into kind. Claim kind does not constrain dependency direction.\n\
     - EA derives case polarity from the final admitted probe answers using the registered case-template policy. Do not restate case polarity as a free-form claim. Revising a probe answer automatically revises the derived conclusion.\n\
     - Recommendation families follow that host-derived conclusion exactly. Use each offered family's use_when contract; do not infer the mapping from the enum name.\n\
     - Claim status has deterministic precedence: contested (conflicting support, a yes counter-probe, or a contested dependency), then not_substantiated (not_applicable support or a not_substantiated dependency), then blocked (unknown support/counterprobe or a blocked dependency), then substantiated, then partially_substantiated. Yes and no are both definitive when the named probe is placed in supporting_probe_ids because probe boundaries define their meaning.\n\
     - Recommendation premise_claim_refs may name any claim that is neither blocked nor not_substantiated. A measurement_before_change recommendation may instead cite a blocked premise when that claim identifies the evidence gap the recommendation will resolve. Other recommendation families cannot cite blocked premises. Choose scope_ids only from decision.recommendation_scope_ids.\n\
     - Choose recommendation family from decision.recommendation_families using its use_when definition; the enum name alone is not the contract.\n\
     - Submit a complete recommendation with record_case_recommendation. To replace an admitted recommendation, call revise_case_recommendation with its current revision and the complete replacement. Do not repeat record_case_recommendation for the same recommendation_ref.\n\
     - Omit no_alternative_reason when alternatives is non-empty. When alternatives is empty, supply one prose string explaining why; it is never an object or a recommendation-family disposition.\n\
     - For structural_inference, qualitative_only, or unknown benefits, omit numeric_value, unit, and extrapolation_model. direct_measurement requires numeric_value and unit. bounded_extrapolation additionally requires extrapolation_model."
}

pub(super) const fn challenger_output_admission_instruction() -> &'static str {
    "Challenger admission rules:\n\
     - Objection evidence_ids accept only closed-packet evidence item IDs or case-probe IDs cited by the admitted reviewer result.\n\
     - target_id names the admitted claim or recommendation being challenged; it is not an evidence citation.\n\
     - Record only active objections. When complete_case_challenge is admitted, the host clears every target without an active objection."
}

pub(super) fn provider_claim_kinds() -> BTreeMap<EngineeringClaimKind, &'static str> {
    BTreeMap::from([
        (
            EngineeringClaimKind::RootCause,
            "Observed causal condition or defect mechanism that adds synthesis beyond the probe answers.",
        ),
        (
            EngineeringClaimKind::Actionability,
            "Whether and why a bounded change is feasible, safe, or blocked.",
        ),
        (
            EngineeringClaimKind::RecommendationPremise,
            "Condition specific to a recommendation and not already supplied by an earlier admitted claim.",
        ),
        (
            EngineeringClaimKind::ExpectedBenefit,
            "Evidence-backed beneficial outcome expected from an action; not a structured recommendation benefit.",
        ),
    ])
}

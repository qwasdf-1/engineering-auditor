use std::collections::BTreeMap;

use crate::{CaseConclusion, CaseKind, ProbeAnswer};

/// Derive the lifecycle case conclusion from the admitted probe-answer ledger.
///
/// The order is intentional and is the complete polarity policy for the enabled
/// lifecycle template. Probe revisions therefore revise the derived conclusion
/// without creating a second, independently authored semantic record.
pub fn derive_case_conclusion(
    case_kind: CaseKind,
    answers_by_slot: &BTreeMap<String, ProbeAnswer>,
) -> Result<CaseConclusion, String> {
    match case_kind {
        CaseKind::LifecycleMismatch => derive_lifecycle(answers_by_slot),
        CaseKind::SharedRootCause => derive_shared_root(answers_by_slot),
        CaseKind::RecommendationConsolidation => derive_consolidation(answers_by_slot),
    }
}

/// Model-visible summary of the host-owned derivation policy. This lives next
/// to the executable policy so the provider projection does not maintain a
/// parallel account of which probes can change the typed conclusion.
pub const fn conclusion_derivation_policy(case_kind: CaseKind) -> &'static str {
    match case_kind {
        CaseKind::LifecycleMismatch => {
            "EA derives lifecycle conclusions from repetition, input_equivalence, intervening_mutation, consumer_distinction, wider_owner, invalidation_feasibility, and retention_risk. material_cost does not participate in this conclusion derivation: when the structural lifecycle predicates establish invariant reusable work, the result is lifecycle_mismatch_substantiated regardless of the material_cost answer. material_cost may support a material-impact claim or benefit calibration, and unknown cost must not be presented as a measured benefit."
        }
        CaseKind::SharedRootCause => {
            "EA derives the shared-root conclusion from responsibility identity, shared-change impact, correlation, recommendation conflict, coupling increase, distinct semantics, and independent actionability. Co-location or correlation alone never establishes a shared root, and independently actionable findings remain explicit."
        }
        CaseKind::RecommendationConsolidation => {
            "EA derives the consolidation conclusion from simultaneous compatibility, abstraction alignment, supersession, measured-property regression, required ordering, proportionality, and deferral appropriateness. It preserves independent actions when compatibility or abstraction alignment fails and never deletes detector-local advice."
        }
    }
}

fn derive_lifecycle(
    answers_by_slot: &BTreeMap<String, ProbeAnswer>,
) -> Result<CaseConclusion, String> {
    let answer = |slot: &str| {
        answers_by_slot
            .get(slot)
            .copied()
            .ok_or_else(|| format!("case-conclusion derivation is missing probe slot `{slot}`"))
    };
    let repetition = answer("repetition")?;
    let input_equivalence = answer("input_equivalence")?;
    let intervening_mutation = answer("intervening_mutation")?;
    let consumer_distinction = answer("consumer_distinction")?;
    let wider_owner = answer("wider_owner")?;
    let invalidation_feasibility = answer("invalidation_feasibility")?;
    let retention_risk = answer("retention_risk")?;

    if matches!(repetition, ProbeAnswer::No | ProbeAnswer::NotApplicable) {
        return Ok(CaseConclusion::ImpactImmaterial);
    }
    if intervening_mutation == ProbeAnswer::Yes
        || input_equivalence == ProbeAnswer::No
        || consumer_distinction == ProbeAnswer::Yes
    {
        return Ok(CaseConclusion::RepetitionJustified);
    }
    if retention_risk == ProbeAnswer::Yes
        || matches!(wider_owner, ProbeAnswer::No | ProbeAnswer::NotApplicable)
        || matches!(
            invalidation_feasibility,
            ProbeAnswer::No | ProbeAnswer::NotApplicable
        )
    {
        return Ok(CaseConclusion::WiderOwnershipInfeasible);
    }
    if repetition == ProbeAnswer::Yes
        && input_equivalence == ProbeAnswer::Yes
        && matches!(
            intervening_mutation,
            ProbeAnswer::No | ProbeAnswer::NotApplicable
        )
        && matches!(
            consumer_distinction,
            ProbeAnswer::No | ProbeAnswer::NotApplicable
        )
    {
        return Ok(CaseConclusion::LifecycleMismatchSubstantiated);
    }
    Ok(CaseConclusion::InsufficientEvidence)
}

fn required_answer(
    answers: &BTreeMap<String, ProbeAnswer>,
    slot: &str,
) -> Result<ProbeAnswer, String> {
    answers
        .get(slot)
        .copied()
        .ok_or_else(|| format!("case-conclusion derivation is missing probe slot `{slot}`"))
}

fn any_uncertain(values: &[ProbeAnswer]) -> bool {
    values.iter().any(|answer| {
        matches!(
            answer,
            ProbeAnswer::Unknown | ProbeAnswer::ConflictingEvidence
        )
    })
}

fn derive_shared_root(answers: &BTreeMap<String, ProbeAnswer>) -> Result<CaseConclusion, String> {
    let same = required_answer(answers, "same_responsibility")?;
    let shared_change = required_answer(answers, "shared_change_impact")?;
    let correlation = required_answer(answers, "mere_correlation")?;
    let conflict = required_answer(answers, "recommendation_conflict")?;
    let coupling = required_answer(answers, "coupling_increase")?;
    let distinct = required_answer(answers, "distinct_semantics")?;
    let independent = required_answer(answers, "independent_actionability")?;
    if any_uncertain(&[
        same,
        shared_change,
        correlation,
        conflict,
        coupling,
        distinct,
        independent,
    ]) {
        return Ok(CaseConclusion::InsufficientEvidence);
    }
    if correlation == ProbeAnswer::Yes && same != ProbeAnswer::Yes {
        return Ok(CaseConclusion::CorrelationOnly);
    }
    if distinct == ProbeAnswer::Yes || same != ProbeAnswer::Yes || shared_change != ProbeAnswer::Yes
    {
        return Ok(CaseConclusion::IndependentFindings);
    }
    if independent == ProbeAnswer::Yes
        || conflict == ProbeAnswer::Yes
        || coupling == ProbeAnswer::Yes
    {
        return Ok(CaseConclusion::PartialOverlap);
    }
    Ok(CaseConclusion::SharedRootCause)
}

fn derive_consolidation(answers: &BTreeMap<String, ProbeAnswer>) -> Result<CaseConclusion, String> {
    let compatible = required_answer(answers, "simultaneous_compatibility")?;
    let abstraction = required_answer(answers, "abstraction_alignment")?;
    let supersession = required_answer(answers, "supersession")?;
    let regression = required_answer(answers, "measured_property_regression")?;
    let ordering = required_answer(answers, "required_ordering")?;
    let proportionate = required_answer(answers, "proportionality")?;
    let deferral = required_answer(answers, "deferral_appropriateness")?;
    if any_uncertain(&[
        compatible,
        abstraction,
        supersession,
        regression,
        ordering,
        proportionate,
        deferral,
    ]) {
        return Ok(CaseConclusion::NoRepositorySpecificAction);
    }
    if compatible != ProbeAnswer::Yes
        || abstraction != ProbeAnswer::Yes
        || regression == ProbeAnswer::Yes
    {
        return Ok(CaseConclusion::PreservedIndependentActions);
    }
    if deferral == ProbeAnswer::Yes || proportionate != ProbeAnswer::Yes {
        return Ok(CaseConclusion::DeferredAction);
    }
    if supersession == ProbeAnswer::Yes {
        return Ok(CaseConclusion::NarrowedAction);
    }
    if ordering == ProbeAnswer::Yes {
        return Ok(CaseConclusion::SequencedActions);
    }
    Ok(CaseConclusion::ConsolidatedAction)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> BTreeMap<String, ProbeAnswer> {
        BTreeMap::from([
            ("repetition".to_string(), ProbeAnswer::Yes),
            ("input_equivalence".to_string(), ProbeAnswer::Yes),
            ("intervening_mutation".to_string(), ProbeAnswer::No),
            ("consumer_distinction".to_string(), ProbeAnswer::No),
            ("wider_owner".to_string(), ProbeAnswer::Yes),
            ("invalidation_feasibility".to_string(), ProbeAnswer::Yes),
            ("retention_risk".to_string(), ProbeAnswer::No),
        ])
    }

    #[test]
    fn lifecycle_conclusion_precedence_is_explicit() {
        let positive = baseline();
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &positive).unwrap(),
            CaseConclusion::LifecycleMismatchSubstantiated
        );

        let mut mutation = baseline();
        mutation.insert("intervening_mutation".to_string(), ProbeAnswer::Yes);
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &mutation).unwrap(),
            CaseConclusion::RepetitionJustified
        );

        let mut constrained = baseline();
        constrained.insert("retention_risk".to_string(), ProbeAnswer::Yes);
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &constrained).unwrap(),
            CaseConclusion::WiderOwnershipInfeasible
        );

        let mut mutated_and_constrained = constrained.clone();
        mutated_and_constrained.insert("intervening_mutation".to_string(), ProbeAnswer::Yes);
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &mutated_and_constrained).unwrap(),
            CaseConclusion::RepetitionJustified
        );

        let mut test_only = constrained;
        test_only.insert("repetition".to_string(), ProbeAnswer::NotApplicable);
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &test_only).unwrap(),
            CaseConclusion::ImpactImmaterial
        );

        let mut unknown = baseline();
        unknown.insert("input_equivalence".to_string(), ProbeAnswer::Unknown);
        assert_eq!(
            derive_case_conclusion(CaseKind::LifecycleMismatch, &unknown).unwrap(),
            CaseConclusion::InsufficientEvidence
        );
    }

    fn answers(values: &[(&str, ProbeAnswer)]) -> BTreeMap<String, ProbeAnswer> {
        values
            .iter()
            .map(|(slot, answer)| ((*slot).to_string(), *answer))
            .collect()
    }

    #[test]
    fn shared_root_outcomes_preserve_causal_and_independent_boundaries() {
        let baseline = [
            ("same_responsibility", ProbeAnswer::Yes),
            ("shared_change_impact", ProbeAnswer::Yes),
            ("mere_correlation", ProbeAnswer::No),
            ("recommendation_conflict", ProbeAnswer::No),
            ("coupling_increase", ProbeAnswer::No),
            ("distinct_semantics", ProbeAnswer::No),
            ("independent_actionability", ProbeAnswer::No),
        ];
        assert_eq!(
            derive_case_conclusion(CaseKind::SharedRootCause, &answers(&baseline)).unwrap(),
            CaseConclusion::SharedRootCause
        );
        for (slot, expected) in [
            ("independent_actionability", CaseConclusion::PartialOverlap),
            ("distinct_semantics", CaseConclusion::IndependentFindings),
        ] {
            let mut changed = answers(&baseline);
            changed.insert(slot.to_string(), ProbeAnswer::Yes);
            assert_eq!(
                derive_case_conclusion(CaseKind::SharedRootCause, &changed).unwrap(),
                expected
            );
        }
        let mut correlated = answers(&baseline);
        correlated.insert("same_responsibility".to_string(), ProbeAnswer::No);
        correlated.insert("mere_correlation".to_string(), ProbeAnswer::Yes);
        assert_eq!(
            derive_case_conclusion(CaseKind::SharedRootCause, &correlated).unwrap(),
            CaseConclusion::CorrelationOnly
        );
        let mut unknown = answers(&baseline);
        unknown.insert("shared_change_impact".to_string(), ProbeAnswer::Unknown);
        assert_eq!(
            derive_case_conclusion(CaseKind::SharedRootCause, &unknown).unwrap(),
            CaseConclusion::InsufficientEvidence
        );
    }

    #[test]
    fn recommendation_consolidation_outcomes_are_non_heuristic() {
        let baseline = [
            ("simultaneous_compatibility", ProbeAnswer::Yes),
            ("abstraction_alignment", ProbeAnswer::Yes),
            ("supersession", ProbeAnswer::No),
            ("measured_property_regression", ProbeAnswer::No),
            ("required_ordering", ProbeAnswer::No),
            ("proportionality", ProbeAnswer::Yes),
            ("deferral_appropriateness", ProbeAnswer::No),
        ];
        assert_eq!(
            derive_case_conclusion(CaseKind::RecommendationConsolidation, &answers(&baseline))
                .unwrap(),
            CaseConclusion::ConsolidatedAction
        );
        for (slot, value, expected) in [
            (
                "required_ordering",
                ProbeAnswer::Yes,
                CaseConclusion::SequencedActions,
            ),
            (
                "simultaneous_compatibility",
                ProbeAnswer::No,
                CaseConclusion::PreservedIndependentActions,
            ),
            (
                "supersession",
                ProbeAnswer::Yes,
                CaseConclusion::NarrowedAction,
            ),
            (
                "deferral_appropriateness",
                ProbeAnswer::Yes,
                CaseConclusion::DeferredAction,
            ),
            (
                "abstraction_alignment",
                ProbeAnswer::Unknown,
                CaseConclusion::NoRepositorySpecificAction,
            ),
        ] {
            let mut changed = answers(&baseline);
            changed.insert(slot.to_string(), value);
            assert_eq!(
                derive_case_conclusion(CaseKind::RecommendationConsolidation, &changed).unwrap(),
                expected
            );
        }
    }
}

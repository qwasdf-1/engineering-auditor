use std::collections::{BTreeMap, BTreeSet};

use super::{
    BenefitEvidenceMode, ContextualRecommendation, CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};
use crate::{
    CaseConclusion, ClaimStatus, EngineeringCase, EngineeringClaim, RecommendationFamily, StableId,
};

pub(crate) fn conclusion_allows_recommendation(
    conclusion: CaseConclusion,
    family: RecommendationFamily,
) -> bool {
    required_recommendation_family(conclusion) == Some(family)
}

pub(crate) fn required_recommendation_family(
    conclusion: CaseConclusion,
) -> Option<RecommendationFamily> {
    const FAMILIES: [Option<RecommendationFamily>; 15] = [
        Some(RecommendationFamily::LifecycleReownership),
        Some(RecommendationFamily::ExplicitRetentionAvoidance),
        None,
        Some(RecommendationFamily::ExplicitRetentionAvoidance),
        Some(RecommendationFamily::MeasurementBeforeChange),
        Some(RecommendationFamily::SharedRootRemediation),
        Some(RecommendationFamily::SharedRootRemediation),
        Some(RecommendationFamily::PreserveIndependentActions),
        Some(RecommendationFamily::PreserveIndependentActions),
        Some(RecommendationFamily::ConsolidatedRemediation),
        Some(RecommendationFamily::SequencedRemediation),
        Some(RecommendationFamily::PreserveIndependentActions),
        Some(RecommendationFamily::NarrowedRemediation),
        Some(RecommendationFamily::DeferredRemediation),
        None,
    ];
    FAMILIES[conclusion as usize]
}

pub(crate) fn validate_recommendations_for_conclusion(
    conclusion: CaseConclusion,
    recommendations: &[ContextualRecommendation],
) -> Result<(), String> {
    let incompatible = recommendations
        .iter()
        .filter(|recommendation| {
            !conclusion_allows_recommendation(conclusion, recommendation.family)
        })
        .map(|recommendation| recommendation.recommendation_id.to_string())
        .collect::<Vec<_>>();
    if !incompatible.is_empty() {
        return Err(format!(
            "case conclusion {conclusion:?} cannot admit recommendation family for: {}",
            incompatible.join(", ")
        ));
    }
    let Some(required) = required_recommendation_family(conclusion) else {
        return Ok(());
    };
    recommendations
        .iter()
        .any(|recommendation| recommendation.family == required)
        .then_some(())
        .ok_or_else(|| {
            format!(
                "case conclusion {conclusion:?} requires at least one {required:?} recommendation"
            )
        })
}

pub(crate) fn validate_recommendations(
    case: &EngineeringCase,
    allowed_probe_ids: &BTreeSet<StableId>,
    claims: &[EngineeringClaim],
    recommendations: &[ContextualRecommendation],
) -> Result<(), String> {
    let context = RecommendationValidationContext::new(case, allowed_probe_ids, claims);
    let mut ids = BTreeSet::new();
    for recommendation in recommendations {
        let duplicate = !ids.insert(recommendation.recommendation_id.clone());
        validate_recommendation(recommendation, duplicate, &context)?;
    }
    Ok(())
}

struct RecommendationValidationContext<'a> {
    case: &'a EngineeringCase,
    allowed_probe_ids: &'a BTreeSet<StableId>,
    claims: BTreeMap<&'a StableId, &'a EngineeringClaim>,
    allowed_scope: BTreeSet<String>,
}

impl<'a> RecommendationValidationContext<'a> {
    fn new(
        case: &'a EngineeringCase,
        allowed_probe_ids: &'a BTreeSet<StableId>,
        claims: &'a [EngineeringClaim],
    ) -> Self {
        Self {
            case,
            allowed_probe_ids,
            claims: claims
                .iter()
                .map(|claim| (&claim.claim_id, claim))
                .collect(),
            allowed_scope: case
                .subject_ids
                .iter()
                .chain(&case.finding_ids)
                .cloned()
                .collect(),
        }
    }
}

fn validate_recommendation(
    recommendation: &ContextualRecommendation,
    duplicate: bool,
    context: &RecommendationValidationContext<'_>,
) -> Result<(), String> {
    validate_recommendation_identity(recommendation, duplicate, context.case)?;
    if !context
        .case
        .recommendation_families
        .contains(&recommendation.family)
    {
        return Err(format!(
            "recommendation {} field family is outside decision.recommendation_families",
            recommendation.recommendation_id
        ));
    }
    validate_scope(recommendation, context)?;
    validate_premises(recommendation, &context.claims)?;
    validate_alternative(recommendation)?;
    validate_benefits(recommendation, context.allowed_probe_ids)?;
    validate_verification(recommendation, context.allowed_probe_ids)
}

fn validate_recommendation_identity(
    recommendation: &ContextualRecommendation,
    duplicate: bool,
    case: &EngineeringCase,
) -> Result<(), String> {
    let rebuilt =
        ContextualRecommendation::from_draft(case.case_id.clone(), recommendation.to_draft())?;
    if recommendation.schema_version != CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION {
        return Err(format!(
            "recommendation {} has schema_version {}; expected {}",
            recommendation.recommendation_id,
            recommendation.schema_version,
            CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION
        ));
    }
    if recommendation.case_id != case.case_id {
        return Err(format!(
            "recommendation {} field case_id is outside the admitted case",
            recommendation.recommendation_id
        ));
    }
    if recommendation.recommendation_id != rebuilt.recommendation_id {
        return Err(format!(
            "recommendation {} has an inconsistent host-derived identity",
            recommendation.recommendation_id
        ));
    }
    if duplicate {
        Err(format!(
            "recommendation {} is duplicated",
            recommendation.recommendation_id
        ))
    } else {
        Ok(())
    }
}

fn validate_scope(
    recommendation: &ContextualRecommendation,
    context: &RecommendationValidationContext<'_>,
) -> Result<(), String> {
    let invalid_scope = recommendation
        .scope_ids
        .iter()
        .filter(|id| !context.allowed_scope.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !invalid_scope.is_empty() {
        return Err(format!(
            "recommendation {} field scope_ids contains out-of-case values: {}",
            recommendation.recommendation_id,
            invalid_scope.join(", ")
        ));
    }
    let invalid_findings = recommendation
        .affected_finding_ids
        .iter()
        .filter(|id| !context.case.finding_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if invalid_findings.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "recommendation {} field affected_finding_ids contains out-of-case values: {}",
            recommendation.recommendation_id,
            invalid_findings.join(", ")
        ))
    }
}

fn validate_premises(
    recommendation: &ContextualRecommendation,
    claims: &BTreeMap<&StableId, &EngineeringClaim>,
) -> Result<(), String> {
    for premise_id in &recommendation.premise_claim_ids {
        match claims.get(premise_id) {
            Some(premise) if premise.status != ClaimStatus::NotSubstantiated => {
                if premise.status != ClaimStatus::Blocked
                    || recommendation.family == RecommendationFamily::MeasurementBeforeChange
                {
                    continue;
                }
                return Err("recommendation depends on an invalidated premise".to_string());
            }
            Some(_) => return Err("recommendation depends on an invalidated premise".to_string()),
            None => return Err("recommendation has a dangling premise claim".to_string()),
        }
    }
    Ok(())
}

fn validate_alternative(recommendation: &ContextualRecommendation) -> Result<(), String> {
    let missing = recommendation.family == RecommendationFamily::LifecycleReownership
        && recommendation.alternatives.is_empty()
        && recommendation
            .no_alternative_reason
            .as_deref()
            .is_none_or(str::is_empty);
    if missing {
        Err("architectural recommendation requires an alternative".to_string())
    } else {
        Ok(())
    }
}

fn validate_benefits(
    recommendation: &ContextualRecommendation,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> Result<(), String> {
    for benefit in &recommendation.expected_benefits {
        validate_benefit(benefit, allowed_probe_ids)?;
    }
    Ok(())
}

fn validate_benefit(
    benefit: &super::ExpectedBenefit,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> Result<(), String> {
    let invalid = benefit.description.trim().is_empty()
        || benefit
            .evidence_probe_ids
            .iter()
            .any(|id| !allowed_probe_ids.contains(id));
    if invalid {
        return Err("recommendation benefit is invalid".to_string());
    }
    let Some(value) = &benefit.numeric_value else {
        return Ok(());
    };
    let numeric = value.parse::<f64>().ok().filter(|value| value.is_finite());
    if numeric.is_none() {
        return Err("numeric benefit value is not a finite number".to_string());
    }
    validate_numeric_benefit(benefit)
}

fn validate_numeric_benefit(benefit: &super::ExpectedBenefit) -> Result<(), String> {
    let invalid = benefit.unit.as_deref().is_none_or(str::is_empty)
        || !matches!(
            benefit.evidence_mode,
            BenefitEvidenceMode::DirectMeasurement | BenefitEvidenceMode::BoundedExtrapolation
        )
        || (benefit.evidence_mode == BenefitEvidenceMode::BoundedExtrapolation
            && benefit
                .extrapolation_model
                .as_deref()
                .is_none_or(str::is_empty));
    if invalid {
        Err("numeric benefit mode, unit, or extrapolation model is invalid".to_string())
    } else {
        Ok(())
    }
}

fn validate_verification(
    recommendation: &ContextualRecommendation,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> Result<(), String> {
    let invalid = recommendation.verification_steps.iter().any(|step| {
        step.method.trim().is_empty()
            || step.expected_observation.trim().is_empty()
            || step
                .evidence_probe_ids
                .iter()
                .any(|id| !allowed_probe_ids.contains(id))
    });
    if invalid {
        Err("recommendation verification is invalid".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        conclusion_allows_recommendation, required_recommendation_family,
        validate_recommendations_for_conclusion,
    };
    use crate::{CaseConclusion, RecommendationFamily};

    #[test]
    fn recommendation_family_is_derived_from_the_case_conclusion() {
        let cases = [
            (
                CaseConclusion::LifecycleMismatchSubstantiated,
                Some(RecommendationFamily::LifecycleReownership),
            ),
            (
                CaseConclusion::RepetitionJustified,
                Some(RecommendationFamily::ExplicitRetentionAvoidance),
            ),
            (
                CaseConclusion::WiderOwnershipInfeasible,
                Some(RecommendationFamily::ExplicitRetentionAvoidance),
            ),
            (
                CaseConclusion::InsufficientEvidence,
                Some(RecommendationFamily::MeasurementBeforeChange),
            ),
            (CaseConclusion::ImpactImmaterial, None),
        ];

        for (conclusion, allowed) in cases {
            assert_eq!(required_recommendation_family(conclusion), allowed);
            for family in [
                RecommendationFamily::LifecycleReownership,
                RecommendationFamily::ExplicitRetentionAvoidance,
                RecommendationFamily::MeasurementBeforeChange,
            ] {
                assert_eq!(
                    conclusion_allows_recommendation(conclusion, family),
                    Some(family) == allowed,
                    "unexpected admission for {conclusion:?} and {family:?}"
                );
            }
        }

        for conclusion in [
            CaseConclusion::LifecycleMismatchSubstantiated,
            CaseConclusion::RepetitionJustified,
            CaseConclusion::WiderOwnershipInfeasible,
            CaseConclusion::InsufficientEvidence,
        ] {
            let error = validate_recommendations_for_conclusion(conclusion, &[]).unwrap_err();
            assert!(error.contains("requires at least one"));
        }
        assert!(
            validate_recommendations_for_conclusion(CaseConclusion::ImpactImmaterial, &[]).is_ok()
        );
    }
}

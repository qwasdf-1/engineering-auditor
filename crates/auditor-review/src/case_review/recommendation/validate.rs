use std::collections::{BTreeMap, BTreeSet};

use super::{
    BenefitEvidenceMode, ContextualRecommendation, CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};
use crate::{ClaimStatus, EngineeringClaim, RecommendationFamily, StableId};

/// What a question lets its recommendations touch.
pub struct RecommendationBounds<'a> {
    pub case_id: &'a StableId,
    pub subject_ids: &'a [String],
    pub finding_ids: &'a [String],
    pub families: &'a [RecommendationFamily],
}

pub fn validate_recommendations(
    bounds: &RecommendationBounds<'_>,
    known_evidence_ids: &BTreeSet<String>,
    claims: &[EngineeringClaim],
    recommendations: &[ContextualRecommendation],
) -> Result<(), String> {
    let context = RecommendationValidationContext::new(bounds, known_evidence_ids, claims);
    let mut ids = BTreeSet::new();
    for recommendation in recommendations {
        let duplicate = !ids.insert(recommendation.recommendation_id.clone());
        validate_recommendation(recommendation, duplicate, &context)?;
    }
    Ok(())
}

struct RecommendationValidationContext<'a> {
    bounds: &'a RecommendationBounds<'a>,
    known_evidence_ids: &'a BTreeSet<String>,
    claims: BTreeMap<&'a StableId, &'a EngineeringClaim>,
    allowed_scope: BTreeSet<&'a String>,
}

impl<'a> RecommendationValidationContext<'a> {
    fn new(
        bounds: &'a RecommendationBounds<'a>,
        known_evidence_ids: &'a BTreeSet<String>,
        claims: &'a [EngineeringClaim],
    ) -> Self {
        Self {
            bounds,
            known_evidence_ids,
            claims: claims
                .iter()
                .map(|claim| (&claim.claim_id, claim))
                .collect(),
            allowed_scope: bounds
                .subject_ids
                .iter()
                .chain(bounds.finding_ids)
                .collect(),
        }
    }
}

fn validate_recommendation(
    recommendation: &ContextualRecommendation,
    duplicate: bool,
    context: &RecommendationValidationContext<'_>,
) -> Result<(), String> {
    validate_recommendation_identity(recommendation, duplicate, context.bounds.case_id)?;
    if !context.bounds.families.contains(&recommendation.family) {
        return Err(format!(
            "recommendation {} field family is outside decision.recommendation_families",
            recommendation.recommendation_id
        ));
    }
    validate_scope(recommendation, context)?;
    validate_premises(recommendation, &context.claims)?;
    validate_alternative(recommendation)?;
    validate_benefits(recommendation, context.known_evidence_ids)?;
    validate_verification(recommendation, context.known_evidence_ids)
}

fn validate_recommendation_identity(
    recommendation: &ContextualRecommendation,
    duplicate: bool,
    case_id: &StableId,
) -> Result<(), String> {
    let rebuilt = ContextualRecommendation::from_draft(case_id.clone(), recommendation.to_draft())?;
    if recommendation.schema_version != CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION {
        return Err(format!(
            "recommendation {} has schema_version {}; expected {}",
            recommendation.recommendation_id,
            recommendation.schema_version,
            CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION
        ));
    }
    if &recommendation.case_id != case_id {
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
        .filter(|id| !context.allowed_scope.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    if !invalid_scope.is_empty() {
        return Err(format!(
            "recommendation {} field scope_ids contains out-of-case values: {}; scope_ids takes only this question's subject IDs ({}) and finding IDs; a file or symbol you read is evidence, not scope",
            recommendation.recommendation_id,
            invalid_scope.join(", "),
            context.bounds.subject_ids.join(", ")
        ));
    }
    let invalid_findings = recommendation
        .affected_finding_ids
        .iter()
        .filter(|id| !context.bounds.finding_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if invalid_findings.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "recommendation {} field affected_finding_ids contains out-of-case values: {}; only the question's finding IDs are allowed",
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
        let Some(premise) = claims.get(premise_id) else {
            return Err("recommendation has a dangling premise claim".to_string());
        };
        let invalidated = match premise.status {
            ClaimStatus::NotSubstantiated => true,
            ClaimStatus::Blocked => {
                recommendation.family != RecommendationFamily::MeasurementBeforeChange
            }
            _ => false,
        };
        if invalidated {
            let status = serde_json::to_value(premise.status).unwrap_or_default();
            return Err(format!(
                "recommendation {} depends on an invalidated premise: claim {premise_id} is {}; substantiate the claim with citations or drop it from premises, and drop the recommendation if no premise remains (a blocked premise suits only measurement_before_change)",
                recommendation.recommendation_id,
                status.as_str().unwrap_or("invalidated"),
            ));
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
        Err(format!(
            "recommendation {} is lifecycle_reownership, which requires an alternative: add one or more entries to its alternatives, each a summary of another way to act and its tradeoffs, or, if there is no other way, say why in no_alternative_reason",
            recommendation.recommendation_id
        ))
    } else {
        Ok(())
    }
}

fn validate_benefits(
    recommendation: &ContextualRecommendation,
    known_evidence_ids: &BTreeSet<String>,
) -> Result<(), String> {
    for benefit in &recommendation.expected_benefits {
        validate_benefit(benefit, known_evidence_ids)?;
    }
    Ok(())
}

fn validate_benefit(
    benefit: &super::ExpectedBenefit,
    known_evidence_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let invalid = benefit.description.trim().is_empty()
        || benefit
            .evidence_ids
            .iter()
            .any(|id| !known_evidence_ids.contains(id));
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
    let missing = |field: &Option<String>| field.as_deref().is_none_or(str::is_empty);
    let error = match benefit.evidence_mode {
        BenefitEvidenceMode::DirectMeasurement | BenefitEvidenceMode::BoundedExtrapolation
            if missing(&benefit.unit) =>
        {
            "a numeric benefit value needs its unit"
        }
        BenefitEvidenceMode::BoundedExtrapolation if missing(&benefit.extrapolation_model) => {
            "a bounded_extrapolation benefit needs the extrapolation_model it was made with"
        }
        BenefitEvidenceMode::DirectMeasurement | BenefitEvidenceMode::BoundedExtrapolation => {
            return Ok(())
        }
        _ => "a numeric benefit value needs evidence_mode direct_measurement or bounded_extrapolation; for any other mode, drop numeric_value and state the size in the description",
    };
    Err(error.to_string())
}

fn validate_verification(
    recommendation: &ContextualRecommendation,
    known_evidence_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let invalid = recommendation.verification_steps.iter().any(|step| {
        step.method.trim().is_empty()
            || step.expected_observation.trim().is_empty()
            || step
                .evidence_ids
                .iter()
                .any(|id| !known_evidence_ids.contains(id))
    });
    if invalid {
        Err("recommendation verification is invalid".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric(
        mode: BenefitEvidenceMode,
        unit: Option<&str>,
        model: Option<&str>,
    ) -> Result<(), String> {
        validate_numeric_benefit(&super::super::ExpectedBenefit {
            description: "Fewer branches.".to_string(),
            evidence_mode: mode,
            numeric_value: Some("3".to_string()),
            unit: unit.map(str::to_string),
            extrapolation_model: model.map(str::to_string),
            evidence_ids: Vec::new(),
        })
    }

    #[test]
    fn a_numeric_benefit_error_names_what_to_add_or_drop() {
        use BenefitEvidenceMode::{BoundedExtrapolation, DirectMeasurement, StructuralInference};
        assert_eq!(numeric(DirectMeasurement, Some("branches"), None), Ok(()));
        assert_eq!(
            numeric(BoundedExtrapolation, Some("calls"), Some("per entry")),
            Ok(())
        );
        assert!(numeric(DirectMeasurement, None, None)
            .unwrap_err()
            .ends_with("needs its unit"));
        assert!(numeric(BoundedExtrapolation, Some("calls"), None)
            .unwrap_err()
            .contains("needs the extrapolation_model"));
        assert!(numeric(StructuralInference, Some("findings"), None)
            .unwrap_err()
            .contains("drop numeric_value and state the size in the description"));
    }
}

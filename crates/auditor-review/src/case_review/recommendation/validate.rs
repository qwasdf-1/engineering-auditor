use std::collections::{BTreeMap, BTreeSet};

use super::{
    BenefitEvidenceMode, ContextualRecommendation, CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};
use crate::{ClaimStatus, EngineeringCase, EngineeringClaim, RecommendationFamily, StableId};

/// What a case or a question lets its recommendations touch.
pub struct RecommendationBounds<'a> {
    pub case_id: &'a StableId,
    pub subject_ids: &'a [String],
    pub finding_ids: &'a [String],
    pub families: &'a [RecommendationFamily],
}

impl EngineeringCase {
    pub fn recommendation_bounds(&self) -> RecommendationBounds<'_> {
        RecommendationBounds {
            case_id: &self.case_id,
            subject_ids: &self.subject_ids,
            finding_ids: &self.finding_ids,
            families: &self.recommendation_families,
        }
    }
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
            "recommendation {} field scope_ids contains out-of-case values: {}",
            recommendation.recommendation_id,
            invalid_scope.join(", ")
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

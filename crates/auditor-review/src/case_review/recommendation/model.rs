use serde::{Deserialize, Serialize};

use super::VerificationStep;
use crate::case_review::identity::case_draft_id;
use crate::{ClaimConfidence, RecommendationFamily, StableId};

pub const CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorAdviceRelationship {
    Preserves,
    Consolidates,
    Narrows,
    Sequences,
    Defers,
    NotActionableWithCurrentEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenefitEvidenceMode {
    DirectMeasurement,
    BoundedExtrapolation,
    StructuralInference,
    QualitativeOnly,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Materiality {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedBenefit {
    pub description: String,
    pub evidence_mode: BenefitEvidenceMode,
    /// Canonical decimal text preserves the model's exact claim without binary-float drift.
    pub numeric_value: Option<String>,
    pub unit: Option<String>,
    pub extrapolation_model: Option<String>,
    pub evidence_probe_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationAlternative {
    pub summary: String,
    pub tradeoffs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationDraft {
    pub family: RecommendationFamily,
    pub summary: String,
    pub scope_ids: Vec<String>,
    pub premise_claim_ids: Vec<StableId>,
    pub affected_finding_ids: Vec<String>,
    pub detector_advice_relationship: DetectorAdviceRelationship,
    pub preconditions: Vec<String>,
    pub expected_benefits: Vec<ExpectedBenefit>,
    pub risks: Vec<String>,
    pub costs: Vec<String>,
    pub alternatives: Vec<RecommendationAlternative>,
    pub no_alternative_reason: Option<String>,
    pub verification_steps: Vec<VerificationStep>,
    pub materiality: Materiality,
    pub materiality_basis: String,
    pub confidence: ClaimConfidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextualRecommendation {
    pub schema_version: u32,
    pub recommendation_id: StableId,
    pub case_id: StableId,
    #[serde(flatten)]
    pub content: RecommendationDraft,
}

impl ContextualRecommendation {
    pub fn from_draft(case_id: StableId, mut draft: RecommendationDraft) -> Result<Self, String> {
        normalize(&mut draft.scope_ids);
        normalize(&mut draft.premise_claim_ids);
        normalize(&mut draft.affected_finding_ids);
        normalize(&mut draft.preconditions);
        normalize(&mut draft.risks);
        normalize(&mut draft.costs);
        for benefit in &mut draft.expected_benefits {
            normalize(&mut benefit.evidence_probe_ids);
        }
        for alternative in &mut draft.alternatives {
            normalize(&mut alternative.tradeoffs);
        }
        for step in &mut draft.verification_steps {
            normalize(&mut step.evidence_probe_ids);
        }
        if draft.summary.trim().is_empty()
            || draft.scope_ids.is_empty()
            || draft.premise_claim_ids.is_empty()
            || draft.verification_steps.is_empty()
            || draft.materiality_basis.trim().is_empty()
        {
            return Err("recommendation is missing required decision content".to_string());
        }
        let recommendation_id = case_draft_id(
            "contextual-recommendation",
            CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
            &case_id,
            &draft,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            schema_version: CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
            recommendation_id,
            case_id,
            content: draft,
        })
    }

    pub fn to_draft(&self) -> RecommendationDraft {
        self.content.clone()
    }
}

impl std::ops::Deref for ContextualRecommendation {
    type Target = RecommendationDraft;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

fn normalize<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

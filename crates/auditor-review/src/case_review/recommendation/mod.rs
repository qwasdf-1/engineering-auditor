mod model;
mod validate;
mod verification;

pub use model::{
    BenefitEvidenceMode, ContextualRecommendation, DetectorAdviceRelationship, ExpectedBenefit,
    Materiality, RecommendationAlternative, RecommendationDraft,
    CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};
pub(crate) use validate::{
    conclusion_allows_recommendation, required_recommendation_family, validate_recommendations,
    validate_recommendations_for_conclusion,
};
pub use verification::VerificationStep;

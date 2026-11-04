mod model;
mod validate;
mod verification;

pub use model::{
    BenefitEvidenceMode, ContextualRecommendation, DetectorAdviceRelationship, ExpectedBenefit,
    Materiality, RecommendationAlternative, RecommendationDraft,
    CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};
pub use validate::{validate_recommendations, RecommendationBounds};
pub use verification::VerificationStep;

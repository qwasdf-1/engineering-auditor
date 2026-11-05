pub mod claim;
mod identity;
pub mod model_protocol;
pub mod recommendation;

pub use claim::{
    validate_claims, ClaimConfidence, ClaimDraft, ClaimSlot, ClaimStatus, EngineeringClaim,
    EngineeringClaimKind, ENGINEERING_CLAIM_SCHEMA_VERSION,
};
pub use identity::{ContentDigest, IdentityError, StableId};
pub use model_protocol::{
    model_value_from_json, render_model_document, render_structured_model_document, ModelDocument,
    ModelField, ModelProjection, ModelProjectionAudit, ModelSection, ModelValue,
    MODEL_MARKDOWN_RENDERER_VERSION,
};
pub use recommendation::{
    validate_recommendations, BenefitEvidenceMode, ContextualRecommendation,
    DetectorAdviceRelationship, ExpectedBenefit, Materiality, RecommendationAlternative,
    RecommendationBounds, RecommendationDraft, RecommendationFamily, VerificationStep,
    CONTEXTUAL_RECOMMENDATION_SCHEMA_VERSION,
};

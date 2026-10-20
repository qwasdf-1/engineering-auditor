mod bundle;
mod closure;
mod instantiate;
mod registry;
mod template;
mod validate;

pub use bundle::{
    CaseTemplateBundle, CaseTemplateEntry, TemplateStatus, CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION,
};
pub use closure::{
    CaseClosure, CaseClosureError, CaseClosureReason, CaseFailure, FinalCaseState,
    CASE_CLOSURE_SCHEMA_VERSION,
};
pub use instantiate::{
    CaseInstantiationResult, EngineeringCase, OperatorCaseConstraints, ProbeRef,
    TemplateNotEnabledReason, TemplateNotEnabledRecord, ENGINEERING_CASE_SCHEMA_VERSION,
};
pub use registry::{CaseInstantiationError, CaseTemplateRegistry};
pub use template::{
    CaseConclusion, CaseTemplate, CaseTemplateDefinition, ChallengePolicy, ChallengeTrigger,
    ClaimSlot, ClosurePolicy, EvidenceClass, EvidenceContractTemplate, EvidenceRequirement,
    InferenceGuard, InferenceGuardKind, MandatoryProbeTemplate, OptionalProbePolicy, ProbeKind,
    RecommendationFamily, RoutePolicy, RoutePredicate, SubjectPolicy, TriggerPolicy,
    CASE_TEMPLATE_SCHEMA_VERSION, CASE_TEMPLATE_VERSION,
};

#[cfg(test)]
mod tests;

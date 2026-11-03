mod bundle;
mod instantiate;
mod registry;
mod template;
mod validate;

pub use bundle::{
    CaseTemplateBundle, CaseTemplateEntry, TemplateStatus, CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION,
};
pub use instantiate::{
    CaseInstantiationResult, EngineeringCase, OperatorCaseConstraints, TemplateNotEnabledReason,
    TemplateNotEnabledRecord, ENGINEERING_CASE_SCHEMA_VERSION,
};
pub use registry::{CaseInstantiationError, CaseTemplateRegistry};
pub use template::{
    CaseTemplate, CaseTemplateDefinition, ClaimSlot, EvidenceClass, EvidenceContractTemplate,
    EvidenceRequirement, RecommendationFamily, SubjectPolicy, TriggerPolicy,
    CASE_TEMPLATE_SCHEMA_VERSION, CASE_TEMPLATE_VERSION,
};

#[cfg(test)]
mod tests;

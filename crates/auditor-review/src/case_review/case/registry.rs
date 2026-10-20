use std::collections::BTreeMap;
use std::fmt;

use super::instantiate::{build_case, TemplateNotEnabledRecord};
use super::{
    CaseInstantiationResult, CaseTemplate, CaseTemplateBundle, CaseTemplateDefinition,
    EngineeringCase, TemplateStatus,
};
use crate::{CaseKind, CasePlanningStatus, ReviewAgenda, ReviewCandidate, ReviewInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaseInstantiationError {
    InvalidBundle { detail: String },
    BundleMismatch,
    DuplicateOwner { kind: CaseKind, version: u32 },
    MissingTemplateVersion { kind: CaseKind },
    InvalidCandidate { detail: String },
    CandidateIneligible { kind: CaseKind },
    TriggerMismatch,
    DisconnectedSubjects,
    DanglingReference { kind: &'static str },
    InvalidCase { detail: String },
    InvalidAgenda { detail: String },
}

impl fmt::Display for CaseInstantiationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

impl std::error::Error for CaseInstantiationError {}

#[derive(Clone)]
struct RegisteredTemplate {
    definition: CaseTemplateDefinition,
    status: TemplateStatus,
}

impl CaseTemplate for RegisteredTemplate {
    fn kind(&self) -> CaseKind {
        self.definition.kind
    }

    fn version(&self) -> u32 {
        self.definition.version
    }

    fn instantiate(
        &self,
        candidate: &ReviewCandidate,
        input: &ReviewInput,
    ) -> Result<EngineeringCase, CaseInstantiationError> {
        build_case(&self.definition, self.status, candidate, input)
    }
}

pub struct CaseTemplateRegistry {
    bundle: CaseTemplateBundle,
    templates: BTreeMap<(CaseKind, u32), Box<dyn CaseTemplate>>,
}

impl CaseTemplateRegistry {
    pub fn from_input(input: &ReviewInput) -> Result<Self, CaseInstantiationError> {
        if input.case_template_bundle().digest() != &input.binding().case_template_bundle_digest {
            return Err(CaseInstantiationError::BundleMismatch);
        }
        let supplied =
            CaseTemplateBundle::from_value(input.case_template_bundle().semantic_snapshot())
                .map_err(|detail| CaseInstantiationError::InvalidBundle { detail })?;
        let checked_in = CaseTemplateBundle::checked_in()
            .map_err(|detail| CaseInstantiationError::InvalidBundle { detail })?;
        if supplied != checked_in {
            return Err(CaseInstantiationError::BundleMismatch);
        }
        Self::from_bundle(supplied)
    }

    fn from_bundle(bundle: CaseTemplateBundle) -> Result<Self, CaseInstantiationError> {
        bundle
            .validate()
            .map_err(|detail| CaseInstantiationError::InvalidBundle { detail })?;
        let mut registry = Self {
            bundle,
            templates: BTreeMap::new(),
        };
        for entry in registry.bundle.entries.clone() {
            if matches!(
                entry.status,
                TemplateStatus::Production | TemplateStatus::Shadow
            ) {
                let definition = entry
                    .template
                    .ok_or(CaseInstantiationError::MissingTemplateVersion { kind: entry.kind })?;
                registry.register(Box::new(RegisteredTemplate {
                    definition,
                    status: entry.status,
                }))?;
            }
        }
        Ok(registry)
    }

    #[cfg(test)]
    pub(super) fn from_bundle_for_test(
        bundle: CaseTemplateBundle,
    ) -> Result<Self, CaseInstantiationError> {
        Self::from_bundle(bundle)
    }

    pub fn bundle(&self) -> &CaseTemplateBundle {
        &self.bundle
    }

    pub fn instantiate(
        &self,
        candidate: &ReviewCandidate,
        kind: CaseKind,
        input: &ReviewInput,
    ) -> Result<CaseInstantiationResult, CaseInstantiationError> {
        candidate
            .validate()
            .map_err(|detail| CaseInstantiationError::InvalidCandidate { detail })?;
        if !candidate.eligible_case_kinds.contains(&kind) {
            return Err(CaseInstantiationError::CandidateIneligible { kind });
        }
        let entry = self
            .bundle
            .entry(kind)
            .ok_or(CaseInstantiationError::MissingTemplateVersion { kind })?;
        if entry.status == TemplateStatus::NotEnabled {
            return Ok(CaseInstantiationResult::TemplateNotEnabled(
                TemplateNotEnabledRecord::new(candidate, kind, entry.version, input),
            ));
        }
        let template = self
            .templates
            .get(&(kind, entry.version))
            .ok_or(CaseInstantiationError::MissingTemplateVersion { kind })?;
        template
            .instantiate(candidate, input)
            .map(Box::new)
            .map(CaseInstantiationResult::Instantiated)
    }

    pub fn instantiate_agenda(
        &self,
        agenda: &ReviewAgenda,
        input: &ReviewInput,
    ) -> Result<Vec<CaseInstantiationResult>, CaseInstantiationError> {
        agenda
            .validate(input)
            .map_err(|error| CaseInstantiationError::InvalidAgenda {
                detail: error.to_string(),
            })?;
        let candidates = agenda
            .candidates
            .iter()
            .map(|candidate| (&candidate.candidate_id, candidate))
            .collect::<BTreeMap<_, _>>();
        agenda
            .case_planning_records
            .iter()
            .filter(|record| record.status == CasePlanningStatus::Planned)
            .map(|record| {
                let candidate = candidates.get(&record.candidate_id).ok_or_else(|| {
                    CaseInstantiationError::InvalidAgenda {
                        detail: "case plan references unknown candidate".to_string(),
                    }
                })?;
                self.instantiate(candidate, record.case_kind, input)
            })
            .collect()
    }

    fn register(&mut self, template: Box<dyn CaseTemplate>) -> Result<(), CaseInstantiationError> {
        let key = (template.kind(), template.version());
        if self.templates.insert(key, template).is_some() {
            return Err(CaseInstantiationError::DuplicateOwner {
                kind: key.0,
                version: key.1,
            });
        }
        Ok(())
    }
}

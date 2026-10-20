use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::candidate::{CaseKind, ReviewCandidate};
use super::index::AgendaIndex;
use super::normalize::normalize_candidates;
use super::planning_ops::{
    apply_budget, coverage, decide_candidates, duplicate_suppressions, materialize_plans,
    select_candidates,
};
use super::selector::{CandidateSelectionError, SelectorManifestEntry, SelectorRegistry};
use super::suppress::{AgendaPolicy, FoundationAgendaPolicy, SuppressionReason, SuppressionRecord};
use crate::case_review::identity::{canonical_json, CanonicalIdentity};
use crate::{InputBinding, ReviewInput, StableId};

pub const REVIEW_AGENDA_SCHEMA_VERSION: u32 = 1;
pub const AGENDA_PLANNER_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CasePlanningStatus {
    Planned,
    Suppressed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CasePlanningRecord {
    pub plan_id: StableId,
    pub candidate_id: StableId,
    pub case_kind: CaseKind,
    pub status: CasePlanningStatus,
    pub suppression_id: Option<StableId>,
}

impl CasePlanningRecord {
    pub(super) fn new(
        candidate_id: StableId,
        case_kind: CaseKind,
        status: CasePlanningStatus,
        suppression_id: Option<StableId>,
    ) -> Self {
        let identity = json!({
            "candidate_id": candidate_id,
            "case_kind": case_kind,
            "status": status,
            "suppression_id": suppression_id,
        });
        let plan_id = StableId::from_identity("case-plan", 1, &identity)
            .expect("case-plan is a valid typed prefix");
        Self {
            plan_id,
            candidate_id,
            case_kind,
            status,
            suppression_id,
        }
    }

    fn validate(&self) -> Result<(), CandidateSelectionError> {
        if !self.plan_id.as_str().starts_with("case-plan:")
            || !self.candidate_id.as_str().starts_with("review-candidate:")
        {
            return Err(invalid_agenda("case plan has the wrong typed id prefix"));
        }
        let expected = Self::new(
            self.candidate_id.clone(),
            self.case_kind,
            self.status,
            self.suppression_id.clone(),
        );
        if self.plan_id != expected.plan_id {
            return Err(invalid_agenda("case plan stable id mismatch"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgendaCoverage {
    pub source_candidate_count: usize,
    pub candidate_count: usize,
    pub suppression_count: usize,
    pub planned_case_count: usize,
    pub referenced_finding_count: usize,
    pub not_selected_finding_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAgenda {
    pub schema_version: u32,
    pub agenda_id: StableId,
    pub input_binding: InputBinding,
    pub selector_manifest: Vec<SelectorManifestEntry>,
    pub candidates: Vec<ReviewCandidate>,
    pub suppression_records: Vec<SuppressionRecord>,
    pub case_planning_records: Vec<CasePlanningRecord>,
    pub coverage: AgendaCoverage,
    pub planner_version: u32,
}

impl ReviewAgenda {
    pub fn from_json(text: &str, input: &ReviewInput) -> Result<Self, CandidateSelectionError> {
        let agenda: Self =
            serde_json::from_str(text).map_err(|error| CandidateSelectionError::InvalidAgenda {
                detail: error.to_string(),
            })?;
        agenda.validate(input)?;
        Ok(agenda)
    }

    pub fn to_canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).expect("review agenda is serializable"))
    }

    pub fn validate(&self, input: &ReviewInput) -> Result<(), CandidateSelectionError> {
        validate_agenda_header(self, input)?;
        validate_manifest(&self.selector_manifest)?;
        let index = AgendaIndex::from_input(input)?;
        validate_ordered_content(self, &index)?;
        let suppressed_candidates = validate_suppressions(self, &index)?;
        validate_case_plans(self, &suppressed_candidates)?;
        validate_manifest_ownership(self)?;
        let expected_coverage = coverage(
            &self.candidates,
            &self.suppression_records,
            &self.case_planning_records,
            &index,
        );
        if self.coverage != expected_coverage {
            return Err(invalid_agenda("agenda coverage mismatch"));
        }
        if self.agenda_id != self.expected_id() {
            return Err(invalid_agenda("agenda stable id mismatch"));
        }
        Ok(())
    }

    fn expected_id(&self) -> StableId {
        StableId::from_identity(
            "review-agenda",
            REVIEW_AGENDA_SCHEMA_VERSION,
            &self.identity_fields(),
        )
        .expect("review-agenda is a valid typed prefix")
    }
}

impl CanonicalIdentity for ReviewAgenda {
    fn identity_fields(&self) -> Value {
        json!({
            "candidates": self.candidates,
            "case_planning_records": self.case_planning_records,
            "coverage": self.coverage,
            "input_binding": self.input_binding,
            "planner_version": self.planner_version,
            "selector_manifest": self.selector_manifest,
            "suppression_records": self.suppression_records,
        })
    }
}

fn validate_agenda_header(
    agenda: &ReviewAgenda,
    input: &ReviewInput,
) -> Result<(), CandidateSelectionError> {
    if agenda.schema_version != REVIEW_AGENDA_SCHEMA_VERSION
        || agenda.planner_version != AGENDA_PLANNER_VERSION
    {
        return Err(invalid_agenda("unsupported agenda or planner version"));
    }
    agenda
        .input_binding
        .validate()
        .map_err(|detail| invalid_agenda(&detail))?;
    if &agenda.input_binding != input.binding() {
        return Err(invalid_agenda("agenda input binding mismatch"));
    }
    Ok(())
}

fn validate_ordered_content(
    agenda: &ReviewAgenda,
    index: &AgendaIndex,
) -> Result<(), CandidateSelectionError> {
    validate_sorted_ids(
        agenda
            .candidates
            .iter()
            .map(|candidate| &candidate.candidate_id),
        "candidate",
    )?;
    for candidate in &agenda.candidates {
        index.validate_candidate(candidate)?;
    }
    validate_sorted_ids(
        agenda
            .suppression_records
            .iter()
            .map(|record| &record.suppression_id),
        "suppression",
    )?;
    validate_sorted_ids(
        agenda
            .case_planning_records
            .iter()
            .map(|record| &record.plan_id),
        "case plan",
    )
}

fn validate_suppressions(
    agenda: &ReviewAgenda,
    index: &AgendaIndex,
) -> Result<BTreeMap<StableId, StableId>, CandidateSelectionError> {
    let candidate_ids = agenda
        .candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<BTreeSet<_>>();
    let source_ids = agenda
        .candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .source_candidates
                .iter()
                .map(|source| source.candidate_id.clone())
        })
        .collect::<BTreeSet<_>>();
    let mut suppressed = BTreeMap::new();
    for record in &agenda.suppression_records {
        record
            .validate()
            .map_err(|detail| invalid_agenda(&detail))?;
        validate_suppression_evidence(record, index)?;
        if record.reason == SuppressionReason::DuplicateOfCase {
            validate_duplicate_suppression(record, &source_ids, &candidate_ids)?;
        } else if !candidate_ids.contains(&record.candidate_id)
            || suppressed
                .insert(record.candidate_id.clone(), record.suppression_id.clone())
                .is_some()
        {
            return Err(invalid_agenda(
                "candidate suppression is dangling or duplicated",
            ));
        }
    }
    Ok(suppressed)
}

fn validate_suppression_evidence(
    record: &SuppressionRecord,
    index: &AgendaIndex,
) -> Result<(), CandidateSelectionError> {
    if record
        .supporting_evidence_refs
        .iter()
        .any(|reference| !index.contains_node(reference))
    {
        return Err(invalid_agenda(
            "suppression references unknown supporting evidence",
        ));
    }
    Ok(())
}

fn validate_duplicate_suppression(
    record: &SuppressionRecord,
    source_ids: &BTreeSet<StableId>,
    candidate_ids: &BTreeSet<StableId>,
) -> Result<(), CandidateSelectionError> {
    if !source_ids.contains(&record.candidate_id)
        || record
            .duplicate_of_candidate_id
            .as_ref()
            .is_none_or(|id| !candidate_ids.contains(id))
    {
        return Err(invalid_agenda(
            "duplicate suppression does not bind source and normalized candidates",
        ));
    }
    Ok(())
}

fn validate_case_plans(
    agenda: &ReviewAgenda,
    suppressed: &BTreeMap<StableId, StableId>,
) -> Result<(), CandidateSelectionError> {
    let candidates = agenda
        .candidates
        .iter()
        .map(|candidate| (&candidate.candidate_id, candidate))
        .collect::<BTreeMap<_, _>>();
    let suppression_ids = agenda
        .suppression_records
        .iter()
        .map(|record| &record.suppression_id)
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for record in &agenda.case_planning_records {
        validate_case_plan(record, &candidates, &suppression_ids, suppressed)?;
        if !actual.insert((&record.candidate_id, record.case_kind)) {
            return Err(invalid_agenda("duplicate candidate case plan"));
        }
    }
    let expected = agenda
        .candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .eligible_case_kinds
                .iter()
                .map(move |kind| (&candidate.candidate_id, *kind))
        })
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid_agenda(
            "agenda requires one case plan per eligible candidate kind",
        ));
    }
    Ok(())
}

fn validate_case_plan(
    record: &CasePlanningRecord,
    candidates: &BTreeMap<&StableId, &ReviewCandidate>,
    suppression_ids: &BTreeSet<&StableId>,
    suppressed: &BTreeMap<StableId, StableId>,
) -> Result<(), CandidateSelectionError> {
    record.validate()?;
    let candidate = candidates
        .get(&record.candidate_id)
        .ok_or_else(|| invalid_agenda("case plan references unknown candidate"))?;
    if !candidate.eligible_case_kinds.contains(&record.case_kind) {
        return Err(invalid_agenda("case plan uses ineligible case kind"));
    }
    match record.status {
        CasePlanningStatus::Planned if record.suppression_id.is_some() => {
            return Err(invalid_agenda("planned case has suppression id"));
        }
        CasePlanningStatus::Suppressed
            if record
                .suppression_id
                .as_ref()
                .is_none_or(|id| !suppression_ids.contains(id)) =>
        {
            return Err(invalid_agenda(
                "suppressed case requires a known suppression id",
            ));
        }
        _ => {}
    }
    if record.suppression_id.as_ref() != suppressed.get(&record.candidate_id) {
        return Err(invalid_agenda(
            "case plan status does not match candidate suppression",
        ));
    }
    Ok(())
}

fn validate_manifest_ownership(agenda: &ReviewAgenda) -> Result<(), CandidateSelectionError> {
    let owners = agenda
        .selector_manifest
        .iter()
        .map(|entry| (entry.id.as_str(), entry.version))
        .collect::<BTreeSet<_>>();
    if agenda.candidates.iter().any(|candidate| {
        candidate
            .source_candidates
            .iter()
            .any(|source| !owners.contains(&(source.selector_id.as_str(), source.selector_version)))
    }) {
        return Err(invalid_agenda(
            "candidate source selector is absent from selector manifest",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AgendaPlanner;

impl AgendaPlanner {
    pub fn plan(
        &self,
        input: &ReviewInput,
        registry: &SelectorRegistry,
        policy: &dyn AgendaPolicy,
    ) -> Result<ReviewAgenda, CandidateSelectionError> {
        let index = AgendaIndex::from_input(input)?;
        let raw_candidates = select_candidates(input, registry, &index)?;
        let normalized = normalize_candidates(raw_candidates, &index)?;
        let mut suppression_records = duplicate_suppressions(normalized.duplicates);
        let mut decisions = decide_candidates(input, policy, &index, &normalized.candidates)?;
        apply_budget(
            policy.max_planned_cases(),
            &normalized.candidates,
            &mut decisions,
        )?;
        let mut case_planning_records =
            materialize_plans(&normalized.candidates, decisions, &mut suppression_records);
        suppression_records.sort_by(|left, right| left.suppression_id.cmp(&right.suppression_id));
        case_planning_records.sort_by(|left, right| left.plan_id.cmp(&right.plan_id));
        let coverage = coverage(
            &normalized.candidates,
            &suppression_records,
            &case_planning_records,
            &index,
        );
        let mut agenda = ReviewAgenda {
            schema_version: REVIEW_AGENDA_SCHEMA_VERSION,
            agenda_id: StableId::from_identity(
                "review-agenda",
                REVIEW_AGENDA_SCHEMA_VERSION,
                &Value::Null,
            )
            .expect("review-agenda is a valid typed prefix"),
            input_binding: input.binding().clone(),
            selector_manifest: registry.manifest(),
            candidates: normalized.candidates,
            suppression_records,
            case_planning_records,
            coverage,
            planner_version: AGENDA_PLANNER_VERSION,
        };
        agenda.agenda_id = agenda.expected_id();
        agenda.validate(input)?;
        Ok(agenda)
    }

    pub fn plan_with_foundation_policy(
        &self,
        input: &ReviewInput,
    ) -> Result<ReviewAgenda, CandidateSelectionError> {
        self.plan(
            input,
            &SelectorRegistry::built_in(),
            &FoundationAgendaPolicy,
        )
    }
}

fn validate_manifest(manifest: &[SelectorManifestEntry]) -> Result<(), CandidateSelectionError> {
    let mut owners = BTreeSet::new();
    for (expected_order, entry) in manifest.iter().enumerate() {
        if entry.order != expected_order
            || entry.id.is_empty()
            || entry.version == 0
            || !owners.insert((entry.id.as_str(), entry.version))
        {
            return Err(invalid_agenda(
                "selector manifest is not ordered and unique",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_ids<'a>(
    ids: impl Iterator<Item = &'a StableId>,
    label: &str,
) -> Result<(), CandidateSelectionError> {
    let ids = ids.collect::<Vec<_>>();
    if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid_agenda(&format!(
            "{label} ids must be sorted and unique"
        )));
    }
    Ok(())
}

fn invalid_agenda(detail: &str) -> CandidateSelectionError {
    CandidateSelectionError::InvalidAgenda {
        detail: detail.to_string(),
    }
}

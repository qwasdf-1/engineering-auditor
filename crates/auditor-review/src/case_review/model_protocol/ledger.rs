use std::collections::{BTreeMap, BTreeSet};

use auditor_core::sha256_digest_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::StableId;

mod challenge;
mod error;
mod response;
mod reviewer;

pub use error::TaskLedgerError;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticRecord {
    pub reference: String,
    pub revision: u32,
    pub active: bool,
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticRevisionRecord {
    pub reference: String,
    pub revision: u32,
    pub before_digest: String,
    pub after_digest: String,
    pub reason: String,
    pub request_id: String,
    pub tool_call_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskCallProvenance<'a> {
    pub request_id: &'a str,
    pub tool_call_id: &'a str,
}

impl<'a> TaskCallProvenance<'a> {
    pub fn new(request_id: &'a str, tool_call_id: &'a str) -> Self {
        Self {
            request_id,
            tool_call_id,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticRecordLedger {
    records: BTreeMap<String, SemanticRecord>,
    #[serde(default)]
    creation_order: Vec<String>,
    revisions: Vec<SemanticRevisionRecord>,
}

impl SemanticRecordLedger {
    pub fn create(
        &mut self,
        reference: &str,
        value: Value,
        _provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        validate_reference(reference)?;
        if self.records.contains_key(reference) {
            return Err(TaskLedgerError::DuplicateReference(reference.to_string()));
        }
        self.records.insert(
            reference.to_string(),
            SemanticRecord {
                reference: reference.to_string(),
                revision: 1,
                active: true,
                value,
            },
        );
        self.creation_order.push(reference.to_string());
        Ok(())
    }

    pub fn revise(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.update_record(reference, expected_revision, reason, provenance, |record| {
            record.value = replacement;
            Ok(())
        })
    }

    pub fn withdraw(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.set_active(reference, expected_revision, false, reason, provenance)
    }

    pub fn reopen(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.set_active(reference, expected_revision, true, reason, provenance)
    }

    pub fn get(&self, reference: &str) -> Option<&SemanticRecord> {
        self.records.get(reference)
    }

    pub fn values(&self) -> impl Iterator<Item = &SemanticRecord> {
        self.records.values()
    }

    pub fn values_in_creation_order(&self) -> Vec<&SemanticRecord> {
        if self.creation_order.len() == self.records.len()
            && self
                .creation_order
                .iter()
                .all(|reference| self.records.contains_key(reference))
        {
            self.creation_order
                .iter()
                .filter_map(|reference| self.records.get(reference))
                .collect()
        } else {
            self.records.values().collect()
        }
    }

    fn is_active_before(&self, earlier: &str, later: &str) -> bool {
        let ordered = self.values_in_creation_order();
        let earlier_position = ordered
            .iter()
            .position(|record| record.reference == earlier && record.active);
        let later_position = ordered.iter().position(|record| record.reference == later);
        matches!((earlier_position, later_position), (Some(earlier), Some(later)) if earlier < later)
    }

    pub fn revisions(&self) -> &[SemanticRevisionRecord] {
        &self.revisions
    }

    fn set_active(
        &mut self,
        reference: &str,
        expected_revision: u32,
        active: bool,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.update_record(reference, expected_revision, reason, provenance, |record| {
            if record.active == active {
                return Err(if active {
                    TaskLedgerError::AlreadyActive(reference.to_string())
                } else {
                    TaskLedgerError::AlreadyWithdrawn(reference.to_string())
                });
            }
            record.active = active;
            Ok(())
        })
    }

    fn update_record(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
        update: impl FnOnce(&mut SemanticRecord) -> Result<(), TaskLedgerError>,
    ) -> Result<(), TaskLedgerError> {
        if reason.trim().is_empty() {
            return Err(TaskLedgerError::EmptyRevisionReason);
        }
        let record = self
            .records
            .get_mut(reference)
            .ok_or_else(|| TaskLedgerError::UnknownReference(reference.to_string()))?;
        if record.revision != expected_revision {
            return Err(TaskLedgerError::StaleRevision {
                reference: reference.to_string(),
                expected: expected_revision,
                actual: record.revision,
            });
        }
        let before_digest = semantic_record_digest(&record.value, record.active);
        update(record)?;
        record.revision += 1;
        let after_digest = semantic_record_digest(&record.value, record.active);
        self.revisions.push(SemanticRevisionRecord {
            reference: reference.to_string(),
            revision: record.revision,
            before_digest,
            after_digest,
            reason: reason.trim().to_string(),
            request_id: provenance.request_id.to_string(),
            tool_call_id: provenance.tool_call_id.to_string(),
        });
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerTaskRequirements {
    pub probe_ids: BTreeSet<String>,
    pub claim_slots: BTreeSet<String>,
    pub recommendation_families: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDeclineDisposition {
    pub task_id: String,
    pub disposition: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerTaskCompletion {
    pub preferred_recommendation_ref: Option<String>,
    pub unresolved_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerTaskLedger {
    requirements: ReviewerTaskRequirements,
    probe_answers: SemanticRecordLedger,
    claims: SemanticRecordLedger,
    recommendations: SemanticRecordLedger,
    completion: Option<ReviewerTaskCompletion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeTaskRequirements {
    pub targets: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "disposition")]
pub enum ChallengeTargetDisposition {
    Cleared,
    Objections { objection_refs: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeTargetDispositionRecord {
    pub reviewer_review_id: StableId,
    pub challenger_run_id: StableId,
    pub target_id: String,
    pub target_type: String,
    pub disposition: ChallengeTargetDisposition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeTaskLedger {
    requirements: ChallengeTaskRequirements,
    target_dispositions: BTreeMap<String, ChallengeTargetDisposition>,
    objections: SemanticRecordLedger,
    common_concerns: SemanticRecordLedger,
    complete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseTaskRequirements {
    pub objection_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseTaskLedger {
    requirements: ResponseTaskRequirements,
    responses: SemanticRecordLedger,
    replacement_recommendations: SemanticRecordLedger,
    complete: bool,
}

fn validate_reference(reference: &str) -> Result<(), TaskLedgerError> {
    if reference.trim().is_empty() {
        Err(TaskLedgerError::InvalidReference)
    } else {
        Ok(())
    }
}

fn reference_array<'a>(value: &'a Value, field: &str) -> Result<Vec<&'a str>, TaskLedgerError> {
    let Some(values) = value.get(field) else {
        return Ok(Vec::new());
    };
    values
        .as_array()
        .ok_or_else(|| TaskLedgerError::InvalidComponent(format!("{field} must be an array")))?
        .iter()
        .map(|value| {
            value.as_str().ok_or_else(|| {
                TaskLedgerError::InvalidComponent(format!("{field} entries must be strings"))
            })
        })
        .collect()
}

fn require_member(
    kind: &'static str,
    value: &str,
    allowed: &BTreeSet<String>,
) -> Result<(), TaskLedgerError> {
    if allowed.contains(value) {
        Ok(())
    } else {
        Err(TaskLedgerError::UnknownTask {
            kind,
            id: value.to_string(),
        })
    }
}

fn missing<'a, I>(required: &BTreeSet<String>, covered: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a String>,
{
    let covered = covered.into_iter().collect::<BTreeSet<_>>();
    required
        .iter()
        .filter(|value| !covered.contains(value))
        .cloned()
        .collect()
}

fn objection_target_id(
    records: &SemanticRecordLedger,
    objection_ref: &str,
) -> Result<String, TaskLedgerError> {
    records
        .get(objection_ref)
        .ok_or_else(|| TaskLedgerError::UnknownReference(objection_ref.to_string()))?
        .value
        .get("target_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or(TaskLedgerError::ImmutableField {
            reference: objection_ref.to_string(),
            field: "target_id",
        })
}

fn semantic_record_digest(value: &Value, active: bool) -> String {
    sha256_digest_bytes(
        &serde_json::to_vec(&(value, active)).expect("semantic task records are JSON-serializable"),
    )
}

fn ensure_unchanged_field(
    ledger: &SemanticRecordLedger,
    reference: &str,
    replacement: &Value,
    field: &'static str,
) -> Result<(), TaskLedgerError> {
    let current = ledger
        .get(reference)
        .ok_or_else(|| TaskLedgerError::UnknownReference(reference.to_string()))?;
    if current.value.get(field) != replacement.get(field) {
        return Err(TaskLedgerError::ImmutableField {
            reference: reference.to_string(),
            field,
        });
    }
    Ok(())
}

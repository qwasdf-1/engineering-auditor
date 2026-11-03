use std::collections::BTreeMap;

use auditor_core::sha256_digest_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod error;

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

fn validate_reference(reference: &str) -> Result<(), TaskLedgerError> {
    if reference.trim().is_empty() {
        Err(TaskLedgerError::InvalidReference)
    } else {
        Ok(())
    }
}

fn semantic_record_digest(value: &Value, active: bool) -> String {
    sha256_digest_bytes(
        &serde_json::to_vec(&(value, active)).expect("semantic task records are JSON-serializable"),
    )
}

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    ensure_unchanged_field, objection_target_id, ChallengeTargetDisposition,
    ChallengeTargetDispositionRecord, ChallengeTaskLedger, ChallengeTaskRequirements,
    SemanticRecordLedger, TaskCallProvenance, TaskLedgerError,
};

impl ChallengeTaskLedger {
    pub fn new(requirements: ChallengeTaskRequirements) -> Self {
        Self {
            requirements,
            target_dispositions: BTreeMap::new(),
            objections: SemanticRecordLedger::default(),
            common_concerns: SemanticRecordLedger::default(),
            complete: false,
        }
    }

    pub fn record_objection(
        &mut self,
        objection_ref: &str,
        target_type: &str,
        target_id: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.require_target(target_type, target_id)?;
        if matches!(
            self.target_dispositions.get(target_id),
            Some(ChallengeTargetDisposition::Cleared)
        ) {
            return Err(TaskLedgerError::TaskAlreadyCovered(target_id.to_string()));
        }
        self.objections.create(objection_ref, value, provenance)?;
        match self.target_dispositions.entry(target_id.to_string()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ChallengeTargetDisposition::Objections {
                    objection_refs: vec![objection_ref.to_string()],
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let ChallengeTargetDisposition::Objections { objection_refs } = entry.get_mut()
                else {
                    return Err(TaskLedgerError::TaskAlreadyCovered(target_id.to_string()));
                };
                objection_refs.push(objection_ref.to_string());
                objection_refs.sort();
                objection_refs.dedup();
            }
        }
        Ok(())
    }

    pub fn record_common_concern(
        &mut self,
        concern_ref: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.common_concerns.create(concern_ref, value, provenance)
    }

    pub fn revise_objection(
        &mut self,
        objection_ref: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        ensure_unchanged_field(&self.objections, objection_ref, &replacement, "target_type")?;
        ensure_unchanged_field(&self.objections, objection_ref, &replacement, "target_id")?;
        self.objections.revise(
            objection_ref,
            expected_revision,
            reason,
            replacement,
            provenance,
        )?;
        self.complete = false;
        Ok(())
    }

    pub fn revise_common_concern(
        &mut self,
        concern_ref: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.common_concerns.revise(
            concern_ref,
            expected_revision,
            reason,
            replacement,
            provenance,
        )?;
        self.complete = false;
        Ok(())
    }

    pub fn withdraw_objection(
        &mut self,
        objection_ref: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        let target_id = objection_target_id(&self.objections, objection_ref)?;
        self.objections
            .withdraw(objection_ref, expected_revision, reason, provenance)?;
        let mut remove_target = false;
        if let Some(ChallengeTargetDisposition::Objections { objection_refs }) =
            self.target_dispositions.get_mut(&target_id)
        {
            objection_refs.retain(|reference| reference != objection_ref);
            remove_target = objection_refs.is_empty();
        }
        if remove_target {
            self.target_dispositions.remove(&target_id);
        }
        self.complete = false;
        Ok(())
    }

    pub fn reopen_objection(
        &mut self,
        objection_ref: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        let target_id = objection_target_id(&self.objections, objection_ref)?;
        if matches!(
            self.target_dispositions.get(&target_id),
            Some(ChallengeTargetDisposition::Cleared)
        ) {
            return Err(TaskLedgerError::TaskAlreadyCovered(target_id));
        }
        self.objections
            .reopen(objection_ref, expected_revision, reason, provenance)?;
        match self.target_dispositions.entry(target_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ChallengeTargetDisposition::Objections {
                    objection_refs: vec![objection_ref.to_string()],
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let target_id = entry.key().clone();
                let ChallengeTargetDisposition::Objections { objection_refs } = entry.get_mut()
                else {
                    return Err(TaskLedgerError::TaskAlreadyCovered(target_id));
                };
                objection_refs.push(objection_ref.to_string());
                objection_refs.sort();
                objection_refs.dedup();
            }
        }
        self.complete = false;
        Ok(())
    }

    pub fn withdraw_common_concern(
        &mut self,
        concern_ref: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.common_concerns
            .withdraw(concern_ref, expected_revision, reason, provenance)?;
        self.complete = false;
        Ok(())
    }

    pub fn reopen_common_concern(
        &mut self,
        concern_ref: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.common_concerns
            .reopen(concern_ref, expected_revision, reason, provenance)?;
        self.complete = false;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<(), TaskLedgerError> {
        self.complete = true;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn objections(&self) -> &SemanticRecordLedger {
        &self.objections
    }

    pub fn common_concerns(&self) -> &SemanticRecordLedger {
        &self.common_concerns
    }

    pub fn target_dispositions(&self) -> &BTreeMap<String, ChallengeTargetDisposition> {
        &self.target_dispositions
    }

    pub fn disposition_records(
        &self,
        reviewer_review_id: &crate::StableId,
        challenger_run_id: &crate::StableId,
    ) -> Vec<ChallengeTargetDispositionRecord> {
        self.requirements
            .targets
            .iter()
            .map(
                |(target_id, target_type)| ChallengeTargetDispositionRecord {
                    reviewer_review_id: reviewer_review_id.clone(),
                    challenger_run_id: challenger_run_id.clone(),
                    target_id: target_id.clone(),
                    target_type: target_type.clone(),
                    disposition: self
                        .target_dispositions
                        .get(target_id)
                        .cloned()
                        .unwrap_or(ChallengeTargetDisposition::Cleared),
                },
            )
            .collect()
    }

    pub fn derived_status(&self) -> &'static str {
        if self.objections.values().any(|record| record.active) {
            "objections_raised"
        } else if self.common_concerns.values().any(|record| record.active) {
            "blocked"
        } else {
            "cleared"
        }
    }

    fn require_target(&self, target_type: &str, target_id: &str) -> Result<(), TaskLedgerError> {
        match self.requirements.targets.get(target_id) {
            Some(expected) if expected == target_type => Ok(()),
            _ => Err(TaskLedgerError::UnknownTask {
                kind: "challenge target",
                id: target_id.to_string(),
            }),
        }
    }
}

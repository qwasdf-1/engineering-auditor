use serde_json::Value;

use super::{
    ensure_unchanged_field, missing, require_member, ResponseTaskLedger, ResponseTaskRequirements,
    SemanticRecordLedger, TaskCallProvenance, TaskLedgerError,
};

impl ResponseTaskLedger {
    pub fn new(requirements: ResponseTaskRequirements) -> Self {
        Self {
            requirements,
            responses: SemanticRecordLedger::default(),
            replacement_recommendations: SemanticRecordLedger::default(),
            complete: false,
        }
    }

    pub fn record_replacement_recommendation(
        &mut self,
        reference: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.replacement_recommendations
            .create(reference, value, provenance)
    }

    pub fn record_response(
        &mut self,
        objection_id: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.require_objection(objection_id)?;
        self.validate_response_replacement(&value)
            .and_then(|()| self.responses.create(objection_id, value, provenance))
    }

    pub fn revise_replacement_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.replacement_recommendations.revise(
            reference,
            expected_revision,
            reason,
            replacement,
            provenance,
        )?;
        self.complete = false;
        Ok(())
    }

    pub fn revise_response(
        &mut self,
        objection_id: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        require_member(
            "challenge objection",
            objection_id,
            &self.requirements.objection_ids,
        )?;
        ensure_unchanged_field(&self.responses, objection_id, &replacement, "objection_id")?;
        self.validate_response_replacement(&replacement)
            .and_then(|()| {
                self.responses.revise(
                    objection_id,
                    expected_revision,
                    reason,
                    replacement,
                    provenance,
                )
            })
            .map(|()| self.complete = false)
    }

    pub fn withdraw_replacement_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.replacement_recommendations.withdraw(
            reference,
            expected_revision,
            reason,
            provenance,
        )?;
        self.complete = false;
        Ok(())
    }

    pub fn reopen_replacement_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.replacement_recommendations.reopen(
            reference,
            expected_revision,
            reason,
            provenance,
        )?;
        self.complete = false;
        Ok(())
    }

    pub fn withdraw_response(
        &mut self,
        objection_id: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.responses
            .withdraw(objection_id, expected_revision, reason, provenance)?;
        self.complete = false;
        Ok(())
    }

    pub fn reopen_response(
        &mut self,
        objection_id: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        let value = self
            .responses
            .get(objection_id)
            .ok_or_else(|| TaskLedgerError::UnknownReference(objection_id.to_string()))?
            .value
            .clone();
        self.validate_response_replacement(&value)?;
        self.responses
            .reopen(objection_id, expected_revision, reason, provenance)?;
        self.complete = false;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<(), TaskLedgerError> {
        let missing = missing(
            &self.requirements.objection_ids,
            self.responses
                .values()
                .filter(|record| record.active)
                .map(|record| &record.reference),
        );
        if !missing.is_empty() {
            return Err(TaskLedgerError::IncompleteTasks(
                missing
                    .into_iter()
                    .map(|id| format!("challenge_objection:{id}"))
                    .collect(),
            ));
        }
        for response in self.responses.values().filter(|record| record.active) {
            self.validate_response_replacement(&response.value)?;
        }
        self.complete = true;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn responses(&self) -> &SemanticRecordLedger {
        &self.responses
    }

    pub fn replacement_recommendations(&self) -> &SemanticRecordLedger {
        &self.replacement_recommendations
    }

    fn validate_response_replacement(&self, value: &Value) -> Result<(), TaskLedgerError> {
        if let Some(reference) = value
            .get("replacement_recommendation_ref")
            .and_then(Value::as_str)
        {
            if !self
                .replacement_recommendations
                .get(reference)
                .is_some_and(|record| record.active)
            {
                return Err(TaskLedgerError::UnknownReference(reference.to_string()));
            }
        }
        Ok(())
    }

    fn require_objection(&self, objection_id: &str) -> Result<(), TaskLedgerError> {
        require_member(
            "challenge objection",
            objection_id,
            &self.requirements.objection_ids,
        )
    }
}

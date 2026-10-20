use serde_json::Value;

use super::{
    ensure_unchanged_field, missing, reference_array, require_member, ReviewerTaskCompletion,
    ReviewerTaskLedger, ReviewerTaskRequirements, SemanticRecordLedger, TaskCallProvenance,
    TaskDeclineDisposition, TaskLedgerError,
};

mod dispositions;

impl ReviewerTaskLedger {
    pub fn new(requirements: ReviewerTaskRequirements) -> Self {
        Self {
            requirements,
            probe_answers: SemanticRecordLedger::default(),
            claims: SemanticRecordLedger::default(),
            recommendations: SemanticRecordLedger::default(),
            completion: None,
        }
    }

    pub fn answer_probe(
        &mut self,
        probe_id: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        require_member("probe", probe_id, &self.requirements.probe_ids)?;
        self.probe_answers.create(probe_id, value, provenance)
    }

    pub fn record_claim(
        &mut self,
        reference: &str,
        slot: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        require_member("claim slot", slot, &self.requirements.claim_slots)?;
        self.validate_new_claim_dependencies(&value)?;
        self.claims.create(reference, value, provenance)?;
        Ok(())
    }

    pub fn record_recommendation(
        &mut self,
        reference: &str,
        family: &str,
        value: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        require_member(
            "recommendation family",
            family,
            &self.requirements.recommendation_families,
        )?;
        self.validate_recommendation_premises(&value)?;
        self.recommendations.create(reference, value, provenance)?;
        Ok(())
    }

    pub fn complete(
        &mut self,
        preferred_recommendation_ref: Option<String>,
        mut unresolved_gaps: Vec<String>,
    ) -> Result<(), TaskLedgerError> {
        let missing_probes = missing(
            &self.requirements.probe_ids,
            self.probe_answers.values().map(|record| &record.reference),
        );
        let mut missing_tasks = missing_probes
            .into_iter()
            .map(|value| format!("probe:{value}"))
            .collect::<Vec<_>>();
        missing_tasks.sort();
        if !missing_tasks.is_empty() {
            return Err(TaskLedgerError::IncompleteTasks(missing_tasks));
        }
        if let Some(reference) = &preferred_recommendation_ref {
            if !self
                .recommendations
                .get(reference)
                .is_some_and(|record| record.active)
            {
                return Err(TaskLedgerError::UnknownReference(reference.clone()));
            }
        }
        unresolved_gaps.sort();
        unresolved_gaps.dedup();
        self.completion = Some(ReviewerTaskCompletion {
            preferred_recommendation_ref,
            unresolved_gaps,
        });
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.completion.is_some()
    }

    pub fn all_probe_answers_admitted(&self) -> bool {
        missing(
            &self.requirements.probe_ids,
            self.probe_answers.values().map(|record| &record.reference),
        )
        .is_empty()
    }

    pub fn needs_evidence_boundary_audit(&self) -> bool {
        let mut has_definitive = false;
        let mut has_uncertain = false;
        for record in self.probe_answers.values() {
            match record.value.get("answer").and_then(Value::as_str) {
                Some("yes" | "no") => has_definitive = true,
                Some("unknown" | "conflicting_evidence") => has_uncertain = true,
                _ => {}
            }
        }
        has_definitive && has_uncertain
    }

    pub fn probe_answers(&self) -> &SemanticRecordLedger {
        &self.probe_answers
    }

    pub fn claims(&self) -> &SemanticRecordLedger {
        &self.claims
    }

    pub fn recommendations(&self) -> &SemanticRecordLedger {
        &self.recommendations
    }

    pub fn completion(&self) -> Option<&ReviewerTaskCompletion> {
        self.completion.as_ref()
    }

    pub fn revise_probe_answer(
        &mut self,
        probe_id: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        ensure_unchanged_field(&self.probe_answers, probe_id, &replacement, "probe_id")?;
        self.probe_answers
            .revise(probe_id, expected_revision, reason, replacement, provenance)?;
        self.completion = None;
        Ok(())
    }

    pub fn revise_claim(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        ensure_unchanged_field(&self.claims, reference, &replacement, "slot")?;
        self.validate_revised_claim_dependencies(reference, &replacement)?;
        self.claims.revise(
            reference,
            expected_revision,
            reason,
            replacement,
            provenance,
        )?;
        self.completion = None;
        Ok(())
    }

    fn validate_new_claim_dependencies(&self, value: &Value) -> Result<(), TaskLedgerError> {
        self.require_active_claims(value, "dependency_claim_refs", "active earlier claim refs")
    }

    /// Every reference in `value[field]` names an active claim.
    fn require_active_claims(
        &self,
        value: &Value,
        field: &str,
        expected: &str,
    ) -> Result<(), TaskLedgerError> {
        for reference in reference_array(value, field)? {
            if !self
                .claims
                .get(reference)
                .is_some_and(|record| record.active)
            {
                return Err(TaskLedgerError::InvalidComponent(format!(
                    "{field} must name {expected}; `{reference}` is unavailable. {}",
                    active_claims_hint(&self.claims)
                )));
            }
        }
        Ok(())
    }

    fn validate_revised_claim_dependencies(
        &self,
        reference: &str,
        value: &Value,
    ) -> Result<(), TaskLedgerError> {
        for dependency in reference_array(value, "dependency_claim_refs")? {
            if !self.claims.is_active_before(dependency, reference) {
                return Err(TaskLedgerError::InvalidComponent(format!(
                    "dependency_claim_refs must name active earlier claim refs; `{dependency}` is not earlier than `{reference}`"
                )));
            }
        }
        Ok(())
    }

    fn validate_recommendation_premises(&self, value: &Value) -> Result<(), TaskLedgerError> {
        self.require_active_claims(value, "premise_claim_refs", "active claim refs")
    }

    pub fn revise_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        replacement: Value,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        ensure_unchanged_field(&self.recommendations, reference, &replacement, "family")?;
        self.validate_recommendation_premises(&replacement)?;
        self.recommendations.revise(
            reference,
            expected_revision,
            reason,
            replacement,
            provenance,
        )?;
        self.completion = None;
        Ok(())
    }

    pub fn withdraw_claim(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.claims
            .withdraw(reference, expected_revision, reason, provenance)?;
        self.completion = None;
        Ok(())
    }

    pub fn reopen_claim(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.claims
            .reopen(reference, expected_revision, reason, provenance)?;
        self.completion = None;
        Ok(())
    }

    pub fn withdraw_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.recommendations
            .withdraw(reference, expected_revision, reason, provenance)?;
        self.completion = None;
        Ok(())
    }

    pub fn reopen_recommendation(
        &mut self,
        reference: &str,
        expected_revision: u32,
        reason: &str,
        provenance: TaskCallProvenance<'_>,
    ) -> Result<(), TaskLedgerError> {
        self.recommendations
            .reopen(reference, expected_revision, reason, provenance)?;
        self.completion = None;
        Ok(())
    }
}

/// The claim refs a correction can point the model to, or how to create one
/// when none is active.
fn active_claims_hint(claims: &SemanticRecordLedger) -> String {
    let active = claims
        .values_in_creation_order()
        .into_iter()
        .filter(|record| record.active)
        .map(|record| format!("`{}`", record.reference))
        .collect::<Vec<_>>();
    if active.is_empty() {
        "No claim is active: record the premise with record_case_claim (a new claim_ref) before the call that cites it, in the same or an earlier turn.".to_string()
    } else {
        format!("Active claim refs: {}.", active.join(", "))
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn premise_corrections_name_the_active_claims_or_how_to_create_one() {
        let mut claims = SemanticRecordLedger::default();
        assert!(active_claims_hint(&claims).starts_with("No claim is active"));
        claims
            .create(
                "reuse-premise",
                serde_json::json!({}),
                TaskCallProvenance::new("request", "call"),
            )
            .unwrap();
        assert_eq!(
            active_claims_hint(&claims),
            "Active claim refs: `reuse-premise`."
        );
    }

    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;

    fn ledger() -> ReviewerTaskLedger {
        ReviewerTaskLedger::new(ReviewerTaskRequirements {
            probe_ids: BTreeSet::from(["probe-a".to_string(), "probe-b".to_string()]),
            claim_slots: BTreeSet::new(),
            recommendation_families: BTreeSet::new(),
        })
    }

    fn provenance() -> TaskCallProvenance<'static> {
        TaskCallProvenance::new("request", "call")
    }

    fn answer(probe_id: &str, answer: &str) -> Value {
        json!({
            "probe_id": probe_id,
            "answer": answer,
            "evidence_ids": [],
            "explanation": "Bounded test answer.",
            "limitations": []
        })
    }

    #[test]
    fn mixed_definitive_and_uncertain_answers_require_boundary_audit() {
        let mut ledger = ledger();
        ledger
            .answer_probe("probe-a", answer("probe-a", "yes"), provenance())
            .unwrap();
        assert!(!ledger.all_probe_answers_admitted());
        assert!(!ledger.needs_evidence_boundary_audit());

        ledger
            .answer_probe("probe-b", answer("probe-b", "unknown"), provenance())
            .unwrap();
        assert!(ledger.all_probe_answers_admitted());
        assert!(ledger.needs_evidence_boundary_audit());
    }

    #[test]
    fn uniformly_uncertain_answers_do_not_add_an_audit_turn() {
        let mut ledger = ledger();
        for probe_id in ["probe-a", "probe-b"] {
            ledger
                .answer_probe(probe_id, answer(probe_id, "unknown"), provenance())
                .unwrap();
        }
        assert!(ledger.all_probe_answers_admitted());
        assert!(!ledger.needs_evidence_boundary_audit());
    }
}

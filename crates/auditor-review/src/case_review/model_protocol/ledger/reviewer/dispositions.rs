use std::collections::BTreeSet;

use serde_json::Value;

use super::{ReviewerTaskLedger, TaskDeclineDisposition};

impl ReviewerTaskLedger {
    pub fn claim_slot_disposition_records(&self) -> Vec<TaskDeclineDisposition> {
        missing_dispositions(
            &self.claims,
            &self.requirements.claim_slots,
            "slot",
            "Reviewer completed without asserting an additional claim for this optional slot.",
        )
    }

    pub fn recommendation_family_disposition_records(&self) -> Vec<TaskDeclineDisposition> {
        missing_dispositions(
            &self.recommendations,
            &self.requirements.recommendation_families,
            "family",
            "Reviewer completed without asserting a recommendation in this optional family.",
        )
    }
}

fn missing_dispositions(
    records: &super::SemanticRecordLedger,
    required: &BTreeSet<String>,
    field: &str,
    reason: &str,
) -> Vec<TaskDeclineDisposition> {
    let active = records
        .values()
        .filter(|record| record.active)
        .filter_map(|record| record.value.get(field).and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    required
        .iter()
        .filter(|task| !active.contains(task.as_str()))
        .map(|task| TaskDeclineDisposition {
            task_id: task.clone(),
            disposition: None,
            reason: reason.to_string(),
        })
        .collect()
}

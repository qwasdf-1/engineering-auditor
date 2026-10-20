use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{EngineeringReviewV2, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionStatus {
    Accepted,
    Deferred,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationDisposition {
    pub recommendation_id: StableId,
    pub status: DispositionStatus,
    pub timestamp_unix_seconds: u64,
    pub operator_identifier: String,
    pub rationale: Option<String>,
    pub counterevidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DispositionLedger {
    pub schema_version: u32,
    pub review_id: StableId,
    pub dispositions: Vec<RecommendationDisposition>,
}

impl DispositionLedger {
    pub fn new(review_id: StableId) -> Self {
        Self {
            schema_version: 1,
            review_id,
            dispositions: Vec::new(),
        }
    }

    pub fn upsert(&mut self, disposition: RecommendationDisposition) -> Result<(), String> {
        validate_disposition(&disposition)?;
        if self
            .dispositions
            .iter()
            .any(|existing| existing.recommendation_id == disposition.recommendation_id)
        {
            return Err(
                "disposition ledger already contains a current decision for this recommendation"
                    .to_string(),
            );
        }
        self.dispositions.push(disposition);
        self.dispositions
            .sort_by(|left, right| left.recommendation_id.cmp(&right.recommendation_id));
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("disposition ledger is serializable")
    }
}

pub fn parse_disposition_ledger(text: &str) -> Result<DispositionLedger, String> {
    let ledger: DispositionLedger = serde_json::from_str(text)
        .map_err(|error| format!("invalid disposition ledger: {error}"))?;
    if ledger.schema_version != 1 || !ledger.review_id.as_str().starts_with("engineering-review:") {
        return Err("unsupported disposition ledger identity".to_string());
    }
    let mut ids = BTreeSet::new();
    for disposition in &ledger.dispositions {
        validate_disposition(disposition)?;
        if !ids.insert(&disposition.recommendation_id) {
            return Err("disposition ledger recommendation ids must be unique".to_string());
        }
    }
    Ok(ledger)
}

pub fn validate_disposition_coverage(
    review: &EngineeringReviewV2,
    ledger: &DispositionLedger,
) -> Result<(), String> {
    if ledger.review_id != review.review_id {
        return Err("disposition ledger targets a different review".to_string());
    }
    let material = crate::string_set(
        review
            .portfolio
            .accepted_recommendation_ids
            .iter()
            .map(|recommendation| recommendation.as_str()),
    );
    let disposed = crate::string_set(
        ledger
            .dispositions
            .iter()
            .map(|disposition| disposition.recommendation_id.as_str()),
    );
    if material != disposed {
        return Err(format!(
            "disposition ledger must cover exactly {} material recommendations",
            material.len()
        ));
    }
    Ok(())
}

fn validate_disposition(disposition: &RecommendationDisposition) -> Result<(), String> {
    if !disposition
        .recommendation_id
        .as_str()
        .starts_with("contextual-recommendation:")
        || disposition.operator_identifier.trim().is_empty()
        || disposition.timestamp_unix_seconds == 0
    {
        return Err(
            "disposition requires recommendation, timestamp, and operator identity".to_string(),
        );
    }
    if matches!(
        disposition.status,
        DispositionStatus::Deferred | DispositionStatus::Rejected
    ) && disposition.rationale.as_deref().is_none_or(str::is_empty)
        && disposition.counterevidence.is_empty()
    {
        return Err(
            "deferred and rejected dispositions require rationale or counterevidence".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_and_deferred_dispositions_require_review_evidence() {
        let review_id =
            StableId::from_identity("engineering-review", 2, &serde_json::json!({"test": true}))
                .unwrap();
        let recommendation_id = StableId::from_identity(
            "contextual-recommendation",
            1,
            &serde_json::json!({"test": true}),
        )
        .unwrap();
        let mut ledger = DispositionLedger::new(review_id);
        assert!(ledger
            .upsert(RecommendationDisposition {
                recommendation_id: recommendation_id.clone(),
                status: DispositionStatus::Rejected,
                timestamp_unix_seconds: 1,
                operator_identifier: "reviewer".to_string(),
                rationale: None,
                counterevidence: Vec::new(),
            })
            .is_err());
        ledger
            .upsert(RecommendationDisposition {
                recommendation_id: recommendation_id.clone(),
                status: DispositionStatus::Rejected,
                timestamp_unix_seconds: 1,
                operator_identifier: "reviewer".to_string(),
                rationale: Some("Not applicable here".to_string()),
                counterevidence: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            parse_disposition_ledger(&ledger.to_json())
                .unwrap()
                .dispositions
                .len(),
            1
        );
        assert!(ledger
            .upsert(RecommendationDisposition {
                recommendation_id,
                status: DispositionStatus::Accepted,
                timestamp_unix_seconds: 2,
                operator_identifier: "reviewer".to_string(),
                rationale: None,
                counterevidence: Vec::new(),
            })
            .is_err());
    }
}

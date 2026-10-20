use serde::{Deserialize, Serialize};

use crate::case_review::identity::case_draft_id;
use crate::{ClaimSlot, StableId};

pub const ENGINEERING_CLAIM_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineeringClaimKind {
    RootCause,
    Actionability,
    RecommendationPremise,
    ExpectedBenefit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Substantiated,
    PartiallySubstantiated,
    NotSubstantiated,
    Blocked,
    Contested,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimDraft {
    pub slot: ClaimSlot,
    pub kind: EngineeringClaimKind,
    pub proposition: String,
    pub status: ClaimStatus,
    pub supporting_probe_ids: Vec<StableId>,
    pub counter_probe_ids: Vec<StableId>,
    pub dependency_claim_ids: Vec<StableId>,
    pub assumptions: Vec<String>,
    pub confidence: ClaimConfidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringClaim {
    pub schema_version: u32,
    pub claim_id: StableId,
    pub case_id: StableId,
    #[serde(flatten)]
    pub content: ClaimDraft,
}

impl EngineeringClaim {
    pub fn from_draft(case_id: StableId, mut draft: ClaimDraft) -> Result<Self, String> {
        normalize(&mut draft.supporting_probe_ids);
        normalize(&mut draft.counter_probe_ids);
        normalize(&mut draft.dependency_claim_ids);
        draft.assumptions.sort();
        draft.assumptions.dedup();
        if draft.proposition.trim().is_empty()
            || (draft.supporting_probe_ids.is_empty() && draft.dependency_claim_ids.is_empty())
        {
            return Err(
                "claim requires a proposition and at least one supporting probe or dependency"
                    .to_string(),
            );
        }
        Self::from_normalized(case_id, draft)
    }

    fn from_normalized(case_id: StableId, content: ClaimDraft) -> Result<Self, String> {
        let claim_id = case_draft_id(
            "engineering-claim",
            ENGINEERING_CLAIM_SCHEMA_VERSION,
            &case_id,
            &content,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            schema_version: ENGINEERING_CLAIM_SCHEMA_VERSION,
            claim_id,
            case_id,
            content,
        })
    }
}

impl std::ops::Deref for EngineeringClaim {
    type Target = ClaimDraft;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

fn normalize(values: &mut Vec<StableId>) {
    values.sort();
    values.dedup();
}

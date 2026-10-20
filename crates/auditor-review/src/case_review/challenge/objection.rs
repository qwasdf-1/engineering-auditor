use serde::{Deserialize, Serialize};

use crate::{Materiality, StableId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeTargetType {
    Claim,
    Recommendation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectionReason {
    UnsupportedAssumption,
    IgnoredCounterevidence,
    OverclaimedBenefit,
    UnsafeLifecycleChange,
    OmittedAlternative,
    MaterialObjection,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeObjection {
    pub objection_id: StableId,
    pub target_type: ChallengeTargetType,
    pub target_id: StableId,
    pub reason: ObjectionReason,
    pub evidence_ids: Vec<String>,
    pub materiality: Materiality,
    pub required_resolution: String,
}

use serde::{Deserialize, Serialize};

use crate::StableId;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioDependency {
    pub prerequisite_id: StableId,
    pub dependent_id: StableId,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioBlockerKind {
    Recommendation,
    OperatorDecision,
    EvidenceGap,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioBlocker {
    pub blocked_recommendation_id: StableId,
    pub kind: PortfolioBlockerKind,
    pub blocking_recommendation_id: Option<StableId>,
    pub reason: String,
    pub evidence_ids: Vec<String>,
}

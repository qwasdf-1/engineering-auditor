use serde::{Deserialize, Serialize};

use crate::StableId;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioConflictKind {
    ActionConflict,
    DependencyCycle,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioConflict {
    pub kind: PortfolioConflictKind,
    pub recommendation_ids: Vec<StableId>,
    pub reason: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibleVariantGroup {
    pub recommendation_ids: Vec<StableId>,
    pub decision_dependencies: Vec<String>,
}

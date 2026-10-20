use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const REVIEW_BRIEF_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPriority {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewGoal {
    pub id: String,
    pub text: String,
    pub priority: GoalPriority,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Must,
    Prefer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConstraint {
    pub id: String,
    pub text: String,
    pub kind: ConstraintKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPosture {
    pub change_tolerance: RiskLevel,
    pub operational_criticality: RiskLevel,
    pub security_sensitivity: RiskLevel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentIntent {
    Requirement,
    Architecture,
    Decision,
    Operations,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDocument {
    pub path: String,
    pub intent: DocumentIntent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BriefBudgets {
    pub max_context_bytes: Option<usize>,
    pub max_document_bytes: Option<usize>,
    pub max_document_total_bytes: Option<usize>,
    pub max_lookup_requests: Option<usize>,
    pub max_lookup_hops: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewBrief {
    pub schema_version: u32,
    pub goals: Vec<ReviewGoal>,
    #[serde(default)]
    pub constraints: Vec<ReviewConstraint>,
    pub risk_posture: RiskPosture,
    #[serde(default)]
    pub documents: Vec<ReviewDocument>,
    #[serde(default)]
    pub excluded_domains: Vec<String>,
    #[serde(default)]
    pub budgets: BriefBudgets,
}

impl ReviewBrief {
    pub fn general() -> Self {
        Self {
            schema_version: REVIEW_BRIEF_SCHEMA_VERSION,
            goals: vec![ReviewGoal {
                id: "goal:engineering-review".to_string(),
                text: "Identify evidence-backed engineering risks and remediation tradeoffs."
                    .to_string(),
                priority: GoalPriority::High,
            }],
            constraints: Vec::new(),
            risk_posture: RiskPosture {
                change_tolerance: RiskLevel::Moderate,
                operational_criticality: RiskLevel::Moderate,
                security_sensitivity: RiskLevel::Moderate,
            },
            documents: Vec::new(),
            excluded_domains: Vec::new(),
            budgets: BriefBudgets::default(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let brief: Self =
            serde_json::from_str(text).map_err(|error| format!("invalid review brief: {error}"))?;
        brief.validate()?;
        Ok(brief)
    }

    pub fn validate(&self) -> Result<(), String> {
        let goals_and_constraints = self.goals.iter().map(|goal| (&goal.id, &goal.text)).chain(
            self.constraints
                .iter()
                .map(|constraint| (&constraint.id, &constraint.text)),
        );
        let mut ids = BTreeSet::new();
        let ids_valid = goals_and_constraints
            .into_iter()
            .all(|(id, text)| !id.is_empty() && !text.trim().is_empty() && ids.insert(id.as_str()));
        let budgets = [
            self.budgets.max_context_bytes,
            self.budgets.max_document_bytes,
            self.budgets.max_document_total_bytes,
            self.budgets.max_lookup_requests,
            self.budgets.max_lookup_hops,
        ];
        let checks = [
            (
                self.schema_version != REVIEW_BRIEF_SCHEMA_VERSION,
                "unsupported review brief schema version",
            ),
            (
                self.goals.is_empty(),
                "review brief requires at least one goal",
            ),
            (
                !ids_valid,
                "review brief goal and constraint ids must be unique and nonempty",
            ),
            (
                !unique_nonempty(self.documents.iter().map(|document| document.path.as_str())),
                "review brief document paths must be unique and nonempty",
            ),
            (
                !unique_nonempty(self.excluded_domains.iter().map(String::as_str)),
                "review brief excluded domains must be unique and nonempty",
            ),
            (
                budgets.into_iter().flatten().any(|value| value == 0),
                "review brief budget overrides must be positive",
            ),
        ];
        checks
            .into_iter()
            .find(|(violated, _)| *violated)
            .map_or(Ok(()), |(_, message)| Err(message.to_string()))
    }
}

/// Whether every value is nonempty and none repeats.
fn unique_nonempty<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .all(|value| !value.is_empty() && seen.insert(value))
}

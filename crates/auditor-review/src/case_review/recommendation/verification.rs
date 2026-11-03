use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationStep {
    pub method: String,
    pub expected_observation: String,
    pub evidence_ids: Vec<String>,
}

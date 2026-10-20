use std::collections::BTreeSet;
use std::fmt;

use auditor_core::sha256_digest_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::v2::ReviewRole;
use super::ToolSpec;
use crate::{EvidenceLookupRequestKind, StableId};

pub const TOOL_REQUEST_CASE_EVIDENCE: &str = "request_case_evidence";
pub const TOOL_ANSWER_CASE_PROBE: &str = "answer_case_probe";
pub const TOOL_RECORD_CASE_CLAIM: &str = "record_case_claim";
pub const TOOL_RECORD_CASE_RECOMMENDATION: &str = "record_case_recommendation";
pub const TOOL_COMPLETE_CASE_REVIEW: &str = "complete_case_review";
pub const TOOL_REVISE_CASE_PROBE_ANSWER: &str = "revise_case_probe_answer";
pub const TOOL_REVISE_CASE_CLAIM: &str = "revise_case_claim";
pub const TOOL_REVISE_CASE_RECOMMENDATION: &str = "revise_case_recommendation";
pub const TOOL_WITHDRAW_CASE_CLAIM: &str = "withdraw_case_claim";
pub const TOOL_REOPEN_CASE_CLAIM: &str = "reopen_case_claim";
pub const TOOL_WITHDRAW_CASE_RECOMMENDATION: &str = "withdraw_case_recommendation";
pub const TOOL_REOPEN_CASE_RECOMMENDATION: &str = "reopen_case_recommendation";
pub const TOOL_RECORD_CASE_OBJECTION: &str = "record_case_objection";
pub const TOOL_RECORD_COMMON_CONCERN: &str = "record_common_concern";
pub const TOOL_COMPLETE_CASE_CHALLENGE: &str = "complete_case_challenge";
pub const TOOL_REVISE_CASE_OBJECTION: &str = "revise_case_objection";
pub const TOOL_WITHDRAW_CASE_OBJECTION: &str = "withdraw_case_objection";
pub const TOOL_REOPEN_CASE_OBJECTION: &str = "reopen_case_objection";
pub const TOOL_REVISE_COMMON_CONCERN: &str = "revise_common_concern";
pub const TOOL_WITHDRAW_COMMON_CONCERN: &str = "withdraw_common_concern";
pub const TOOL_REOPEN_COMMON_CONCERN: &str = "reopen_common_concern";
pub const TOOL_RECORD_REPLACEMENT_RECOMMENDATION: &str = "record_replacement_recommendation";
pub const TOOL_RECORD_OBJECTION_RESPONSE: &str = "record_objection_response";
pub const TOOL_COMPLETE_CHALLENGE_RESPONSE: &str = "complete_challenge_response";
pub const TOOL_REVISE_REPLACEMENT_RECOMMENDATION: &str = "revise_replacement_recommendation";
pub const TOOL_WITHDRAW_REPLACEMENT_RECOMMENDATION: &str = "withdraw_replacement_recommendation";
pub const TOOL_REOPEN_REPLACEMENT_RECOMMENDATION: &str = "reopen_replacement_recommendation";
pub const TOOL_REVISE_OBJECTION_RESPONSE: &str = "revise_objection_response";
pub const TOOL_WITHDRAW_OBJECTION_RESPONSE: &str = "withdraw_objection_response";
pub const TOOL_REOPEN_OBJECTION_RESPONSE: &str = "reopen_objection_response";
pub const TOOL_EMIT_QUALIFICATION_NONCE: &str = "emit_qualification_nonce";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CaseTool {
    RequestCaseEvidence,
    AnswerCaseProbe,
    RecordCaseClaim,
    RecordCaseRecommendation,
    CompleteCaseReview,
    ReviseCaseProbeAnswer,
    ReviseCaseClaim,
    ReviseCaseRecommendation,
    WithdrawCaseClaim,
    ReopenCaseClaim,
    WithdrawCaseRecommendation,
    ReopenCaseRecommendation,
    RecordCaseObjection,
    RecordCommonConcern,
    CompleteCaseChallenge,
    ReviseCaseObjection,
    WithdrawCaseObjection,
    ReopenCaseObjection,
    ReviseCommonConcern,
    WithdrawCommonConcern,
    ReopenCommonConcern,
    RecordReplacementRecommendation,
    RecordObjectionResponse,
    CompleteChallengeResponse,
    ReviseReplacementRecommendation,
    WithdrawReplacementRecommendation,
    ReopenReplacementRecommendation,
    ReviseObjectionResponse,
    WithdrawObjectionResponse,
    ReopenObjectionResponse,
    EmitQualificationNonce,
}

#[derive(Clone, Copy)]
struct CaseToolMetadata {
    name: &'static str,
    description: &'static str,
}

const CASE_TOOL_METADATA: [CaseToolMetadata; 31] = [
    CaseToolMetadata {
        name: TOOL_REQUEST_CASE_EVIDENCE,
        description: "Request one case-bounded evidence lookup. A successful read returns every byte of each addressed source item; oversized items fail explicitly and are never truncated.",
    },
    CaseToolMetadata {
        name: TOOL_ANSWER_CASE_PROBE,
        description: "Answer one unresolved case question. Use the question's probe_id as probe_id and cite only its admissible evidence IDs.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_CASE_CLAIM,
        description: "Create one claim under a declared claim slot. A claim_ref is create-only.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_CASE_RECOMMENDATION,
        description: "Create one complete typed recommendation. Omit no_alternative_reason when alternatives are present. A recommendation_ref is create-only; use revise_case_recommendation for a later full replacement.",
    },
    CaseToolMetadata {
        name: TOOL_COMPLETE_CASE_REVIEW,
        description: "Complete the reviewer task after every model-eligible probe is covered. EA derives the typed case conclusion from the admitted probe answers; omitted supporting claim slots and recommendation families are host-recorded as not asserted.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_CASE_PROBE_ANSWER,
        description: "Replace one existing probe answer at its current revision with a complete answer.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_CASE_CLAIM,
        description: "Replace one existing claim at its current revision with a complete claim.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_CASE_RECOMMENDATION,
        description: "Replace one existing recommendation at its current revision with a complete recommendation.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_CASE_CLAIM,
        description: "Withdraw one active claim at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_CASE_CLAIM,
        description: "Reopen one withdrawn claim at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_CASE_RECOMMENDATION,
        description: "Withdraw one active recommendation at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_CASE_RECOMMENDATION,
        description: "Reopen one withdrawn recommendation at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_CASE_OBJECTION,
        description: "Create one typed objection against an admitted claim or recommendation target.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_COMMON_CONCERN,
        description: "Record one concern that affects the challenge as a whole rather than one target.",
    },
    CaseToolMetadata {
        name: TOOL_COMPLETE_CASE_CHALLENGE,
        description: "Complete the challenge. EA clears every admitted target without an active objection and publishes all typed target dispositions.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_CASE_OBJECTION,
        description: "Replace one existing challenge objection at its current revision with a complete objection.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_CASE_OBJECTION,
        description: "Withdraw one active challenge objection at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_CASE_OBJECTION,
        description: "Reopen one withdrawn challenge objection at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_COMMON_CONCERN,
        description: "Replace one existing common concern at its current revision with a complete concern.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_COMMON_CONCERN,
        description: "Withdraw one active common concern at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_COMMON_CONCERN,
        description: "Reopen one withdrawn common concern at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_REPLACEMENT_RECOMMENDATION,
        description: "Create one complete replacement recommendation for a later narrow response.",
    },
    CaseToolMetadata {
        name: TOOL_RECORD_OBJECTION_RESPONSE,
        description: "Address one bound challenge objection exactly once.",
    },
    CaseToolMetadata {
        name: TOOL_COMPLETE_CHALLENGE_RESPONSE,
        description: "Complete the response after every bound objection is addressed.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_REPLACEMENT_RECOMMENDATION,
        description: "Replace one existing replacement recommendation at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_REPLACEMENT_RECOMMENDATION,
        description: "Withdraw one active replacement recommendation at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_REPLACEMENT_RECOMMENDATION,
        description: "Reopen one withdrawn replacement recommendation at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REVISE_OBJECTION_RESPONSE,
        description: "Replace one existing objection response at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_WITHDRAW_OBJECTION_RESPONSE,
        description: "Withdraw one active objection response at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_REOPEN_OBJECTION_RESPONSE,
        description: "Reopen one withdrawn objection response at its current revision.",
    },
    CaseToolMetadata {
        name: TOOL_EMIT_QUALIFICATION_NONCE,
        description: "Emit the bound qualification response.",
    },
];

impl CaseTool {
    pub const fn name(self) -> &'static str {
        CASE_TOOL_METADATA[self as usize].name
    }

    pub const fn description(self) -> &'static str {
        CASE_TOOL_METADATA[self as usize].description
    }

    pub fn parameters(self) -> Value {
        super::case_tool_schema::parameters(self)
    }

    pub fn spec(self, strict: bool) -> ToolSpec {
        ToolSpec {
            name: self.name(),
            description: self.description(),
            parameters: self.parameters(),
            strict,
        }
    }
}

pub fn challenge_response_tools(strict: bool, revision_tools_allowed: bool) -> Vec<ToolSpec> {
    let mut tools = vec![
        CaseTool::RecordReplacementRecommendation,
        CaseTool::RecordObjectionResponse,
        CaseTool::CompleteChallengeResponse,
    ];
    if revision_tools_allowed {
        tools.extend([
            CaseTool::ReviseReplacementRecommendation,
            CaseTool::WithdrawReplacementRecommendation,
            CaseTool::ReopenReplacementRecommendation,
            CaseTool::ReviseObjectionResponse,
            CaseTool::WithdrawObjectionResponse,
            CaseTool::ReopenObjectionResponse,
        ]);
    }
    tools.into_iter().map(|tool| tool.spec(strict)).collect()
}

pub fn case_tools_for_role(
    role: ReviewRole,
    strict: bool,
    evidence_requests_allowed: bool,
    revision_tools_allowed: bool,
) -> Vec<ToolSpec> {
    let task_tools: &[CaseTool] = match role {
        ReviewRole::Challenger => &[
            CaseTool::RecordCaseObjection,
            CaseTool::RecordCommonConcern,
            CaseTool::CompleteCaseChallenge,
        ],
        ReviewRole::SingleReviewer
        | ReviewRole::ReviewerA
        | ReviewRole::ReviewerB
        | ReviewRole::ReviewerC
        | ReviewRole::AlternativeComparator => &[
            CaseTool::AnswerCaseProbe,
            CaseTool::RecordCaseClaim,
            CaseTool::RecordCaseRecommendation,
            CaseTool::CompleteCaseReview,
        ],
    };
    let mut tools = Vec::new();
    if evidence_requests_allowed {
        tools.push(CaseTool::RequestCaseEvidence.spec(strict));
    }
    tools.extend(task_tools.iter().copied().map(|tool| tool.spec(strict)));
    if revision_tools_allowed {
        let revision_tools: &[CaseTool] = if role == ReviewRole::Challenger {
            &[
                CaseTool::ReviseCaseObjection,
                CaseTool::WithdrawCaseObjection,
                CaseTool::ReopenCaseObjection,
                CaseTool::ReviseCommonConcern,
                CaseTool::WithdrawCommonConcern,
                CaseTool::ReopenCommonConcern,
            ]
        } else {
            &[
                CaseTool::ReviseCaseProbeAnswer,
                CaseTool::ReviseCaseClaim,
                CaseTool::ReviseCaseRecommendation,
                CaseTool::WithdrawCaseClaim,
                CaseTool::ReopenCaseClaim,
                CaseTool::WithdrawCaseRecommendation,
                CaseTool::ReopenCaseRecommendation,
            ]
        };
        tools.extend(revision_tools.iter().copied().map(|tool| tool.spec(strict)));
    }
    tools
}

pub fn case_tool_schema_digest() -> String {
    let schemas = v2_protocol_tools()
        .iter()
        .copied()
        .map(|tool| (tool.name(), tool.parameters()))
        .collect::<Vec<_>>();
    sha256_digest_bytes(
        serde_json::to_string(&schemas)
            .expect("case tool schemas are serializable")
            .as_bytes(),
    )
}

fn v2_protocol_tools() -> &'static [CaseTool] {
    &[
        CaseTool::RequestCaseEvidence,
        CaseTool::AnswerCaseProbe,
        CaseTool::RecordCaseClaim,
        CaseTool::RecordCaseRecommendation,
        CaseTool::CompleteCaseReview,
        CaseTool::ReviseCaseProbeAnswer,
        CaseTool::ReviseCaseClaim,
        CaseTool::ReviseCaseRecommendation,
        CaseTool::WithdrawCaseClaim,
        CaseTool::ReopenCaseClaim,
        CaseTool::WithdrawCaseRecommendation,
        CaseTool::ReopenCaseRecommendation,
        CaseTool::RecordCaseObjection,
        CaseTool::RecordCommonConcern,
        CaseTool::CompleteCaseChallenge,
        CaseTool::ReviseCaseObjection,
        CaseTool::WithdrawCaseObjection,
        CaseTool::ReopenCaseObjection,
        CaseTool::ReviseCommonConcern,
        CaseTool::WithdrawCommonConcern,
        CaseTool::ReopenCommonConcern,
        CaseTool::RecordReplacementRecommendation,
        CaseTool::RecordObjectionResponse,
        CaseTool::CompleteChallengeResponse,
        CaseTool::ReviseReplacementRecommendation,
        CaseTool::WithdrawReplacementRecommendation,
        CaseTool::ReopenReplacementRecommendation,
        CaseTool::ReviseObjectionResponse,
        CaseTool::WithdrawObjectionResponse,
        CaseTool::ReopenObjectionResponse,
        CaseTool::EmitQualificationNonce,
    ]
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseEvidenceRequest {
    pub case_id: StableId,
    pub probe_id: StableId,
    pub request_kind: EvidenceLookupRequestKind,
    pub target_id: String,
    pub decision_relevance: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseProviderTurn {
    pub evidence_requests: Vec<CaseEvidenceRequest>,
    pub case_review: Option<Value>,
    pub challenge: Option<Value>,
}

pub struct CaseToolBoundary<'a> {
    pub case_id: &'a StableId,
    pub probe_ids: &'a BTreeSet<StableId>,
    pub target_ids: &'a BTreeSet<String>,
}

impl CaseProviderTurn {
    pub fn validate(&self, boundary: &CaseToolBoundary<'_>) -> Result<(), CaseToolError> {
        self.validate_shape()?;
        if self.evidence_requests.iter().any(|request| {
            &request.case_id != boundary.case_id
                || !boundary.probe_ids.contains(&request.probe_id)
                || !boundary.target_ids.contains(&request.target_id)
                || request.decision_relevance.trim().is_empty()
        }) {
            return Err(CaseToolError::OutOfCaseRequest);
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), CaseToolError> {
        let final_count =
            usize::from(self.case_review.is_some()) + usize::from(self.challenge.is_some());
        if final_count > 1 || (final_count == 1 && !self.evidence_requests.is_empty()) {
            return Err(CaseToolError::MixedTurn);
        }
        if final_count == 0 && self.evidence_requests.is_empty() {
            return Err(CaseToolError::InvalidOutput);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseToolError {
    InvalidOutput,
    MixedTurn,
    OutOfCaseRequest,
}

impl fmt::Display for CaseToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOutput => "case provider output is invalid",
            Self::MixedTurn => "evidence requests and final output cannot share one turn",
            Self::OutOfCaseRequest => "case evidence request exceeds its typed boundary",
        })
    }
}

impl std::error::Error for CaseToolError {}

#[cfg(test)]
mod task_schema_tests {
    use super::*;

    #[test]
    fn reviewer_and_response_task_schemas_encode_absence_by_omission() {
        let recommendation = CaseTool::RecordCaseRecommendation.parameters();
        assert_eq!(
            recommendation["properties"]["no_alternative_reason"]["type"],
            "string"
        );
        assert!(!required_fields(&recommendation).contains("no_alternative_reason"));

        let completion = CaseTool::CompleteCaseReview.parameters();
        assert_eq!(
            completion["properties"]["preferred_recommendation_ref"]["type"],
            "string"
        );
        assert!(!required_fields(&completion).contains("preferred_recommendation_ref"));

        let response = CaseTool::RecordObjectionResponse.parameters();
        assert_eq!(
            response["properties"]["replacement_recommendation_ref"]["type"],
            "string"
        );
        assert!(!required_fields(&response).contains("replacement_recommendation_ref"));
    }

    #[test]
    fn metadata_and_schema_partitions_cover_every_protocol_tool_in_enum_order() {
        let mut names = BTreeSet::new();
        for (index, tool) in v2_protocol_tools().iter().copied().enumerate() {
            assert_eq!(tool as usize, index);
            assert!(names.insert(tool.name()));
            assert!(!tool.description().is_empty());
            assert_ne!(tool.parameters(), serde_json::json!({"not": {}}));
        }
        assert_eq!(names.len(), CASE_TOOL_METADATA.len());
    }

    fn required_fields(schema: &Value) -> BTreeSet<&str> {
        schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    }
}

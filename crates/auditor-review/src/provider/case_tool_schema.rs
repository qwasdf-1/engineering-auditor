use serde_json::{json, Value};

use super::case_tool::CaseTool;

pub(super) fn parameters(tool: CaseTool) -> Value {
    match tool {
        CaseTool::RequestCaseEvidence => evidence_request_schema(),
        CaseTool::AnswerCaseProbe
        | CaseTool::RecordCaseClaim
        | CaseTool::RecordCaseRecommendation
        | CaseTool::CompleteCaseReview
        | CaseTool::ReviseCaseProbeAnswer
        | CaseTool::ReviseCaseClaim
        | CaseTool::ReviseCaseRecommendation
        | CaseTool::WithdrawCaseClaim
        | CaseTool::ReopenCaseClaim
        | CaseTool::WithdrawCaseRecommendation
        | CaseTool::ReopenCaseRecommendation => reviewer_parameters(tool),
        CaseTool::RecordCaseObjection
        | CaseTool::RecordCommonConcern
        | CaseTool::CompleteCaseChallenge
        | CaseTool::ReviseCaseObjection
        | CaseTool::WithdrawCaseObjection
        | CaseTool::ReopenCaseObjection
        | CaseTool::ReviseCommonConcern
        | CaseTool::WithdrawCommonConcern
        | CaseTool::ReopenCommonConcern => challenge_parameters(tool),
        CaseTool::RecordReplacementRecommendation
        | CaseTool::RecordObjectionResponse
        | CaseTool::CompleteChallengeResponse
        | CaseTool::ReviseReplacementRecommendation
        | CaseTool::WithdrawReplacementRecommendation
        | CaseTool::ReopenReplacementRecommendation
        | CaseTool::ReviseObjectionResponse
        | CaseTool::WithdrawObjectionResponse
        | CaseTool::ReopenObjectionResponse => response_parameters(tool),
        CaseTool::EmitQualificationNonce => qualification_schema(),
    }
}

fn reviewer_parameters(tool: CaseTool) -> Value {
    match tool {
        CaseTool::AnswerCaseProbe => review_item_schema("model_resolved_probes"),
        CaseTool::RecordCaseClaim => review_item_schema("claims"),
        CaseTool::RecordCaseRecommendation => review_item_schema("recommendations"),
        CaseTool::CompleteCaseReview => review_completion_schema(),
        CaseTool::ReviseCaseProbeAnswer => {
            revision_schema("probe_id", review_item_schema("model_resolved_probes"))
        }
        CaseTool::ReviseCaseClaim => revision_schema("claim_ref", review_item_schema("claims")),
        CaseTool::ReviseCaseRecommendation => {
            revision_schema("recommendation_ref", review_item_schema("recommendations"))
        }
        CaseTool::WithdrawCaseClaim | CaseTool::ReopenCaseClaim => transition_schema("claim_ref"),
        CaseTool::WithdrawCaseRecommendation | CaseTool::ReopenCaseRecommendation => {
            transition_schema("recommendation_ref")
        }
        _ => partition_mismatch_schema(),
    }
}

fn challenge_parameters(tool: CaseTool) -> Value {
    match tool {
        CaseTool::RecordCaseObjection => objection_schema(),
        CaseTool::RecordCommonConcern => common_concern_schema(),
        CaseTool::CompleteCaseChallenge => empty_object_schema(),
        CaseTool::ReviseCaseObjection => revision_schema("objection_ref", objection_schema()),
        CaseTool::WithdrawCaseObjection | CaseTool::ReopenCaseObjection => {
            transition_schema("objection_ref")
        }
        CaseTool::ReviseCommonConcern => revision_schema("concern_ref", common_concern_schema()),
        CaseTool::WithdrawCommonConcern | CaseTool::ReopenCommonConcern => {
            transition_schema("concern_ref")
        }
        _ => partition_mismatch_schema(),
    }
}

fn response_parameters(tool: CaseTool) -> Value {
    match tool {
        CaseTool::RecordReplacementRecommendation => replacement_recommendation_schema(),
        CaseTool::RecordObjectionResponse => objection_response_schema(),
        CaseTool::CompleteChallengeResponse => empty_object_schema(),
        CaseTool::ReviseReplacementRecommendation => {
            revision_schema("recommendation_ref", replacement_recommendation_schema())
        }
        CaseTool::WithdrawReplacementRecommendation | CaseTool::ReopenReplacementRecommendation => {
            transition_schema("recommendation_ref")
        }
        CaseTool::ReviseObjectionResponse => {
            revision_schema("objection_id", objection_response_schema())
        }
        CaseTool::WithdrawObjectionResponse | CaseTool::ReopenObjectionResponse => {
            transition_schema("objection_id")
        }
        _ => partition_mismatch_schema(),
    }
}

fn evidence_request_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "case_id": {"type": "string", "pattern": "^engineering-case:[0-9a-f]{64}$"},
            "probe_id": {"type": "string", "pattern": "^[a-z0-9-]+:[0-9a-f]{64}$"},
            "request_kind": {"enum": ["finding_source", "symbol_neighborhood"]},
            "target_id": {"type": "string", "minLength": 1},
            "decision_relevance": {"type": "string", "minLength": 1}
        },
        "required": ["case_id", "probe_id", "request_kind", "target_id", "decision_relevance"],
        "type": "object"
    })
}

fn review_completion_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "preferred_recommendation_ref": {
                "pattern": "^[a-z][a-z0-9_-]{0,63}$",
                "type": "string"
            },
            "unresolved_gaps": {
                "items": {"type": "string", "minLength": 1},
                "type": "array"
            }
        },
        "required": ["unresolved_gaps"],
        "type": "object"
    })
}

fn objection_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "objection_ref": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"},
            "target_type": {"enum": ["claim", "recommendation"]},
            "target_id": {"type": "string", "minLength": 1},
            "reason": {"enum": [
                "unsupported_assumption", "ignored_counterevidence", "overclaimed_benefit",
                "unsafe_lifecycle_change", "omitted_alternative", "material_objection"
            ]},
            "evidence_ids": {"items": {"type": "string", "minLength": 1}, "type": "array"},
            "materiality": {"enum": ["low", "medium", "high"]},
            "required_resolution": {"type": "string", "minLength": 1}
        },
        "required": [
            "objection_ref", "target_type", "target_id", "reason", "evidence_ids",
            "materiality", "required_resolution"
        ],
        "type": "object"
    })
}

fn common_concern_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "concern_ref": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"},
            "concern": {"type": "string", "minLength": 1}
        },
        "required": ["concern_ref", "concern"],
        "type": "object"
    })
}

fn replacement_recommendation_schema() -> Value {
    let mut schema = challenge_replacement_schema();
    if let Some(required) = schema["required"].as_array_mut() {
        required.push(json!("recommendation_ref"));
    }
    schema["properties"]["recommendation_ref"] =
        json!({"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"});
    schema
}

fn objection_response_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "objection_id": {"pattern": "^challenge-objection:[0-9a-f]{64}$", "type": "string"},
            "action": {"enum": [
                "concede", "narrow", "maintain_with_evidence", "withdraw", "mark_unresolved"
            ]},
            "rationale": {"type": "string", "minLength": 1},
            "evidence_probe_ids": {
                "items": {"pattern": "^[a-z0-9-]+:[0-9a-f]{64}$", "type": "string"},
                "type": "array",
                "uniqueItems": true
            },
            "replacement_recommendation_ref": {
                "pattern": "^[a-z][a-z0-9_-]{0,63}$",
                "type": "string"
            }
        },
        "required": ["objection_id", "action", "rationale", "evidence_probe_ids"],
        "type": "object"
    })
}

fn qualification_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "nonce": {"type": "string", "minLength": 1},
            "classification": {
                "enum": ["positive", "negative", "unknown", "conflicting_evidence", "forbidden_redesign"]
            },
            "native_array_markers": {
                "items": {"enum": ["native-array-one", "native-array-two"], "type": "string"},
                "maxItems": 2,
                "minItems": 2,
                "type": "array",
                "uniqueItems": true
            }
        },
        "required": ["nonce", "classification", "native_array_markers"],
        "type": "object"
    })
}

fn empty_object_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {},
        "required": [],
        "type": "object"
    })
}

fn partition_mismatch_schema() -> Value {
    // The public dispatcher exhaustively assigns every CaseTool to one family.
    // Keep family helpers fail-closed if that internal invariant is ever broken.
    json!({"not": {}})
}

fn review_item_schema(field: &str) -> Value {
    let schema = crate::case_review::deliberation::reviewer_case_review_payload_schema();
    schema["properties"]["review"]["properties"][field]["items"].clone()
}

fn challenge_replacement_schema() -> Value {
    let schema = crate::case_review::challenge_response_payload_schema();
    schema["properties"]["responses"]["items"]["properties"]["replacement_recommendation"]["anyOf"]
        [0]
    .clone()
}

fn revision_schema(reference_field: &str, replacement_schema: Value) -> Value {
    let reference_schema = replacement_schema["properties"][reference_field].clone();
    json!({
        "additionalProperties": false,
        "properties": {
            reference_field: reference_schema,
            "expected_revision": {"type": "integer", "minimum": 1},
            "reason": {"type": "string", "minLength": 1},
            "replacement": replacement_schema
        },
        "required": [reference_field, "expected_revision", "reason", "replacement"],
        "type": "object"
    })
}

fn transition_schema(reference_field: &str) -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            reference_field: {
                "pattern": "^[a-z][a-z0-9_-]{0,63}$",
                "type": "string"
            },
            "expected_revision": {"type": "integer", "minimum": 1},
            "reason": {"type": "string", "minLength": 1}
        },
        "required": [reference_field, "expected_revision", "reason"],
        "type": "object"
    })
}

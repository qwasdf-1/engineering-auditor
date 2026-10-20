use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{ChallengeError, ChallengeRecord};
use crate::{
    ContextualRecommendation, EngineeringCase, RecommendationDraft, ReviewerCaseReview, StableId,
};

pub const CHALLENGE_RESPONSE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseAction {
    Concede,
    Narrow,
    MaintainWithEvidence,
    Withdraw,
    MarkUnresolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectionResponse {
    pub objection_id: StableId,
    pub action: ResponseAction,
    pub rationale: String,
    pub evidence_probe_ids: Vec<StableId>,
    pub replacement_recommendation: Option<ContextualRecommendation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeResponsePayload {
    pub responses: Vec<ObjectionResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeResponse {
    pub schema_version: u32,
    pub response_id: StableId,
    pub reviewer_run_id: StableId,
    pub case_id: StableId,
    pub challenge_id: StableId,
    pub responses: Vec<ObjectionResponse>,
}

pub struct ChallengeResponseAdmission<'a> {
    reviewer_run_id: StableId,
    case: &'a EngineeringCase,
    challenge: &'a ChallengeRecord,
    allowed_claim_ids: BTreeSet<StableId>,
    allowed_probe_ids: &'a BTreeSet<StableId>,
}

impl<'a> ChallengeResponseAdmission<'a> {
    pub fn for_review(
        reviewer_run_id: StableId,
        case: &'a EngineeringCase,
        challenge: &'a ChallengeRecord,
        review: &ReviewerCaseReview,
        allowed_probe_ids: &'a BTreeSet<StableId>,
    ) -> Self {
        Self {
            reviewer_run_id,
            case,
            challenge,
            allowed_claim_ids: review
                .claims
                .iter()
                .map(|claim| claim.claim_id.clone())
                .collect(),
            allowed_probe_ids,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectionResponseWire {
    objection_id: StableId,
    action: ResponseAction,
    rationale: String,
    evidence_probe_ids: Vec<StableId>,
    replacement_recommendation: Option<RecommendationDraft>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeResponseWirePayload {
    responses: Vec<ObjectionResponseWire>,
}

impl ChallengeResponse {
    pub(crate) fn admit_wire(
        value: Value,
        context: ChallengeResponseAdmission<'_>,
    ) -> Result<Self, ChallengeError> {
        let wire: ChallengeResponseWirePayload = serde_json::from_value(value)
            .map_err(|error| ChallengeError::InvalidResponse(format!("payload: {error}")))?;
        let responses = wire
            .responses
            .into_iter()
            .enumerate()
            .map(|(index, response)| {
                let replacement_recommendation = response
                    .replacement_recommendation
                    .map(|draft| {
                        ContextualRecommendation::from_draft(context.case.case_id.clone(), draft)
                            .map_err(|error| {
                                ChallengeError::InvalidResponse(format!(
                                    "response {index}: replacement recommendation: {error}"
                                ))
                            })
                    })
                    .transpose()?;
                Ok(ObjectionResponse {
                    objection_id: response.objection_id,
                    action: response.action,
                    rationale: response.rationale,
                    evidence_probe_ids: response.evidence_probe_ids,
                    replacement_recommendation,
                })
            })
            .collect::<Result<Vec<_>, ChallengeError>>()?;
        Self::admit(ChallengeResponsePayload { responses }, context)
    }

    pub fn admit(
        mut payload: ChallengeResponsePayload,
        context: ChallengeResponseAdmission<'_>,
    ) -> Result<Self, ChallengeError> {
        if context.challenge.case_id != context.case.case_id {
            return Err(ChallengeError::CaseMismatch);
        }
        let objections = context
            .challenge
            .objections
            .iter()
            .map(|objection| (&objection.objection_id, objection))
            .collect::<BTreeMap<_, _>>();
        let seen = validate_responses(&mut payload.responses, &context, &objections)?;
        validate_response_coverage(&seen, &objections)?;
        payload
            .responses
            .sort_by(|left, right| left.objection_id.cmp(&right.objection_id));
        let response_id = response_identity(&payload, &context)?;
        Ok(Self {
            schema_version: CHALLENGE_RESPONSE_SCHEMA_VERSION,
            response_id,
            reviewer_run_id: context.reviewer_run_id,
            case_id: context.case.case_id.clone(),
            challenge_id: context.challenge.challenge_id.clone(),
            responses: payload.responses,
        })
    }
}

fn validate_responses<'a>(
    responses: &mut [ObjectionResponse],
    context: &ChallengeResponseAdmission<'_>,
    objections: &BTreeMap<&'a StableId, &'a super::ChallengeObjection>,
) -> Result<BTreeSet<StableId>, ChallengeError> {
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for (index, response) in responses.iter_mut().enumerate() {
        response.evidence_probe_ids.sort();
        response.evidence_probe_ids.dedup();
        let Some(objection) = objections.get(&response.objection_id) else {
            errors.push(format!(
                "response {index}: objection_id {} is not in the bound challenge",
                response.objection_id
            ));
            continue;
        };
        let duplicate = !seen.insert(response.objection_id.clone());
        let reasons = response_errors(response, objection, duplicate, context);
        if !reasons.is_empty() {
            errors.push(format!("response {index}: {}", reasons.join(", ")));
        }
    }
    if errors.is_empty() {
        Ok(seen)
    } else {
        Err(ChallengeError::InvalidResponse(errors.join("; ")))
    }
}

fn response_errors(
    response: &ObjectionResponse,
    objection: &super::ChallengeObjection,
    duplicate: bool,
    context: &ChallengeResponseAdmission<'_>,
) -> Vec<String> {
    let mut reasons = basic_response_errors(response, duplicate, context.allowed_probe_ids);
    if response.action == ResponseAction::Narrow {
        if objection.target_type != super::ChallengeTargetType::Recommendation {
            reasons.push("narrow is valid only for a recommendation-target objection".to_string());
        }
        if response.replacement_recommendation.is_none() {
            reasons.push("narrow requires replacement_recommendation".to_string());
        }
    } else if response.replacement_recommendation.is_some() {
        reasons.push("replacement_recommendation is valid only with narrow".to_string());
    }
    if let Some(recommendation) = &response.replacement_recommendation {
        reasons.extend(replacement_errors(recommendation, objection, context));
    }
    reasons
}

fn basic_response_errors(
    response: &ObjectionResponse,
    duplicate: bool,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if duplicate {
        reasons.push("objection_id is duplicated".to_string());
    }
    if response.rationale.trim().is_empty() {
        reasons.push("rationale is empty".to_string());
    }
    reasons.extend(super::unknown_identifier_reason(
        &response.evidence_probe_ids,
        allowed_probe_ids,
        "evidence_probe_ids contains IDs outside the bound case: ",
        "",
    ));
    if response.action == ResponseAction::MaintainWithEvidence
        && response.evidence_probe_ids.is_empty()
    {
        reasons.push(
            "maintain_with_evidence requires at least one bound evidence_probe_id".to_string(),
        );
    }
    reasons
}

fn replacement_errors(
    recommendation: &ContextualRecommendation,
    objection: &super::ChallengeObjection,
    context: &ChallengeResponseAdmission<'_>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if recommendation.case_id != context.case.case_id {
        reasons.push("replacement recommendation belongs to another case".to_string());
    }
    if recommendation.recommendation_id == objection.target_id {
        reasons.push("replacement recommendation must have a new derived identity".to_string());
    }
    let unknown_claim_ids = recommendation
        .premise_claim_ids
        .iter()
        .filter(|id| !context.allowed_claim_ids.contains(*id))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !unknown_claim_ids.is_empty() {
        reasons.push(format!(
            "replacement premise_claim_ids contains IDs outside the admitted review: {unknown_claim_ids:?}"
        ));
    }
    reasons
}

fn validate_response_coverage(
    seen: &BTreeSet<StableId>,
    objections: &BTreeMap<&StableId, &super::ChallengeObjection>,
) -> Result<(), ChallengeError> {
    if seen.len() == objections.len() {
        return Ok(());
    }
    let missing = objections
        .keys()
        .filter(|id| !seen.contains(*id))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Err(ChallengeError::InvalidResponse(format!(
        "responses must address every bound objection exactly once; missing {missing:?}"
    )))
}

fn response_identity(
    payload: &ChallengeResponsePayload,
    context: &ChallengeResponseAdmission<'_>,
) -> Result<StableId, ChallengeError> {
    StableId::from_identity(
        "challenge-response",
        CHALLENGE_RESPONSE_SCHEMA_VERSION,
        &json!({
            "case_id": context.case.case_id,
            "challenge_id": context.challenge.challenge_id,
            "responses": payload.responses,
            "reviewer_run_id": context.reviewer_run_id,
        }),
    )
    .map_err(|error| ChallengeError::InvalidIdentity(error.to_string()))
}

pub fn challenge_response_payload_schema() -> Value {
    let replacement_schema = replacement_recommendation_wire_schema();
    json!({
        "additionalProperties": false,
        "properties": {
            "responses": {
                "items": {
                    "additionalProperties": false,
                    "properties": {
                        "action": {
                            "enum": [
                                "concede", "narrow", "maintain_with_evidence", "withdraw",
                                "mark_unresolved"
                            ]
                        },
                        "evidence_probe_ids": {
                            "items": {
                                "pattern": "^[a-z0-9-]+:[0-9a-f]{64}$",
                                "type": "string"
                            },
                            "type": "array",
                            "uniqueItems": true
                        },
                        "objection_id": {
                            "pattern": "^challenge-objection:[0-9a-f]{64}$",
                            "type": "string"
                        },
                        "rationale": {"minLength": 1, "type": "string"},
                        "replacement_recommendation": {
                            "anyOf": [
                                replacement_schema,
                                {"type": "null"}
                            ]
                        }
                    },
                    "required": [
                        "objection_id", "action", "rationale", "evidence_probe_ids",
                        "replacement_recommendation"
                    ],
                    "type": "object"
                },
                "type": "array",
                "uniqueItems": true
            }
        },
        "required": ["responses"],
        "type": "object"
    })
}

pub fn bound_challenge_response_payload_schema(
    challenge: &ChallengeRecord,
    review: &ReviewerCaseReview,
    allowed_probe_ids: &BTreeSet<StableId>,
) -> Value {
    let mut replacement_schema = replacement_recommendation_wire_schema();
    replacement_schema["properties"]["premise_claim_ids"]["items"] = json!({
        "enum": review
            .claims
            .iter()
            .map(|claim| claim.claim_id.to_string())
            .collect::<Vec<_>>()
    });
    let probe_ids = allowed_probe_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let response_branches = challenge
        .objections
        .iter()
        .flat_map(|objection| {
            let objection_id = objection.objection_id.to_string();
            let mut branches = vec![
                bound_response_item_schema(
                    &objection_id,
                    &["concede", "withdraw", "mark_unresolved"],
                    &probe_ids,
                    false,
                    json!({"type": "null"}),
                ),
                bound_response_item_schema(
                    &objection_id,
                    &["maintain_with_evidence"],
                    &probe_ids,
                    true,
                    json!({"type": "null"}),
                ),
            ];
            if objection.target_type == super::ChallengeTargetType::Recommendation {
                branches.push(bound_response_item_schema(
                    &objection_id,
                    &["narrow"],
                    &probe_ids,
                    false,
                    replacement_schema.clone(),
                ));
            }
            branches
        })
        .collect::<Vec<_>>();
    json!({
        "additionalProperties": false,
        "properties": {
            "responses": {
                "items": {"oneOf": response_branches},
                "minItems": challenge.objections.len(),
                "maxItems": challenge.objections.len(),
                "type": "array",
                "uniqueItems": true
            }
        },
        "required": ["responses"],
        "type": "object"
    })
}

fn bound_response_item_schema(
    objection_id: &str,
    actions: &[&str],
    probe_ids: &[String],
    evidence_required: bool,
    replacement_schema: Value,
) -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "action": {"enum": actions},
            "evidence_probe_ids": {
                "items": {"enum": probe_ids},
                "minItems": if evidence_required { 1 } else { 0 },
                "type": "array",
                "uniqueItems": true
            },
            "objection_id": {"enum": [objection_id]},
            "rationale": {"minLength": 1, "type": "string"},
            "replacement_recommendation": replacement_schema
        },
        "required": [
            "objection_id", "action", "rationale", "evidence_probe_ids",
            "replacement_recommendation"
        ],
        "type": "object"
    })
}

fn replacement_recommendation_wire_schema() -> Value {
    let mut schema = crate::case_review::deliberation::recommendation_wire_schema();
    let object = schema
        .as_object_mut()
        .expect("recommendation wire schema is an object");
    let properties = object["properties"]
        .as_object_mut()
        .expect("recommendation wire properties are an object");
    properties.remove("recommendation_ref");
    properties.remove("premise_claim_refs");
    properties.insert(
        "premise_claim_ids".to_string(),
        json!({
            "items": {
                "pattern": "^engineering-claim:[0-9a-f]{64}$",
                "type": "string"
            },
            "minItems": 1,
            "type": "array",
            "uniqueItems": true
        }),
    );
    let required = object["required"]
        .as_array_mut()
        .expect("recommendation wire required is an array");
    required.retain(|field| {
        !matches!(
            field.as_str(),
            Some("recommendation_ref" | "premise_claim_refs")
        )
    });
    required.push(Value::String("premise_claim_ids".to_string()));
    schema
}

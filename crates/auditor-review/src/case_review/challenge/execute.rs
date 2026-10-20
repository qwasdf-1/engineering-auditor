use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ChallengeObjection;
use crate::case_review::deliberation::evidence_round::{
    decode_final_payload, EvidenceRound, EvidenceRoundError,
};
use crate::{
    CaseEvidenceRequest, CaseProviderTurn, ContentDigest, EngineeringCase, EvidencePacket,
    ReviewerCaseReview, RouteTarget, StableId,
};

pub const CHALLENGE_RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStatus {
    Cleared,
    ObjectionsRaised,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRecordPayload {
    pub status: ChallengeStatus,
    pub objections: Vec<ChallengeObjection>,
    pub common_mode_concerns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ChallengeObjectionDraft {
    target_type: super::ChallengeTargetType,
    target_id: StableId,
    reason: super::ObjectionReason,
    evidence_ids: Vec<String>,
    materiality: crate::Materiality,
    required_resolution: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ChallengeRecordWirePayload {
    status: ChallengeStatus,
    objections: Vec<ChallengeObjectionDraft>,
    common_mode_concerns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRecord {
    pub schema_version: u32,
    pub challenge_id: StableId,
    pub challenger_run_id: StableId,
    pub case_id: StableId,
    pub reviewer_review_id: StableId,
    pub packet_digest: ContentDigest,
    pub route_target: RouteTarget,
    pub status: ChallengeStatus,
    pub objections: Vec<ChallengeObjection>,
    pub common_mode_concerns: Vec<String>,
}

pub struct ChallengeAdmissionContext<'a> {
    pub challenger_run_id: &'a StableId,
    pub route_target: &'a RouteTarget,
    pub case: &'a EngineeringCase,
    pub packet: &'a EvidencePacket,
    pub reviewer_review: &'a ReviewerCaseReview,
}

impl ChallengeRecord {
    fn admit_wire(
        payload: ChallengeRecordWirePayload,
        context: ChallengeAdmissionContext<'_>,
    ) -> Result<Self, ChallengeError> {
        let objections = payload
            .objections
            .into_iter()
            .map(|draft| {
                let objection_id = StableId::from_identity(
                    "challenge-objection",
                    CHALLENGE_RECORD_SCHEMA_VERSION,
                    &json!({
                        "case_id": context.case.case_id,
                        "reason": draft.reason,
                        "target_id": draft.target_id,
                        "target_type": draft.target_type,
                    }),
                )
                .map_err(|error| ChallengeError::InvalidIdentity(error.to_string()))?;
                Ok(ChallengeObjection {
                    objection_id,
                    target_type: draft.target_type,
                    target_id: draft.target_id,
                    reason: draft.reason,
                    evidence_ids: draft.evidence_ids,
                    materiality: draft.materiality,
                    required_resolution: draft.required_resolution,
                })
            })
            .collect::<Result<Vec<_>, ChallengeError>>()?;
        Self::admit(
            ChallengeRecordPayload {
                status: payload.status,
                objections,
                common_mode_concerns: payload.common_mode_concerns,
            },
            context,
        )
    }

    pub fn admit(
        mut payload: ChallengeRecordPayload,
        context: ChallengeAdmissionContext<'_>,
    ) -> Result<Self, ChallengeError> {
        validate_context(&context)?;
        let targets = super::challenge_targets(context.reviewer_review);
        let evidence = context
            .packet
            .items
            .iter()
            .map(|item| item.id.clone())
            .chain(super::reviewer_cited_probe_ids(context.reviewer_review))
            .collect::<BTreeSet<_>>();
        validate_objections(&mut payload.objections, &context, &targets, &evidence)?;
        normalize_payload(&mut payload);
        validate_status(&payload)?;
        let challenge_id = challenge_identity(&payload, &context)?;
        Ok(Self {
            schema_version: CHALLENGE_RECORD_SCHEMA_VERSION,
            challenge_id,
            challenger_run_id: context.challenger_run_id.clone(),
            case_id: context.case.case_id.clone(),
            reviewer_review_id: context.reviewer_review.review_id.clone(),
            packet_digest: context.packet.packet_digest.clone(),
            route_target: context.route_target.clone(),
            status: payload.status,
            objections: payload.objections,
            common_mode_concerns: payload.common_mode_concerns,
        })
    }
}

fn validate_context(context: &ChallengeAdmissionContext<'_>) -> Result<(), ChallengeError> {
    let mismatched = context.packet.case_id != context.case.case_id
        || context.reviewer_review.case_id != context.case.case_id
        || context.reviewer_review.packet_digest != context.packet.packet_digest;
    if mismatched {
        Err(ChallengeError::CaseMismatch)
    } else {
        Ok(())
    }
}

fn validate_objections(
    objections: &mut [ChallengeObjection],
    context: &ChallengeAdmissionContext<'_>,
    targets: &std::collections::BTreeMap<StableId, super::ChallengeTargetType>,
    evidence: &BTreeSet<String>,
) -> Result<(), ChallengeError> {
    let mut identities = BTreeSet::new();
    let mut errors = Vec::new();
    for (index, objection) in objections.iter_mut().enumerate() {
        objection.evidence_ids.sort();
        objection.evidence_ids.dedup();
        let expected = objection_identity(&context.case.case_id, objection)?;
        let duplicate = !identities.insert(expected.clone());
        let reasons = objection_errors(objection, &expected, duplicate, targets, evidence);
        if !reasons.is_empty() {
            errors.push(format!("objection {index}: {}", reasons.join(", ")));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ChallengeError::InvalidObjection(errors.join("; ")))
    }
}

fn objection_errors(
    objection: &ChallengeObjection,
    expected_id: &StableId,
    duplicate: bool,
    targets: &std::collections::BTreeMap<StableId, super::ChallengeTargetType>,
    evidence: &BTreeSet<String>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if targets.get(&objection.target_id) != Some(&objection.target_type) {
        reasons.push(
            "target_id and target_type do not name an admitted claim or recommendation".to_string(),
        );
    }
    reasons.extend(
        objection
            .evidence_ids
            .is_empty()
            .then(|| "evidence_ids is empty".to_string()),
    );
    reasons.extend(
        objection
            .required_resolution
            .trim()
            .is_empty()
            .then(|| "required_resolution is empty".to_string()),
    );
    reasons.extend(super::unknown_identifier_reason(
        &objection.evidence_ids,
        evidence,
        "unknown evidence IDs ",
        "; use closed-packet evidence item IDs or case-probe IDs cited by the admitted reviewer result",
    ));
    if objection.objection_id != *expected_id || duplicate {
        reasons.push("objection identity is invalid or duplicated".to_string());
    }
    reasons
}

fn objection_identity(
    case_id: &StableId,
    objection: &ChallengeObjection,
) -> Result<StableId, ChallengeError> {
    StableId::from_identity(
        "challenge-objection",
        CHALLENGE_RECORD_SCHEMA_VERSION,
        &json!({
            "case_id": case_id,
            "reason": objection.reason,
            "target_id": objection.target_id,
            "target_type": objection.target_type,
        }),
    )
    .map_err(|error| ChallengeError::InvalidIdentity(error.to_string()))
}

fn normalize_payload(payload: &mut ChallengeRecordPayload) {
    payload
        .objections
        .sort_by(|left, right| left.objection_id.cmp(&right.objection_id));
    payload
        .common_mode_concerns
        .retain(|concern| !concern.trim().is_empty());
    payload.common_mode_concerns.sort();
    payload.common_mode_concerns.dedup();
}

fn validate_status(payload: &ChallengeRecordPayload) -> Result<(), ChallengeError> {
    let invalid = (payload.status == ChallengeStatus::Cleared && !payload.objections.is_empty())
        || (payload.status == ChallengeStatus::ObjectionsRaised && payload.objections.is_empty())
        || (payload.status == ChallengeStatus::Blocked
            && payload.objections.is_empty()
            && payload.common_mode_concerns.is_empty());
    if invalid {
        Err(ChallengeError::InvalidStatus)
    } else {
        Ok(())
    }
}

fn challenge_identity(
    payload: &ChallengeRecordPayload,
    context: &ChallengeAdmissionContext<'_>,
) -> Result<StableId, ChallengeError> {
    StableId::from_identity(
        "challenge-record",
        CHALLENGE_RECORD_SCHEMA_VERSION,
        &json!({
            "case_id": context.case.case_id,
            "challenger_run_id": context.challenger_run_id,
            "common_mode_concerns": payload.common_mode_concerns,
            "objections": payload.objections,
            "packet_digest": context.packet.packet_digest,
            "reviewer_review_id": context.reviewer_review.review_id,
            "route_target": context.route_target,
            "status": payload.status,
        }),
    )
    .map_err(|error| ChallengeError::InvalidIdentity(error.to_string()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChallengeTurn {
    EvidenceRequested(Vec<CaseEvidenceRequest>),
    Complete(Box<ChallengeRecord>),
}

pub struct ChallengeSession {
    evidence_round: EvidenceRound,
}

impl ChallengeSession {
    pub fn new(case: &EngineeringCase, packet: &EvidencePacket) -> Result<Self, ChallengeError> {
        if case.case_id != packet.case_id {
            return Err(ChallengeError::CaseMismatch);
        }
        Ok(Self {
            evidence_round: EvidenceRound::new(case.case_id.clone(), packet.packet_digest.clone()),
        })
    }

    pub fn consume_turn(
        &mut self,
        turn: CaseProviderTurn,
        context: ChallengeAdmissionContext<'_>,
    ) -> Result<ChallengeTurn, ChallengeError> {
        if let Some(requests) = self
            .evidence_round
            .advance_for(context.case, context.packet, turn.evidence_requests)
            .map_err(challenge_round_error)?
        {
            return Ok(ChallengeTurn::EvidenceRequested(requests));
        }
        let payload: ChallengeRecordWirePayload =
            decode_final_payload(turn.challenge, ChallengeError::MissingChallenge, |error| {
                ChallengeError::InvalidPayload(error.to_string())
            })?;
        let challenge = ChallengeRecord::admit_wire(payload, context)?;
        self.evidence_round.complete();
        Ok(ChallengeTurn::Complete(Box::new(challenge)))
    }
}

fn challenge_round_error(error: EvidenceRoundError) -> ChallengeError {
    match error {
        EvidenceRoundError::CaseMismatch => ChallengeError::CaseMismatch,
        EvidenceRoundError::Exhausted => ChallengeError::EvidenceRoundExhausted,
        EvidenceRoundError::OutOfCase => ChallengeError::OutOfCaseEvidenceRequest,
        EvidenceRoundError::NotRecompiled => ChallengeError::EvidenceNotRecompiled,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChallengeError {
    CaseMismatch,
    EvidenceRoundExhausted,
    OutOfCaseEvidenceRequest,
    EvidenceNotRecompiled,
    MissingChallenge,
    InvalidPayload(String),
    InvalidStatus,
    InvalidObjection(String),
    InvalidResponse(String),
    InvalidIdentity(String),
}

impl fmt::Display for ChallengeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ChallengeError {}

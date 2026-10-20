use std::collections::BTreeMap;

use crate::{
    ChallengeRecord, ChallengeRecordPayload, ChallengeResponse, ChallengeResponseAdmission,
    ChallengeResponsePayload, EngineeringCase, EvidencePacket, ProbeDefinition, ReviewerCaseReview,
    StableId,
};

use super::offline::{ChallengeOutcome, ReviewVerificationError};

pub(super) struct NormalizedChallengeVerification<'a> {
    pub(super) outcome: &'a ChallengeOutcome,
    pub(super) case: &'a EngineeringCase,
    pub(super) packet: &'a EvidencePacket,
    pub(super) review: &'a ReviewerCaseReview,
    pub(super) definitions: &'a BTreeMap<&'a StableId, &'a ProbeDefinition>,
    pub(super) stale_challenge_message: &'static str,
    pub(super) stale_response_message: &'static str,
}

pub(super) fn verify_normalized_challenge(
    context: NormalizedChallengeVerification<'_>,
) -> Result<(), ReviewVerificationError> {
    let admitted = ChallengeRecord::admit(
        ChallengeRecordPayload {
            status: context.outcome.challenge.status,
            objections: context.outcome.challenge.objections.clone(),
            common_mode_concerns: context.outcome.challenge.common_mode_concerns.clone(),
        },
        crate::ChallengeAdmissionContext {
            challenger_run_id: &context.outcome.challenge.challenger_run_id,
            route_target: &context.outcome.challenge.route_target,
            case: context.case,
            packet: context.packet,
            reviewer_review: context.review,
        },
    )
    .map_err(|error| ReviewVerificationError::Challenge(error.to_string()))?;
    if admitted != context.outcome.challenge {
        return Err(ReviewVerificationError::Challenge(
            context.stale_challenge_message.to_string(),
        ));
    }
    if let Some(response) = &context.outcome.response {
        let allowed = context
            .definitions
            .values()
            .filter(|definition| definition.case_id == context.case.case_id)
            .map(|definition| definition.probe_id.clone())
            .collect();
        let admitted = ChallengeResponse::admit(
            ChallengeResponsePayload {
                responses: response.responses.clone(),
            },
            ChallengeResponseAdmission::for_review(
                response.reviewer_run_id.clone(),
                context.case,
                &context.outcome.challenge,
                context.review,
                &allowed,
            ),
        )
        .map_err(|error| ReviewVerificationError::Challenge(error.to_string()))?;
        if admitted != *response {
            return Err(ReviewVerificationError::Challenge(
                context.stale_response_message.to_string(),
            ));
        }
    }
    Ok(())
}

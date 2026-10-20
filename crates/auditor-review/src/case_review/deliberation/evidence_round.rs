use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{CaseEvidenceRequest, ContentDigest, StableId};

#[derive(Clone, Debug)]
pub(crate) struct EvidenceRound {
    case_id: StableId,
    initial_packet_digest: ContentDigest,
    requested: bool,
    recompile_required: bool,
    complete: bool,
}

impl EvidenceRound {
    pub(crate) fn new(case_id: StableId, packet_digest: ContentDigest) -> Self {
        Self {
            case_id,
            initial_packet_digest: packet_digest,
            requested: false,
            recompile_required: false,
            complete: false,
        }
    }

    /// Advances the round with a turn's evidence requests for `case`; a
    /// request the current packet cannot satisfy requires a recompile.
    pub(crate) fn advance_for(
        &mut self,
        case: &crate::EngineeringCase,
        packet: &crate::EvidencePacket,
        requests: Vec<CaseEvidenceRequest>,
    ) -> Result<Option<Vec<CaseEvidenceRequest>>, EvidenceRoundError> {
        // A completed case takes no further turns.
        if self.complete {
            return Err(EvidenceRoundError::CaseMismatch);
        }
        let recompile_required = requests
            .iter()
            .any(|request| !super::reviewer::evidence_request_is_satisfied(packet, request));
        self.advance(
            &case.case_id,
            &packet.packet_digest,
            recompile_required,
            requests,
        )
    }

    /// Marks the case complete once its final payload is admitted.
    pub(crate) fn complete(&mut self) {
        self.complete = true;
    }

    pub(crate) fn advance(
        &mut self,
        current_case_id: &StableId,
        current_packet_digest: &ContentDigest,
        recompile_required: bool,
        requests: Vec<CaseEvidenceRequest>,
    ) -> Result<Option<Vec<CaseEvidenceRequest>>, EvidenceRoundError> {
        if current_case_id != &self.case_id {
            return Err(EvidenceRoundError::CaseMismatch);
        }
        if !requests.is_empty() {
            if self.requested {
                return Err(EvidenceRoundError::Exhausted);
            }
            if requests
                .iter()
                .any(|request| request.case_id != self.case_id)
            {
                return Err(EvidenceRoundError::OutOfCase);
            }
            self.requested = true;
            self.recompile_required = recompile_required;
            return Ok(Some(requests));
        }
        if self.requested
            && self.recompile_required
            && current_packet_digest == &self.initial_packet_digest
        {
            return Err(EvidenceRoundError::NotRecompiled);
        }
        Ok(None)
    }
}

pub(crate) fn decode_final_payload<T: DeserializeOwned, E, F>(
    value: Option<Value>,
    missing: E,
    invalid: F,
) -> Result<T, E>
where
    F: FnOnce(serde_json::Error) -> E,
{
    serde_json::from_value(value.ok_or(missing)?).map_err(invalid)
}

pub(crate) enum EvidenceRoundError {
    CaseMismatch,
    Exhausted,
    OutOfCase,
    NotRecompiled,
}

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::evidence_round::{decode_final_payload, EvidenceRound, EvidenceRoundError};
#[cfg(test)]
use super::single_schema::{
    expected_benefit_wire_schema, validate_recommendation_benefit_arguments,
};
use super::single_wire::ReviewerCaseReviewWirePayload;
#[cfg(test)]
use super::single_wire::WireExpectedBenefit;
#[cfg(test)]
use crate::BenefitEvidenceMode;
use crate::{
    CaseClosure, CaseClosureReason, CaseConclusion, CaseEvidenceRequest, CaseProviderTurn,
    ContextualRecommendation, EngineeringCase, EngineeringClaim, EvidencePacket, ProbeDefinition,
    ProbeResult, RouteTarget, StableId,
};
#[cfg(test)]
use serde_json::json;

pub const REVIEWER_CASE_REVIEW_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CasePosition {
    Substantiated,
    NotSubstantiated,
    EvidenceBlocked,
    Contested,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerCaseReviewPayload {
    pub case_conclusion: CaseConclusion,
    pub case_position: CasePosition,
    pub model_resolved_probes: Vec<ProbeResult>,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    pub preferred_recommendation_id: Option<StableId>,
    pub unresolved_gaps: Vec<String>,
    pub proposed_closure: CaseClosureReason,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerCaseReview {
    pub schema_version: u32,
    pub review_id: StableId,
    pub reviewer_run_id: StableId,
    pub case_id: StableId,
    pub template_version: u32,
    pub finding_ids: Vec<String>,
    pub packet_digest: crate::ContentDigest,
    pub route_target: RouteTarget,
    pub case_conclusion: CaseConclusion,
    pub case_position: CasePosition,
    pub model_resolved_probes: Vec<ProbeResult>,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    pub preferred_recommendation_id: Option<StableId>,
    pub unresolved_gaps: Vec<String>,
    pub proposed_closure: CaseClosure,
}

impl ReviewerCaseReview {
    pub(crate) fn target_ids(&self) -> BTreeSet<&StableId> {
        self.claims
            .iter()
            .map(|claim| &claim.claim_id)
            .chain(
                self.recommendations
                    .iter()
                    .map(|recommendation| &recommendation.recommendation_id),
            )
            .collect()
    }
}

pub struct ReviewerAdmissionContext<'a> {
    pub reviewer_run_id: &'a StableId,
    pub route_target: &'a RouteTarget,
    pub case: &'a EngineeringCase,
    pub packet: &'a EvidencePacket,
    pub probe_definitions: &'a [ProbeDefinition],
    pub deterministic_probe_results: &'a [ProbeResult],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SingleReviewerTurn {
    EvidenceRequested(Vec<CaseEvidenceRequest>),
    Complete(Box<ReviewerCaseReview>),
}

#[derive(Clone, Debug)]
pub struct SingleReviewerSession {
    evidence_round: EvidenceRound,
}

impl SingleReviewerSession {
    pub fn new(
        case: &EngineeringCase,
        packet: &EvidencePacket,
    ) -> Result<Self, SingleReviewerError> {
        if packet.case_id != case.case_id {
            return Err(SingleReviewerError::CaseMismatch);
        }
        Ok(Self {
            evidence_round: EvidenceRound::new(case.case_id.clone(), packet.packet_digest.clone()),
        })
    }

    pub fn consume_turn(
        &mut self,
        turn: CaseProviderTurn,
        context: ReviewerAdmissionContext<'_>,
    ) -> Result<SingleReviewerTurn, SingleReviewerError> {
        if let Some(requests) = self
            .evidence_round
            .advance_for(context.case, context.packet, turn.evidence_requests)
            .map_err(single_round_error)?
        {
            return Ok(SingleReviewerTurn::EvidenceRequested(requests));
        }
        let payload: ReviewerCaseReviewWirePayload = decode_final_payload(
            turn.case_review,
            SingleReviewerError::MissingReview,
            |error| SingleReviewerError::InvalidPayload(error.to_string()),
        )?;
        let review = ReviewerCaseReview::admit_wire(payload, context)?;
        self.evidence_round.complete();
        Ok(SingleReviewerTurn::Complete(Box::new(review)))
    }
}

fn single_round_error(error: EvidenceRoundError) -> SingleReviewerError {
    match error {
        EvidenceRoundError::CaseMismatch => SingleReviewerError::CaseMismatch,
        EvidenceRoundError::Exhausted => SingleReviewerError::EvidenceRoundExhausted,
        EvidenceRoundError::OutOfCase => SingleReviewerError::OutOfCaseEvidenceRequest,
        EvidenceRoundError::NotRecompiled => SingleReviewerError::EvidenceNotRecompiled,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SingleReviewerError {
    CaseMismatch,
    EvidenceRoundExhausted,
    OutOfCaseEvidenceRequest,
    EvidenceNotRecompiled,
    MissingReview,
    InvalidPayload(String),
    InvalidProbeSet,
    InvalidModelProbe(String),
    UnresolvedMandatoryProbe,
    InvalidClaims(String),
    InvalidRecommendations(String),
    InvalidPreferredRecommendation,
    InvalidClosure,
    InvalidCaseConclusion(String),
    InvalidIdentity(String),
}

impl fmt::Display for SingleReviewerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SingleReviewerError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn benefit_probe_id() -> StableId {
        StableId::from_identity("case-probe", 1, &json!({"probe": "expected-benefit"}))
            .expect("valid benefit probe identity")
    }

    #[test]
    fn model_benefit_variants_omit_inapplicable_fields_and_expand_for_storage() {
        let probe_id = benefit_probe_id();
        let structural: WireExpectedBenefit = serde_json::from_value(json!({
            "description": "removes the duplicate ownership path",
            "evidence_mode": "structural_inference",
            "evidence_probe_ids": [probe_id],
        }))
        .expect("structural inference should not require numeric placeholders");
        let stored = structural.into_expected_benefit();
        assert_eq!(
            stored.evidence_mode,
            BenefitEvidenceMode::StructuralInference
        );
        assert_eq!(stored.numeric_value, None);
        assert_eq!(stored.unit, None);
        assert_eq!(stored.extrapolation_model, None);

        let invalid = serde_json::from_value::<WireExpectedBenefit>(json!({
            "description": "removes the duplicate ownership path",
            "evidence_mode": "structural_inference",
            "numeric_value": null,
            "unit": null,
            "extrapolation_model": null,
            "evidence_probe_ids": [benefit_probe_id()],
        }))
        .expect_err("structural inference must reject inapplicable placeholders");
        assert!(invalid.to_string().contains("unknown field"));

        let measured: WireExpectedBenefit = serde_json::from_value(json!({
            "description": "reduces review time",
            "evidence_mode": "direct_measurement",
            "numeric_value": 12,
            "unit": "minutes",
            "evidence_probe_ids": [benefit_probe_id()],
        }))
        .expect("direct measurement should require and accept value plus unit");
        let stored = measured.into_expected_benefit();
        assert_eq!(stored.evidence_mode, BenefitEvidenceMode::DirectMeasurement);
        assert_eq!(stored.numeric_value.as_deref(), Some("12"));
        assert_eq!(stored.unit.as_deref(), Some("minutes"));
        assert_eq!(stored.extrapolation_model, None);

        let symbolic = serde_json::from_value::<WireExpectedBenefit>(json!({
            "description": "reduces asymptotic work by a factor of n",
            "evidence_mode": "bounded_extrapolation",
            "numeric_value": "n",
            "unit": "factor",
            "extrapolation_model": "static complexity model",
            "evidence_probe_ids": [benefit_probe_id()],
        }))
        .expect_err("symbolic values belong to structural inference");
        assert!(symbolic.to_string().contains("number"));
    }

    #[test]
    fn benefit_schema_and_host_admission_share_exact_mode_variants() {
        let schema = expected_benefit_wire_schema();
        let variants = schema["oneOf"].as_array().expect("benefit variants");
        assert_eq!(variants.len(), 5);
        assert!(variants.iter().all(|variant| {
            variant["additionalProperties"] == false
                && variant["properties"]["evidence_mode"]["enum"]
                    .as_array()
                    .is_some_and(|modes| modes.len() == 1)
        }));

        let errors = validate_recommendation_benefit_arguments(&json!({
            "expected_benefits": [
                {
                    "description": "bounded estimate",
                    "evidence_mode": "bounded_extrapolation",
                    "evidence_probe_ids": []
                },
                {
                    "description": "qualitative observation",
                    "evidence_mode": "qualitative_only",
                    "evidence_probe_ids": [],
                    "numeric_value": 10,
                    "unit": "percent",
                    "extrapolation_model": "not applicable"
                }
            ]
        }))
        .expect_err("all mode violations should be reported together");
        assert_eq!(errors.len(), 6);
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[0].numeric_value:")));
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[0].unit:")));
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[0].extrapolation_model:")));
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[1].numeric_value:")));
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[1].unit:")));
        assert!(errors
            .iter()
            .any(|error| error.starts_with("expected_benefits[1].extrapolation_model:")));
    }
}

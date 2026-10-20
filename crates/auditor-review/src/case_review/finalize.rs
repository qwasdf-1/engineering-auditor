use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::case_review::recommendation::{
    conclusion_allows_recommendation, validate_recommendations,
};
use crate::{
    CaseClosure, CaseClosureReason, CaseFailure, ChallengeRecord, ChallengeResponse,
    ChallengeStatus, ClaimStatus, ContextualRecommendation, DetectorAdviceRelationship,
    DeterministicDisposition, EngineeringCase, EngineeringClaim, FinalCaseState, ResponseAction,
    ReviewerCaseReview, StableId,
};

pub const FINALIZED_CASE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalClaimDisposition {
    Accepted,
    Rejected,
    Blocked,
    Contested,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalRecommendationDisposition {
    Accepted,
    Rejected,
    Blocked,
    Contested,
    Narrowed,
    Withdrawn,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingDispositionKind {
    NotSelectedForContextualReview,
    CaseReferenced,
    IndependentlyActionable,
    SharedRootCauseMember,
    OverlappingSymptom,
    TemplateRecommendationPreserved,
    TemplateRecommendationConsolidated,
    TemplateRecommendationNarrowed,
    ActionDeferred,
    OperatorDecisionRequired,
    AdditionalEvidenceRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimDispositionRecord {
    pub claim_id: StableId,
    pub disposition: FinalClaimDisposition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationDispositionRecord {
    pub recommendation_id: StableId,
    pub disposition: FinalRecommendationDisposition,
    pub accepted_variant_id: Option<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FindingDispositionRecord {
    pub finding_id: String,
    pub case_id: Option<StableId>,
    pub disposition: FindingDispositionKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FinalizationBasis {
    Reviewer,
    Deterministic(DeterministicDisposition),
    Failure(CaseFailure),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedCase {
    pub schema_version: u32,
    pub finalization_id: StableId,
    pub case_id: StableId,
    pub basis: FinalizationBasis,
    pub reviewer_review_id: Option<StableId>,
    pub challenge_id: Option<StableId>,
    pub response_id: Option<StableId>,
    pub claim_dispositions: Vec<ClaimDispositionRecord>,
    pub recommendation_dispositions: Vec<RecommendationDispositionRecord>,
    pub accepted_recommendations: Vec<ContextualRecommendation>,
    pub finding_dispositions: Vec<FindingDispositionRecord>,
    pub closure: CaseClosure,
}

pub struct CaseFinalizationContext<'a> {
    pub case: &'a EngineeringCase,
    pub review: &'a ReviewerCaseReview,
    pub challenge: Option<&'a ChallengeRecord>,
    pub response: Option<&'a ChallengeResponse>,
    pub allowed_probe_ids: &'a BTreeSet<StableId>,
}

struct FinalizationParts {
    reviewer_review_id: Option<StableId>,
    challenge_id: Option<StableId>,
    response_id: Option<StableId>,
    claim_dispositions: Vec<ClaimDispositionRecord>,
    recommendation_dispositions: Vec<RecommendationDispositionRecord>,
    accepted_recommendations: Vec<ContextualRecommendation>,
    finding_dispositions: Vec<FindingDispositionRecord>,
    closure: CaseClosure,
}

impl FinalizedCase {
    pub fn finalize(context: CaseFinalizationContext<'_>) -> Result<Self, FinalizationError> {
        validate_links(&context)?;
        let responses = context
            .response
            .into_iter()
            .flat_map(|response| &response.responses)
            .map(|response| (&response.objection_id, response))
            .collect::<BTreeMap<_, _>>();
        let claim_dispositions = finalize_claims(&context, &responses);
        let claim_disposition_by_id = claim_dispositions
            .iter()
            .map(|record| (&record.claim_id, record.disposition))
            .collect::<BTreeMap<_, _>>();
        let mut accepted = Vec::new();
        let mut dispositions = Vec::new();
        for recommendation in &context.review.recommendations {
            let (disposition, variant) = finalize_recommendation(
                recommendation,
                &context,
                &responses,
                &claim_disposition_by_id,
            )?;
            if disposition == FinalRecommendationDisposition::Accepted {
                accepted.push(recommendation.clone());
            }
            if let Some(variant) = &variant {
                validate_recommendations(
                    context.case,
                    context.allowed_probe_ids,
                    &context.review.claims,
                    std::slice::from_ref(variant),
                )
                .map_err(FinalizationError::InvalidVariant)?;
                accepted.push(variant.clone());
            }
            dispositions.push(RecommendationDispositionRecord {
                recommendation_id: recommendation.recommendation_id.clone(),
                disposition,
                accepted_variant_id: variant.map(|value| value.recommendation_id),
            });
        }
        accepted.sort_by(|left, right| left.recommendation_id.cmp(&right.recommendation_id));
        let closure_reason = if dispositions
            .iter()
            .any(|record| record.disposition == FinalRecommendationDisposition::Contested)
            || claim_dispositions
                .iter()
                .any(|record| record.disposition == FinalClaimDisposition::Contested)
        {
            CaseClosureReason::Contested
        } else if dispositions
            .iter()
            .any(|record| record.disposition == FinalRecommendationDisposition::Blocked)
        {
            CaseClosureReason::BlockedByMissingEvidence
        } else {
            context.review.proposed_closure.reason
        };
        let closure = CaseClosure::terminal(
            context.case.case_id.clone(),
            closure_reason,
            closure_details(&dispositions),
        )
        .map_err(|_| FinalizationError::InvalidClosure)?;
        let finding_dispositions =
            finding_dispositions(context.case, &accepted, closure.final_state);
        Self::build(
            context.case,
            FinalizationBasis::Reviewer,
            FinalizationParts {
                reviewer_review_id: Some(context.review.review_id.clone()),
                challenge_id: context
                    .challenge
                    .map(|challenge| challenge.challenge_id.clone()),
                response_id: context
                    .response
                    .map(|response| response.response_id.clone()),
                claim_dispositions,
                recommendation_dispositions: dispositions,
                accepted_recommendations: accepted,
                finding_dispositions,
                closure,
            },
        )
    }

    pub fn deterministic(
        case: &EngineeringCase,
        disposition: DeterministicDisposition,
    ) -> Result<Self, FinalizationError> {
        let reason = match disposition {
            DeterministicDisposition::NotSubstantiated => CaseClosureReason::NotSubstantiated,
            DeterministicDisposition::EvidenceUnavailable => {
                CaseClosureReason::BlockedByMissingEvidence
            }
            DeterministicDisposition::ActionFullyDetermined => CaseClosureReason::Resolved,
            DeterministicDisposition::Unresolved => return Err(FinalizationError::InvalidBasis),
        };
        let closure = CaseClosure::terminal(
            case.case_id.clone(),
            reason,
            vec!["deterministic_disposition".to_string()],
        )
        .map_err(|_| FinalizationError::InvalidClosure)?;
        Self::build(
            case,
            FinalizationBasis::Deterministic(disposition),
            FinalizationParts {
                reviewer_review_id: None,
                challenge_id: None,
                response_id: None,
                claim_dispositions: Vec::new(),
                recommendation_dispositions: Vec::new(),
                accepted_recommendations: Vec::new(),
                finding_dispositions: finding_dispositions(case, &[], closure.final_state),
                closure,
            },
        )
    }

    pub fn failed(case: &EngineeringCase, closure: CaseClosure) -> Result<Self, FinalizationError> {
        closure
            .validate()
            .map_err(|_| FinalizationError::InvalidClosure)?;
        if closure.case_id != case.case_id {
            return Err(FinalizationError::CaseMismatch);
        }
        let failure = closure.failure.ok_or(FinalizationError::InvalidBasis)?;
        Self::build(
            case,
            FinalizationBasis::Failure(failure),
            FinalizationParts {
                reviewer_review_id: None,
                challenge_id: None,
                response_id: None,
                claim_dispositions: Vec::new(),
                recommendation_dispositions: Vec::new(),
                accepted_recommendations: Vec::new(),
                finding_dispositions: finding_dispositions(case, &[], closure.final_state),
                closure,
            },
        )
    }

    fn build(
        case: &EngineeringCase,
        basis: FinalizationBasis,
        parts: FinalizationParts,
    ) -> Result<Self, FinalizationError> {
        let FinalizationParts {
            reviewer_review_id,
            challenge_id,
            response_id,
            claim_dispositions,
            recommendation_dispositions,
            accepted_recommendations,
            finding_dispositions,
            closure,
        } = parts;
        let finalization_id = StableId::from_identity(
            "case-finalization",
            FINALIZED_CASE_SCHEMA_VERSION,
            &json!({
                "accepted_recommendations": accepted_recommendations,
                "basis": basis,
                "case_id": case.case_id,
                "challenge_id": challenge_id,
                "claim_dispositions": claim_dispositions,
                "closure": closure,
                "finding_dispositions": finding_dispositions,
                "recommendation_dispositions": recommendation_dispositions,
                "response_id": response_id,
                "reviewer_review_id": reviewer_review_id,
            }),
        )
        .map_err(|error| FinalizationError::InvalidIdentity(error.to_string()))?;
        Ok(Self {
            schema_version: FINALIZED_CASE_SCHEMA_VERSION,
            finalization_id,
            case_id: case.case_id.clone(),
            basis,
            reviewer_review_id,
            challenge_id,
            response_id,
            claim_dispositions,
            recommendation_dispositions,
            accepted_recommendations,
            finding_dispositions,
            closure,
        })
    }
}

fn validate_links(context: &CaseFinalizationContext<'_>) -> Result<(), FinalizationError> {
    if context.review.case_id != context.case.case_id
        || context.challenge.is_some_and(|challenge| {
            challenge.case_id != context.case.case_id
                || challenge.reviewer_review_id != context.review.review_id
        })
        || context.response.is_some_and(|response| {
            response.case_id != context.case.case_id
                || context
                    .challenge
                    .is_none_or(|challenge| response.challenge_id != challenge.challenge_id)
        })
    {
        return Err(FinalizationError::CaseMismatch);
    }
    Ok(())
}

fn claim_disposition(claim: &EngineeringClaim) -> FinalClaimDisposition {
    match claim.status {
        ClaimStatus::Substantiated | ClaimStatus::PartiallySubstantiated => {
            FinalClaimDisposition::Accepted
        }
        ClaimStatus::NotSubstantiated => FinalClaimDisposition::Rejected,
        ClaimStatus::Blocked => FinalClaimDisposition::Blocked,
        ClaimStatus::Contested => FinalClaimDisposition::Contested,
    }
}

fn finalize_claims(
    context: &CaseFinalizationContext<'_>,
    responses: &BTreeMap<&StableId, &crate::ObjectionResponse>,
) -> Vec<ClaimDispositionRecord> {
    context
        .review
        .claims
        .iter()
        .map(|claim| {
            let mut disposition = claim_disposition(claim);
            if let Some(challenge) = context.challenge {
                for objection in challenge
                    .objections
                    .iter()
                    .filter(|objection| objection.target_id == claim.claim_id)
                {
                    let objection_disposition = match responses.get(&objection.objection_id) {
                        None => FinalClaimDisposition::Contested,
                        Some(response) => match response.action {
                            ResponseAction::Concede | ResponseAction::Withdraw => {
                                FinalClaimDisposition::Rejected
                            }
                            ResponseAction::MarkUnresolved => FinalClaimDisposition::Contested,
                            ResponseAction::MaintainWithEvidence => disposition,
                            ResponseAction::Narrow => FinalClaimDisposition::Contested,
                        },
                    };
                    disposition = stronger_claim_disposition(disposition, objection_disposition);
                }
            }
            ClaimDispositionRecord {
                claim_id: claim.claim_id.clone(),
                disposition,
            }
        })
        .collect()
}

fn stronger_claim_disposition(
    left: FinalClaimDisposition,
    right: FinalClaimDisposition,
) -> FinalClaimDisposition {
    let rank = |value| match value {
        FinalClaimDisposition::Accepted => 0,
        FinalClaimDisposition::Contested => 1,
        FinalClaimDisposition::Blocked => 2,
        FinalClaimDisposition::Rejected => 3,
    };
    if rank(left) >= rank(right) {
        left
    } else {
        right
    }
}

fn finalize_recommendation(
    recommendation: &ContextualRecommendation,
    context: &CaseFinalizationContext<'_>,
    responses: &BTreeMap<&StableId, &crate::ObjectionResponse>,
    claim_dispositions: &BTreeMap<&StableId, FinalClaimDisposition>,
) -> Result<
    (
        FinalRecommendationDisposition,
        Option<ContextualRecommendation>,
    ),
    FinalizationError,
> {
    if !conclusion_allows_recommendation(context.review.case_conclusion, recommendation.family) {
        return Ok((FinalRecommendationDisposition::Rejected, None));
    }
    if let Some(disposition) = invalid_premise_disposition(recommendation, claim_dispositions) {
        return Ok((disposition, None));
    }
    let requires_challenge =
        recommendation.family == crate::RecommendationFamily::LifecycleReownership;
    let Some(challenge) = context.challenge else {
        return Ok((
            if requires_challenge {
                FinalRecommendationDisposition::Blocked
            } else {
                FinalRecommendationDisposition::Accepted
            },
            None,
        ));
    };
    if challenge.status == ChallengeStatus::Blocked {
        return Ok((FinalRecommendationDisposition::Blocked, None));
    }
    Ok(resolve_recommendation_objections(
        recommendation,
        challenge,
        responses,
    ))
}

fn invalid_premise_disposition(
    recommendation: &ContextualRecommendation,
    claim_dispositions: &BTreeMap<&StableId, FinalClaimDisposition>,
) -> Option<FinalRecommendationDisposition> {
    let premises = recommendation
        .premise_claim_ids
        .iter()
        .filter_map(|id| claim_dispositions.get(id))
        .copied()
        .collect::<BTreeSet<_>>();
    if premises.contains(&FinalClaimDisposition::Contested) {
        Some(FinalRecommendationDisposition::Contested)
    } else if premises.contains(&FinalClaimDisposition::Blocked)
        && recommendation.family != crate::RecommendationFamily::MeasurementBeforeChange
    {
        Some(FinalRecommendationDisposition::Blocked)
    } else if premises.contains(&FinalClaimDisposition::Rejected) {
        Some(FinalRecommendationDisposition::Rejected)
    } else {
        None
    }
}

fn resolve_recommendation_objections(
    recommendation: &ContextualRecommendation,
    challenge: &ChallengeRecord,
    responses: &BTreeMap<&StableId, &crate::ObjectionResponse>,
) -> (
    FinalRecommendationDisposition,
    Option<ContextualRecommendation>,
) {
    let objections = challenge
        .objections
        .iter()
        .filter(|objection| objection.target_id == recommendation.recommendation_id)
        .collect::<Vec<_>>();
    if objections.is_empty() {
        return (FinalRecommendationDisposition::Accepted, None);
    }
    let mut disposition = FinalRecommendationDisposition::Accepted;
    let mut variant = None;
    for objection in objections {
        let Some(response) = responses.get(&objection.objection_id) else {
            return (FinalRecommendationDisposition::Contested, None);
        };
        match response.action {
            ResponseAction::Concede => {
                disposition = stronger_recommendation_disposition(
                    disposition,
                    FinalRecommendationDisposition::Rejected,
                )
            }
            ResponseAction::Withdraw => disposition = FinalRecommendationDisposition::Withdrawn,
            ResponseAction::MarkUnresolved => {
                disposition = stronger_recommendation_disposition(
                    disposition,
                    FinalRecommendationDisposition::Contested,
                )
            }
            ResponseAction::MaintainWithEvidence => {}
            ResponseAction::Narrow => {
                disposition = stronger_recommendation_disposition(
                    disposition,
                    FinalRecommendationDisposition::Narrowed,
                );
                variant = response.replacement_recommendation.clone();
            }
        }
    }
    if disposition != FinalRecommendationDisposition::Narrowed {
        variant = None;
    }
    (disposition, variant)
}

fn stronger_recommendation_disposition(
    left: FinalRecommendationDisposition,
    right: FinalRecommendationDisposition,
) -> FinalRecommendationDisposition {
    let rank = |value| match value {
        FinalRecommendationDisposition::Accepted => 0,
        FinalRecommendationDisposition::Narrowed => 1,
        FinalRecommendationDisposition::Contested => 2,
        FinalRecommendationDisposition::Blocked => 3,
        FinalRecommendationDisposition::Rejected => 4,
        FinalRecommendationDisposition::Withdrawn => 5,
    };
    if rank(left) >= rank(right) {
        left
    } else {
        right
    }
}

fn closure_details(dispositions: &[RecommendationDispositionRecord]) -> Vec<String> {
    dispositions
        .iter()
        .filter(|record| {
            matches!(
                record.disposition,
                FinalRecommendationDisposition::Blocked
                    | FinalRecommendationDisposition::Contested
                    | FinalRecommendationDisposition::Withdrawn
            )
        })
        .map(|record| format!("{:?}", record.disposition).to_lowercase())
        .collect()
}

fn finding_dispositions(
    case: &EngineeringCase,
    accepted: &[ContextualRecommendation],
    final_state: FinalCaseState,
) -> Vec<FindingDispositionRecord> {
    case.finding_ids
        .iter()
        .map(|finding_id| {
            let disposition = if final_state == FinalCaseState::BlockedByMissingEvidence {
                FindingDispositionKind::AdditionalEvidenceRequired
            } else if final_state == FinalCaseState::Contested {
                FindingDispositionKind::OperatorDecisionRequired
            } else if final_state == FinalCaseState::ExecutionFailed {
                FindingDispositionKind::ActionDeferred
            } else {
                accepted
                    .iter()
                    .find(|recommendation| recommendation.affected_finding_ids.contains(finding_id))
                    .map(
                        |recommendation| match recommendation.detector_advice_relationship {
                            DetectorAdviceRelationship::Preserves => {
                                FindingDispositionKind::TemplateRecommendationPreserved
                            }
                            DetectorAdviceRelationship::Consolidates => {
                                FindingDispositionKind::TemplateRecommendationConsolidated
                            }
                            DetectorAdviceRelationship::Narrows => {
                                FindingDispositionKind::TemplateRecommendationNarrowed
                            }
                            DetectorAdviceRelationship::Sequences
                            | DetectorAdviceRelationship::Defers => {
                                FindingDispositionKind::ActionDeferred
                            }
                            DetectorAdviceRelationship::NotActionableWithCurrentEvidence => {
                                FindingDispositionKind::AdditionalEvidenceRequired
                            }
                        },
                    )
                    .unwrap_or(FindingDispositionKind::CaseReferenced)
            };
            FindingDispositionRecord {
                finding_id: finding_id.clone(),
                case_id: Some(case.case_id.clone()),
                disposition,
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FinalizationError {
    CaseMismatch,
    InvalidVariant(String),
    InvalidClosure,
    InvalidBasis,
    InvalidIdentity(String),
}

impl fmt::Display for FinalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for FinalizationError {}

use std::collections::BTreeMap;

use crate::{
    CaseFinalizationContext, EngineeringCase, FinalizationBasis, FinalizationError, FinalizedCase,
    ProbeDefinition, ReviewRole, ReviewerCaseReview, StableId,
};

use super::offline::{
    unique_by, verify_case_challenge, verify_deterministic_basis, ChallengeOutcome,
    ChallengeVerification, EngineeringReviewV2, ReviewVerificationError,
};

struct FinalizationIndexes<'a> {
    reviews: BTreeMap<&'a StableId, &'a ReviewerCaseReview>,
    challenges: BTreeMap<&'a StableId, &'a ChallengeOutcome>,
    finalizations: BTreeMap<&'a StableId, &'a FinalizedCase>,
}

struct Inputs<'a> {
    artifact: &'a EngineeringReviewV2,
    definitions: &'a BTreeMap<&'a StableId, &'a ProbeDefinition>,
    reviews: &'a BTreeMap<&'a StableId, &'a ReviewerCaseReview>,
    case_id: &'a StableId,
    case: &'a EngineeringCase,
    challenge: Option<&'a ChallengeOutcome>,
}

pub(super) fn verify_challenges_and_finalization<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
) -> Result<(), ReviewVerificationError> {
    let indexes = finalization_indexes(artifact)?;
    if indexes.finalizations.len() != cases.len() {
        return Err(ReviewVerificationError::Reference(
            "every case requires one terminal finalization".to_string(),
        ));
    }
    for (case_id, case) in cases {
        verify_case_finalization(artifact, definitions, &indexes, case_id, case)?;
    }
    verify_challenger_bindings(artifact, cases)
}

fn finalization_indexes(
    artifact: &EngineeringReviewV2,
) -> Result<FinalizationIndexes<'_>, ReviewVerificationError> {
    Ok(FinalizationIndexes {
        reviews: unique_by(
            &artifact.reviewer_runs,
            |review| &review.case_id,
            "reviewer case review",
        )?,
        challenges: unique_by(
            &artifact.challenges,
            |outcome| &outcome.challenge.case_id,
            "case challenge",
        )?,
        finalizations: unique_by(
            &artifact.execution_truth,
            |finalized| &finalized.case_id,
            "case finalization",
        )?,
    })
}

fn verify_case_finalization<'a>(
    artifact: &'a EngineeringReviewV2,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
    indexes: &FinalizationIndexes<'a>,
    case_id: &'a StableId,
    case: &'a EngineeringCase,
) -> Result<(), ReviewVerificationError> {
    let finalized = indexes.finalizations.get(case_id).copied().ok_or_else(|| {
        ReviewVerificationError::Reference("case finalization is absent".to_string())
    })?;
    let challenge = indexes.challenges.get(case_id).copied();
    let recomputed = match finalized.basis {
        FinalizationBasis::Reviewer => recompute_reviewer(Inputs {
            artifact,
            definitions,
            reviews: &indexes.reviews,
            case_id,
            case,
            challenge,
        })?,
        FinalizationBasis::Deterministic(disposition) => {
            if indexes.reviews.contains_key(case_id) || challenge.is_some() {
                return Err(ReviewVerificationError::Finalization(
                    "deterministic case carries provider-owned review state".to_string(),
                ));
            }
            verify_deterministic_basis(artifact, case_id, disposition)?;
            FinalizedCase::deterministic(case, disposition)
        }
        FinalizationBasis::Failure(failure) => {
            if indexes.reviews.contains_key(case_id)
                || challenge.is_some()
                || finalized.closure.failure != Some(failure)
            {
                return Err(ReviewVerificationError::Finalization(
                    "failed case carries inconsistent execution state".to_string(),
                ));
            }
            FinalizedCase::failed(case, finalized.closure.clone())
        }
    }
    .map_err(|error| ReviewVerificationError::Finalization(error.to_string()))?;
    (finalized == &recomputed).then_some(()).ok_or_else(|| {
        ReviewVerificationError::Finalization("case finalization is stale".to_string())
    })
}

fn recompute_reviewer(
    context: Inputs<'_>,
) -> Result<Result<FinalizedCase, FinalizationError>, ReviewVerificationError> {
    let Inputs {
        artifact,
        definitions,
        reviews,
        case_id,
        case,
        challenge,
    } = context;
    let review = reviews
        .get(case_id)
        .copied()
        .ok_or_else(|| ReviewVerificationError::Reference("case review is absent".to_string()))?;
    verify_case_challenge(ChallengeVerification {
        artifact,
        case_id,
        case,
        review,
        challenge,
        definitions,
    })?;
    let allowed = definitions
        .values()
        .filter(|definition| definition.case_id == *case_id)
        .map(|definition| definition.probe_id.clone())
        .collect();
    Ok(FinalizedCase::finalize(CaseFinalizationContext {
        case,
        review,
        challenge: challenge.map(|outcome| &outcome.challenge),
        response: challenge.and_then(|outcome| outcome.response.as_ref()),
        allowed_probe_ids: &allowed,
    }))
}

fn verify_challenger_bindings(
    artifact: &EngineeringReviewV2,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
) -> Result<(), ReviewVerificationError> {
    for outcome in &artifact.challenges {
        let case = cases.get(&outcome.challenge.case_id).ok_or_else(|| {
            ReviewVerificationError::Provider("challenge references an unknown case".to_string())
        })?;
        let qualified = artifact.qualification_records.iter().any(|qualification| {
            qualification.is_qualified()
                && qualification.binding.target == outcome.challenge.route_target
                && qualification.binding.case_kind == case.case_kind
                && qualification.binding.role == ReviewRole::Challenger
        });
        if !qualified {
            return Err(ReviewVerificationError::Provider(
                "challenge has no qualified challenger binding".to_string(),
            ));
        }
    }
    Ok(())
}

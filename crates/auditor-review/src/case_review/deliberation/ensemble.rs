use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CaseEvidenceRequest, ChallengeOutcome, ContentDigest, EnsembleComparison, ReviewerCaseReview,
    RouteTarget, StableId,
};

pub const ENSEMBLE_CASE_RUN_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnsembleReviewerSlot {
    ReviewerA,
    ReviewerB,
    ReviewerC,
}

impl EnsembleReviewerSlot {
    pub const ALL: [Self; 3] = [Self::ReviewerA, Self::ReviewerB, Self::ReviewerC];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnsembleIndependence {
    SameModelRepeated,
    CrossModelSameProvider,
    CrossProvider,
    CrossPromptSameModel,
    MixedIndependence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnsembleFailureStage {
    PassA,
    EvidenceUnion,
    SharedPacket,
    PassB,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleCaseFailure {
    pub schema_version: u32,
    pub case_id: StableId,
    pub stage: EnsembleFailureStage,
    pub completed_slots: Vec<EnsembleReviewerSlot>,
    pub detail_code: String,
    pub shadow_only: bool,
}

impl EnsembleCaseFailure {
    pub fn new(
        case_id: StableId,
        stage: EnsembleFailureStage,
        mut completed_slots: Vec<EnsembleReviewerSlot>,
        detail_code: impl Into<String>,
    ) -> Result<Self, String> {
        completed_slots.sort();
        completed_slots.dedup();
        let failure = Self {
            schema_version: ENSEMBLE_CASE_RUN_SCHEMA_VERSION,
            case_id,
            stage,
            completed_slots,
            detail_code: detail_code.into(),
            shadow_only: true,
        };
        failure.validate()?;
        Ok(failure)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != ENSEMBLE_CASE_RUN_SCHEMA_VERSION
            || !self.shadow_only
            || self.detail_code.trim().is_empty()
            || self
                .completed_slots
                .iter()
                .any(|slot| !EnsembleReviewerSlot::ALL.contains(slot))
        {
            return Err("ensemble failure record is invalid".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleReviewerPass {
    pub reviewer: EnsembleReviewerSlot,
    pub pass: u8,
    pub packet_digest: ContentDigest,
    pub route_target: RouteTarget,
    pub prompt_identity: StableId,
    pub evidence_requests: Vec<CaseEvidenceRequest>,
    pub review: ReviewerCaseReview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleCaseRun {
    pub schema_version: u32,
    pub case_id: StableId,
    pub independence: EnsembleIndependence,
    pub initial_packet_digest: ContentDigest,
    pub shared_packet_digest: ContentDigest,
    pub pass_a: Vec<EnsembleReviewerPass>,
    pub pass_b: Vec<EnsembleReviewerPass>,
    pub shadow_only: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleChallengeExecution {
    pub schema_version: u32,
    pub comparison_id: StableId,
    pub case_id: StableId,
    pub reviewer: EnsembleReviewerSlot,
    pub reviewer_review_id: StableId,
    pub target_ids: Vec<StableId>,
    pub outcome: Option<ChallengeOutcome>,
    pub failure_code: Option<String>,
    pub shadow_only: bool,
}

impl EnsembleChallengeExecution {
    pub fn complete(
        comparison: &EnsembleComparison,
        reviewer: EnsembleReviewerSlot,
        reviewer_review_id: StableId,
        target_ids: Vec<StableId>,
        outcome: ChallengeOutcome,
    ) -> Self {
        Self::new(
            comparison,
            reviewer,
            reviewer_review_id,
            target_ids,
            ChallengeTerminal::Complete(Box::new(outcome)),
        )
    }

    pub fn failed(
        comparison: &EnsembleComparison,
        reviewer: EnsembleReviewerSlot,
        reviewer_review_id: StableId,
        target_ids: Vec<StableId>,
        failure_code: impl Into<String>,
    ) -> Self {
        Self::new(
            comparison,
            reviewer,
            reviewer_review_id,
            target_ids,
            ChallengeTerminal::Failed(failure_code.into()),
        )
    }

    fn new(
        comparison: &EnsembleComparison,
        reviewer: EnsembleReviewerSlot,
        reviewer_review_id: StableId,
        mut target_ids: Vec<StableId>,
        terminal: ChallengeTerminal,
    ) -> Self {
        target_ids.sort();
        target_ids.dedup();
        let (outcome, failure_code) = match terminal {
            ChallengeTerminal::Complete(outcome) => (Some(*outcome), None),
            ChallengeTerminal::Failed(code) => (None, Some(code)),
        };
        Self {
            schema_version: ENSEMBLE_CASE_RUN_SCHEMA_VERSION,
            comparison_id: comparison.comparison_id.clone(),
            case_id: comparison.case_id.clone(),
            reviewer,
            reviewer_review_id,
            target_ids,
            outcome,
            failure_code,
            shadow_only: true,
        }
    }

    pub fn validate(
        &self,
        run: &EnsembleCaseRun,
        comparison: &EnsembleComparison,
    ) -> Result<(), String> {
        let Some(pass) = run
            .pass_b
            .iter()
            .find(|pass| pass.reviewer == self.reviewer)
        else {
            return Err("ensemble challenge reviewer has no Pass B review".to_string());
        };
        let allowed = comparison
            .challenge_targets
            .iter()
            .map(|target| &target.target_id)
            .collect::<BTreeSet<_>>();
        let owned = pass.review.target_ids();
        let expected_targets = allowed
            .intersection(&owned)
            .map(|target| (*target).clone())
            .collect::<Vec<_>>();
        let mut normalized = self.target_ids.clone();
        normalized.sort();
        normalized.dedup();
        let terminal_state_valid = match (&self.outcome, &self.failure_code) {
            (Some(outcome), None) => {
                outcome.challenge.case_id == self.case_id
                    && outcome.challenge.reviewer_review_id == self.reviewer_review_id
                    && outcome
                        .challenge
                        .objections
                        .iter()
                        .all(|objection| self.target_ids.contains(&objection.target_id))
            }
            (None, Some(code)) => !code.trim().is_empty(),
            _ => false,
        };
        if self.schema_version != ENSEMBLE_CASE_RUN_SCHEMA_VERSION
            || !self.shadow_only
            || self.case_id != run.case_id
            || self.case_id != comparison.case_id
            || self.comparison_id != comparison.comparison_id
            || self.reviewer_review_id != pass.review.review_id
            || self.target_ids.is_empty()
            || normalized != self.target_ids
            || self.target_ids != expected_targets
            || self
                .target_ids
                .iter()
                .any(|target| !allowed.contains(target) || !owned.contains(target))
            || !terminal_state_valid
        {
            return Err("ensemble challenge execution is stale or unbound".to_string());
        }
        Ok(())
    }
}

enum ChallengeTerminal {
    Complete(Box<ChallengeOutcome>),
    Failed(String),
}

impl EnsembleCaseRun {
    pub fn build(
        case_id: StableId,
        initial_packet_digest: ContentDigest,
        shared_packet_digest: ContentDigest,
        mut pass_a: Vec<EnsembleReviewerPass>,
        mut pass_b: Vec<EnsembleReviewerPass>,
    ) -> Result<Self, String> {
        normalize_passes(&mut pass_a);
        normalize_passes(&mut pass_b);
        validate_passes(&case_id, 1, &initial_packet_digest, &pass_a)?;
        validate_passes(&case_id, 2, &shared_packet_digest, &pass_b)?;
        let independence = classify_ensemble_independence(&pass_a);
        Ok(Self {
            schema_version: ENSEMBLE_CASE_RUN_SCHEMA_VERSION,
            case_id,
            independence,
            initial_packet_digest,
            shared_packet_digest,
            pass_a,
            pass_b,
            shadow_only: true,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != ENSEMBLE_CASE_RUN_SCHEMA_VERSION || !self.shadow_only {
            return Err("ensemble run must be schema v1 and shadow-only".to_string());
        }
        validate_passes(&self.case_id, 1, &self.initial_packet_digest, &self.pass_a)?;
        validate_passes(&self.case_id, 2, &self.shared_packet_digest, &self.pass_b)?;
        if self.independence != classify_ensemble_independence(&self.pass_a) {
            return Err("ensemble independence classification is stale".to_string());
        }
        Ok(())
    }
}

fn validate_passes(
    case_id: &StableId,
    expected_pass: u8,
    packet_digest: &ContentDigest,
    passes: &[EnsembleReviewerPass],
) -> Result<(), String> {
    let slots = passes
        .iter()
        .map(|pass| pass.reviewer)
        .collect::<BTreeSet<_>>();
    if passes.len() != 3 || slots != EnsembleReviewerSlot::ALL.into_iter().collect() {
        return Err("ensemble requires exactly reviewer A, B, and C".to_string());
    }
    if passes.iter().any(|pass| {
        pass.pass != expected_pass
            || &pass.packet_digest != packet_digest
            || pass.review.case_id != *case_id
            || pass.review.packet_digest != *packet_digest
            || pass.review.route_target != pass.route_target
    }) {
        return Err("ensemble pass is not bound to its case, packet, route, or pass".to_string());
    }
    Ok(())
}

fn normalize_passes(passes: &mut [EnsembleReviewerPass]) {
    passes.sort_by_key(|pass| pass.reviewer);
    for pass in passes {
        pass.evidence_requests.sort_by(|left, right| {
            (&left.probe_id, &left.target_id, &left.decision_relevance).cmp(&(
                &right.probe_id,
                &right.target_id,
                &right.decision_relevance,
            ))
        });
        pass.evidence_requests.dedup();
    }
}

pub fn classify_ensemble_independence(passes: &[EnsembleReviewerPass]) -> EnsembleIndependence {
    let providers = passes
        .iter()
        .map(|pass| pass.route_target.provider_id.as_str())
        .collect::<BTreeSet<_>>();
    let models = passes
        .iter()
        .map(|pass| pass.route_target.model.as_str())
        .collect::<BTreeSet<_>>();
    let prompts = passes
        .iter()
        .map(|pass| pass.prompt_identity.as_str())
        .collect::<BTreeSet<_>>();
    match (providers.len(), models.len(), prompts.len()) {
        (1, 1, 1) => EnsembleIndependence::SameModelRepeated,
        (1, 1, count) if count > 1 => EnsembleIndependence::CrossPromptSameModel,
        (1, count, _) if count > 1 => EnsembleIndependence::CrossModelSameProvider,
        (3, _, _) => EnsembleIndependence::CrossProvider,
        _ => EnsembleIndependence::MixedIndependence,
    }
}

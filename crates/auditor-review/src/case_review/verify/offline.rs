use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    CaseProviderCallRecord, CaseQualificationRecord, ChallengeRecord, ChallengeResponse,
    ClosureState, ContentDigest, ContextualRecommendation, DeterministicDisposition,
    EgressManifest, EngineeringCase, EngineeringClaim, EnsembleCaseFailure, EnsembleCaseRun,
    EnsembleChallengeExecution, EnsembleComparison, EvidencePacket, FinalizedCase,
    FindingDispositionRecord, InputBinding, ProbeAnswer, ProbeDefinition, ProbeOrigin, ProbeResult,
    ReviewAgenda, ReviewInput, ReviewerAdmissionContext, ReviewerCaseReview,
    ReviewerCaseReviewPayload, StableId,
};

pub(super) use super::artifact_support::unique_by;
use super::artifact_support::{
    complete_finding_dispositions, semantic_index, sort_draft, verify_egress,
    verify_finding_dispositions,
};
use super::challenge_normalization::{
    verify_normalized_challenge, NormalizedChallengeVerification,
};

pub const ENGINEERING_REVIEW_V2_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverageV2 {
    pub report_finding_count: usize,
    pub selected_finding_count: usize,
    pub planned_case_count: usize,
    pub terminal_case_count: usize,
    pub challenge_count: usize,
    pub challenge_response_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeOutcome {
    pub challenge: ChallengeRecord,
    pub response: Option<ChallengeResponse>,
}

impl ChallengeOutcome {
    /// The replacement recommendations the challenge response proposes.
    pub fn replacement_recommendations(&self) -> Vec<crate::ContextualRecommendation> {
        self.response
            .iter()
            .flat_map(|response| &response.responses)
            .filter_map(|response| response.replacement_recommendation.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineVerification {
    pub verified: bool,
    pub checks: Vec<String>,
}

impl OfflineVerification {
    fn pending() -> Self {
        Self {
            verified: false,
            checks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringReviewV2 {
    pub schema_version: u32,
    pub review_id: StableId,
    pub input_binding: InputBinding,
    pub coverage: ReviewCoverageV2,
    pub agenda: ReviewAgenda,
    pub cases: Vec<EngineeringCase>,
    pub evidence_packets: Vec<EvidencePacket>,
    #[serde(default)]
    pub evidence_packet_history: Vec<EvidencePacket>,
    pub probe_definitions: Vec<ProbeDefinition>,
    pub probe_results: Vec<ProbeResult>,
    pub reviewer_runs: Vec<ReviewerCaseReview>,
    #[serde(default)]
    pub ensemble_runs: Vec<EnsembleCaseRun>,
    #[serde(default)]
    pub ensemble_failures: Vec<EnsembleCaseFailure>,
    #[serde(default)]
    pub ensemble_evidence_packets: Vec<EvidencePacket>,
    pub ensemble_comparisons: Vec<EnsembleComparison>,
    pub ensemble_challenges: Vec<EnsembleChallengeExecution>,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    pub challenges: Vec<ChallengeOutcome>,
    pub finding_dispositions: Vec<FindingDispositionRecord>,
    pub portfolio: crate::ReviewPortfolio,
    pub execution_truth: Vec<FinalizedCase>,
    pub qualification_records: Vec<CaseQualificationRecord>,
    pub provider_records: Vec<CaseProviderCallRecord>,
    pub egress_manifests: Vec<EgressManifest>,
    #[serde(default)]
    pub claim_slot_dispositions: Vec<crate::TaskDeclineDisposition>,
    #[serde(default)]
    pub recommendation_family_dispositions: Vec<crate::TaskDeclineDisposition>,
    #[serde(default)]
    pub challenge_target_dispositions: Vec<crate::ChallengeTargetDispositionRecord>,
    #[serde(default)]
    pub semantic_revisions: Vec<crate::SemanticRevisionRecord>,
    #[serde(default)]
    pub projection_audits: Vec<crate::ModelProjectionAudit>,
    pub verification: OfflineVerification,
}

pub struct EngineeringReviewV2Draft {
    pub agenda: ReviewAgenda,
    pub cases: Vec<EngineeringCase>,
    pub evidence_packets: Vec<EvidencePacket>,
    pub evidence_packet_history: Vec<EvidencePacket>,
    pub probe_definitions: Vec<ProbeDefinition>,
    pub probe_results: Vec<ProbeResult>,
    pub reviewer_runs: Vec<ReviewerCaseReview>,
    pub ensemble_runs: Vec<EnsembleCaseRun>,
    pub ensemble_failures: Vec<EnsembleCaseFailure>,
    pub ensemble_evidence_packets: Vec<EvidencePacket>,
    pub ensemble_challenges: Vec<EnsembleChallengeExecution>,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    pub challenges: Vec<ChallengeOutcome>,
    pub execution_truth: Vec<FinalizedCase>,
    pub qualification_records: Vec<CaseQualificationRecord>,
    pub provider_records: Vec<CaseProviderCallRecord>,
    pub egress_manifests: Vec<EgressManifest>,
    pub claim_slot_dispositions: Vec<crate::TaskDeclineDisposition>,
    pub recommendation_family_dispositions: Vec<crate::TaskDeclineDisposition>,
    pub challenge_target_dispositions: Vec<crate::ChallengeTargetDispositionRecord>,
    pub semantic_revisions: Vec<crate::SemanticRevisionRecord>,
    pub projection_audits: Vec<crate::ModelProjectionAudit>,
}

pub fn parse_engineering_review_v2(
    text: &str,
) -> Result<EngineeringReviewV2, ReviewVerificationError> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        ReviewVerificationError::Contract(format!(
            "invalid engineering review v2 artifact: {error}"
        ))
    })?;
    if value.get("schema_version").and_then(Value::as_u64)
        != Some(u64::from(ENGINEERING_REVIEW_V2_SCHEMA_VERSION))
    {
        return Err(ReviewVerificationError::Contract(
            "unsupported engineering review artifact version; only v2 is accepted".to_string(),
        ));
    }
    let artifact: EngineeringReviewV2 = serde_json::from_value(value).map_err(|error| {
        ReviewVerificationError::Contract(format!(
            "invalid engineering review v2 artifact: {error}"
        ))
    })?;
    if artifact.review_id != artifact.expected_id()? || !artifact.verification.verified {
        return Err(ReviewVerificationError::Identity(
            "engineering review v2 artifact identity or verification record is stale".to_string(),
        ));
    }
    Ok(artifact)
}

impl EngineeringReviewV2 {
    pub fn build(
        input: &ReviewInput,
        mut draft: EngineeringReviewV2Draft,
    ) -> Result<Self, ReviewVerificationError> {
        let production_case_ids = draft
            .cases
            .iter()
            .filter(|case| case.template_status == crate::TemplateStatus::Production)
            .map(|case| case.case_id.clone())
            .collect::<BTreeSet<_>>();
        draft.recommendations.extend(
            draft
                .execution_truth
                .iter()
                .flat_map(|case| &case.accepted_recommendations)
                .cloned(),
        );
        sort_draft(&mut draft);
        draft
            .recommendations
            .dedup_by(|left, right| left.recommendation_id == right.recommendation_id);
        let finding_dispositions = complete_finding_dispositions(
            input,
            &draft
                .execution_truth
                .iter()
                .filter(|finalized| production_case_ids.contains(&finalized.case_id))
                .cloned()
                .collect::<Vec<_>>(),
        )?;
        let ensemble_comparisons = draft
            .ensemble_runs
            .iter()
            .map(|run| {
                let deterministic = draft
                    .probe_results
                    .iter()
                    .filter(|result| {
                        result.case_id == run.case_id && result.reviewer_run_id.is_none()
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                crate::compare_ensemble_run(run, &deterministic)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReviewVerificationError::Contract)?;
        let portfolio = crate::PortfolioAssembler
            .assemble(
                &draft.execution_truth,
                &draft.cases,
                &ensemble_comparisons,
                crate::PortfolioAssemblyDirectives::default(),
            )
            .map_err(ReviewVerificationError::Contract)?;
        let selected_finding_count = draft
            .cases
            .iter()
            .flat_map(|case| &case.finding_ids)
            .collect::<BTreeSet<_>>()
            .len();
        let coverage = ReviewCoverageV2 {
            report_finding_count: input.report()["findings"].as_array().map_or(0, Vec::len),
            selected_finding_count,
            planned_case_count: draft.cases.len(),
            terminal_case_count: draft.execution_truth.len(),
            challenge_count: draft.challenges.len(),
            challenge_response_count: draft
                .challenges
                .iter()
                .filter(|outcome| outcome.response.is_some())
                .count(),
        };
        let mut artifact = Self {
            schema_version: ENGINEERING_REVIEW_V2_SCHEMA_VERSION,
            review_id: StableId::from_identity(
                "engineering-review",
                ENGINEERING_REVIEW_V2_SCHEMA_VERSION,
                &json!({"pending": true}),
            )
            .expect("engineering-review is a valid typed prefix"),
            input_binding: input.binding().clone(),
            coverage,
            agenda: draft.agenda,
            cases: draft.cases,
            evidence_packets: draft.evidence_packets,
            evidence_packet_history: draft.evidence_packet_history,
            probe_definitions: draft.probe_definitions,
            probe_results: draft.probe_results,
            reviewer_runs: draft.reviewer_runs,
            ensemble_runs: draft.ensemble_runs,
            ensemble_failures: draft.ensemble_failures,
            ensemble_evidence_packets: draft.ensemble_evidence_packets,
            ensemble_comparisons,
            ensemble_challenges: draft.ensemble_challenges,
            claims: draft.claims,
            recommendations: draft.recommendations,
            challenges: draft.challenges,
            finding_dispositions,
            portfolio,
            execution_truth: draft.execution_truth,
            qualification_records: draft.qualification_records,
            provider_records: draft.provider_records,
            egress_manifests: draft.egress_manifests,
            claim_slot_dispositions: draft.claim_slot_dispositions,
            recommendation_family_dispositions: draft.recommendation_family_dispositions,
            challenge_target_dispositions: draft.challenge_target_dispositions,
            semantic_revisions: draft.semantic_revisions,
            projection_audits: draft.projection_audits,
            verification: OfflineVerification::pending(),
        };
        artifact.review_id = artifact.expected_id()?;
        artifact.verification = verify_engineering_review_v2(&artifact, input)?;
        Ok(artifact)
    }

    pub fn expected_id(&self) -> Result<StableId, ReviewVerificationError> {
        StableId::from_identity(
            "engineering-review",
            ENGINEERING_REVIEW_V2_SCHEMA_VERSION,
            &json!({
                "agenda": self.agenda,
                "cases": self.cases,
                "challenges": self.challenges,
                "claims": self.claims,
                "coverage": self.coverage,
                "egress_manifests": self.egress_manifests,
                "ensemble_comparisons": self.ensemble_comparisons,
                "ensemble_runs": self.ensemble_runs,
                "ensemble_failures": self.ensemble_failures,
                "ensemble_evidence_packets": self.ensemble_evidence_packets,
                "ensemble_challenges": self.ensemble_challenges,
                "evidence_packets": self.evidence_packets,
                "evidence_packet_history": self.evidence_packet_history,
                "execution_truth": self.execution_truth,
                "finding_dispositions": self.finding_dispositions,
                "input_binding": self.input_binding,
                "portfolio": self.portfolio,
                "probe_definitions": self.probe_definitions,
                "probe_results": self.probe_results,
                "provider_records": self.provider_records,
                "qualification_records": self.qualification_records,
                "recommendations": self.recommendations,
                "reviewer_runs": self.reviewer_runs,
                "claim_slot_dispositions": self.claim_slot_dispositions,
                "recommendation_family_dispositions": self.recommendation_family_dispositions,
                "challenge_target_dispositions": self.challenge_target_dispositions,
                "semantic_revisions": self.semantic_revisions,
                "projection_audits": self.projection_audits,
            }),
        )
        .map_err(|error| ReviewVerificationError::Identity(error.to_string()))
    }
}

pub fn verify_engineering_review_v2(
    artifact: &EngineeringReviewV2,
    input: &ReviewInput,
) -> Result<OfflineVerification, ReviewVerificationError> {
    verify_artifact_identity(artifact, input)?;
    let cases = unique_by(&artifact.cases, |case| &case.case_id, "case")?;
    let packets = unique_by(
        &artifact.evidence_packets,
        |packet| &packet.case_id,
        "evidence packet",
    )?;
    let packet_versions = verify_packet_versions(artifact, input, &cases, &packets)?;
    for case in &artifact.cases {
        packets.get(&case.case_id).ok_or_else(|| {
            ReviewVerificationError::Reference("case packet is absent".to_string())
        })?;
    }
    let definitions = unique_by(
        &artifact.probe_definitions,
        |definition| &definition.probe_id,
        "probe definition",
    )?;
    verify_probe_results(artifact, &cases, &packets, &definitions)?;
    verify_reviews(artifact, &cases, &packets, &definitions)?;
    verify_ensemble_runs(artifact, &cases, &packet_versions, &definitions)?;
    super::finalization::verify_challenges_and_finalization(artifact, &cases, &definitions)?;
    verify_finding_dispositions(artifact, input)?;
    artifact
        .portfolio
        .validate(
            &artifact.execution_truth,
            &artifact.cases,
            &artifact.ensemble_comparisons,
        )
        .map_err(ReviewVerificationError::Contract)?;
    super::provider_checks::verify_provider_records(artifact, &cases, &packet_versions)?;
    verify_egress(&artifact.egress_manifests)?;
    super::protocol_audits::verify_protocol_audits(artifact)?;
    verify_coverage(artifact, input)?;
    Ok(OfflineVerification {
        verified: true,
        checks: vec![
            "artifact_identity".to_string(),
            "case_evidence_probe_graph".to_string(),
            "shadow_ensemble_bindings".to_string(),
            "claims_recommendations_challenges".to_string(),
            "finding_immutability_and_dispositions".to_string(),
            "provider_qualification_route_and_egress".to_string(),
            "native_task_dispositions_revisions_and_projections".to_string(),
        ],
    })
}

fn verify_ensemble_runs<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    packet_versions: &BTreeMap<&'a ContentDigest, &'a EvidencePacket>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
) -> Result<(), ReviewVerificationError> {
    let index = verify_ensemble_records(artifact, cases, packet_versions, definitions)?;
    verify_ensemble_comparisons(artifact, &index.runs_by_case)?;
    verify_ensemble_challenges(artifact, cases, definitions, &index)?;
    Ok(())
}

struct EnsembleVerificationIndex<'a> {
    packet_versions: BTreeMap<&'a ContentDigest, &'a EvidencePacket>,
    runs_by_case: BTreeMap<&'a StableId, &'a EnsembleCaseRun>,
}

/// The packet versions plus the ensemble evidence packets, each validated
/// and bound to a known case exactly once.
fn ensemble_packet_versions<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    packet_versions: &BTreeMap<&'a ContentDigest, &'a EvidencePacket>,
) -> Result<BTreeMap<&'a ContentDigest, &'a EvidencePacket>, ReviewVerificationError> {
    let mut versions = packet_versions.clone();
    for packet in &artifact.ensemble_evidence_packets {
        packet
            .validate()
            .map_err(|error| ReviewVerificationError::Evidence(error.to_string()))?;
        if !cases.contains_key(&packet.case_id)
            || versions.insert(&packet.packet_digest, packet).is_some()
        {
            return Err(ReviewVerificationError::Evidence(
                "ensemble packet is unbound or duplicated".to_string(),
            ));
        }
    }
    Ok(versions)
}

fn verify_ensemble_records<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    packet_versions: &BTreeMap<&'a ContentDigest, &'a EvidencePacket>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
) -> Result<EnsembleVerificationIndex<'a>, ReviewVerificationError> {
    let ensemble_packet_versions = ensemble_packet_versions(artifact, cases, packet_versions)?;
    let mut case_ids = BTreeSet::new();
    let mut runs_by_case = BTreeMap::new();
    let production_review_ids = artifact
        .execution_truth
        .iter()
        .filter_map(|finalized| finalized.reviewer_review_id.as_ref())
        .collect::<BTreeSet<_>>();
    for run in &artifact.ensemble_runs {
        run.validate().map_err(ReviewVerificationError::Contract)?;
        if !cases.contains_key(&run.case_id)
            || !case_ids.insert(&run.case_id)
            || runs_by_case.insert(&run.case_id, run).is_some()
            || !ensemble_packet_versions.contains_key(&run.initial_packet_digest)
            || !ensemble_packet_versions.contains_key(&run.shared_packet_digest)
        {
            return Err(ReviewVerificationError::Reference(
                "ensemble run is duplicated or references an unknown case or packet".to_string(),
            ));
        }
        if run
            .pass_a
            .iter()
            .chain(&run.pass_b)
            .any(|pass| production_review_ids.contains(&pass.review.review_id))
        {
            return Err(ReviewVerificationError::Contract(
                "shadow ensemble output cannot become execution truth".to_string(),
            ));
        }
        let case = cases[&run.case_id];
        let case_definitions = definitions
            .values()
            .filter(|definition| definition.case_id == run.case_id)
            .copied()
            .cloned()
            .collect::<Vec<_>>();
        for pass in run.pass_a.iter().chain(&run.pass_b) {
            let packet = ensemble_packet_versions
                .get(&pass.packet_digest)
                .copied()
                .ok_or_else(|| {
                    ReviewVerificationError::Reference(
                        "ensemble review packet is absent".to_string(),
                    )
                })?;
            verify_normalized_review(
                &pass.review,
                case,
                packet,
                &case_definitions,
                &artifact.probe_results,
            )?;
        }
    }
    for failure in &artifact.ensemble_failures {
        failure
            .validate()
            .map_err(ReviewVerificationError::Contract)?;
        if !cases.contains_key(&failure.case_id) || !case_ids.insert(&failure.case_id) {
            return Err(ReviewVerificationError::Reference(
                "ensemble success/failure records must be unique per case".to_string(),
            ));
        }
    }
    Ok(EnsembleVerificationIndex {
        packet_versions: ensemble_packet_versions,
        runs_by_case,
    })
}

fn verify_ensemble_comparisons<'a>(
    artifact: &'a EngineeringReviewV2,
    runs_by_case: &BTreeMap<&'a StableId, &'a EnsembleCaseRun>,
) -> Result<(), ReviewVerificationError> {
    if artifact.ensemble_comparisons.len() != artifact.ensemble_runs.len() {
        return Err(ReviewVerificationError::Contract(
            "every complete ensemble run requires exactly one comparison".to_string(),
        ));
    }
    for comparison in &artifact.ensemble_comparisons {
        let run = runs_by_case
            .get(&comparison.case_id)
            .copied()
            .ok_or_else(|| {
                ReviewVerificationError::Reference(
                    "ensemble comparison has no complete run".to_string(),
                )
            })?;
        let deterministic = artifact
            .probe_results
            .iter()
            .filter(|result| result.case_id == run.case_id && result.reviewer_run_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        comparison
            .validate(run, &deterministic)
            .map_err(ReviewVerificationError::Contract)?;
    }
    Ok(())
}

fn verify_ensemble_challenges<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
    index: &EnsembleVerificationIndex<'a>,
) -> Result<(), ReviewVerificationError> {
    let mut execution_keys = BTreeSet::new();
    for execution in &artifact.ensemble_challenges {
        let run = index
            .runs_by_case
            .get(&execution.case_id)
            .copied()
            .ok_or_else(|| {
                ReviewVerificationError::Reference(
                    "ensemble challenge has no complete run".to_string(),
                )
            })?;
        let comparison = artifact
            .ensemble_comparisons
            .iter()
            .find(|comparison| comparison.comparison_id == execution.comparison_id)
            .ok_or_else(|| {
                ReviewVerificationError::Reference(
                    "ensemble challenge has no bound comparison".to_string(),
                )
            })?;
        execution
            .validate(run, comparison)
            .map_err(ReviewVerificationError::Contract)?;
        if !execution_keys.insert((execution.case_id.clone(), execution.reviewer)) {
            return Err(ReviewVerificationError::Contract(
                "ensemble challenge executions must be unique per case and reviewer".to_string(),
            ));
        }
        if let Some(outcome) = &execution.outcome {
            let pass = run
                .pass_b
                .iter()
                .find(|pass| pass.reviewer == execution.reviewer)
                .expect("ensemble execution validation bound the reviewer");
            let packet = index
                .packet_versions
                .get(&run.shared_packet_digest)
                .copied()
                .expect("ensemble run validation bound the packet");
            verify_shadow_challenge(
                outcome,
                cases[&run.case_id],
                packet,
                &pass.review,
                definitions,
            )?;
            crate::validate_comparison_recommendation_families(
                &comparison.recommendation_families,
                &outcome.replacement_recommendations(),
            )
            .map_err(ReviewVerificationError::Challenge)?;
        }
    }
    let mut expected_execution_keys = BTreeSet::new();
    for comparison in &artifact.ensemble_comparisons {
        let target_ids = comparison
            .challenge_targets
            .iter()
            .map(|target| &target.target_id)
            .collect::<BTreeSet<_>>();
        let run = index
            .runs_by_case
            .get(&comparison.case_id)
            .copied()
            .expect("comparison validation bound the ensemble run");
        for pass in &run.pass_b {
            let owns_target = pass
                .review
                .target_ids()
                .iter()
                .any(|target| target_ids.contains(*target));
            if owns_target {
                expected_execution_keys.insert((run.case_id.clone(), pass.reviewer));
            }
        }
    }
    if execution_keys != expected_execution_keys {
        return Err(ReviewVerificationError::Contract(
            "every reviewer-owned comparison target requires exactly one ensemble challenge execution"
                .to_string(),
        ));
    }
    Ok(())
}

fn verify_shadow_challenge(
    outcome: &ChallengeOutcome,
    case: &EngineeringCase,
    packet: &EvidencePacket,
    review: &ReviewerCaseReview,
    definitions: &BTreeMap<&StableId, &ProbeDefinition>,
) -> Result<(), ReviewVerificationError> {
    verify_normalized_challenge(NormalizedChallengeVerification {
        outcome,
        case,
        packet,
        review,
        definitions,
        stale_challenge_message: "shadow challenge normalization is stale",
        stale_response_message: "shadow challenge response normalization is stale",
    })
}

fn verify_artifact_identity(
    artifact: &EngineeringReviewV2,
    input: &ReviewInput,
) -> Result<(), ReviewVerificationError> {
    let valid = artifact.schema_version == ENGINEERING_REVIEW_V2_SCHEMA_VERSION
        && artifact.input_binding == *input.binding()
        && artifact.review_id == artifact.expected_id()?;
    if !valid {
        return Err(ReviewVerificationError::Identity(
            "artifact schema, input binding, or identity is stale".to_string(),
        ));
    }
    artifact
        .agenda
        .validate(input)
        .map_err(|error| ReviewVerificationError::Agenda(error.to_string()))
}

fn verify_probe_results<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    packets: &BTreeMap<&'a StableId, &'a EvidencePacket>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
) -> Result<(), ReviewVerificationError> {
    let mut results = BTreeMap::<&StableId, &ProbeResult>::new();
    for result in &artifact.probe_results {
        let definition = definitions.get(&result.probe_id).ok_or_else(|| {
            ReviewVerificationError::Reference("probe result has no definition".to_string())
        })?;
        let case = cases.get(&result.case_id).ok_or_else(|| {
            ReviewVerificationError::Reference("probe result has no case".to_string())
        })?;
        let packet = packets.get(&result.case_id).ok_or_else(|| {
            ReviewVerificationError::Reference("probe result has no packet".to_string())
        })?;
        definition
            .validate(case, packet)
            .map_err(|error| ReviewVerificationError::Probe(error.to_string()))?;
        result
            .validate(definition, packet)
            .map_err(|error| ReviewVerificationError::Probe(error.to_string()))?;
        if results.insert(&result.probe_id, result).is_some() {
            return Err(ReviewVerificationError::Reference(
                "probe has more than one result".to_string(),
            ));
        }
    }
    (results.len() == definitions.len())
        .then_some(())
        .ok_or_else(|| {
            ReviewVerificationError::Reference("not every probe has exactly one result".to_string())
        })
}

fn verify_coverage(
    artifact: &EngineeringReviewV2,
    input: &ReviewInput,
) -> Result<(), ReviewVerificationError> {
    let expected = ReviewCoverageV2 {
        report_finding_count: input.report()["findings"].as_array().map_or(0, Vec::len),
        selected_finding_count: artifact
            .cases
            .iter()
            .flat_map(|case| &case.finding_ids)
            .collect::<BTreeSet<_>>()
            .len(),
        planned_case_count: artifact.cases.len(),
        terminal_case_count: artifact.execution_truth.len(),
        challenge_count: artifact.challenges.len(),
        challenge_response_count: artifact
            .challenges
            .iter()
            .filter(|outcome| outcome.response.is_some())
            .count(),
    };
    (artifact.coverage == expected)
        .then_some(())
        .ok_or(ReviewVerificationError::Coverage)
}

fn verify_packet_versions<'a>(
    artifact: &'a EngineeringReviewV2,
    input: &ReviewInput,
    cases: &BTreeMap<&StableId, &EngineeringCase>,
    final_packets: &BTreeMap<&StableId, &'a EvidencePacket>,
) -> Result<BTreeMap<&'a ContentDigest, &'a EvidencePacket>, ReviewVerificationError> {
    let mut versions = BTreeMap::new();
    for packet in artifact
        .evidence_packets
        .iter()
        .chain(&artifact.evidence_packet_history)
    {
        packet
            .validate()
            .map_err(|error| ReviewVerificationError::Evidence(error.to_string()))?;
        if packet.input_binding_id != input.binding().stable_id()
            || !cases.contains_key(&packet.case_id)
            || versions.insert(&packet.packet_digest, packet).is_some()
        {
            return Err(ReviewVerificationError::Evidence(
                "packet version is stale, unbound, or duplicated".to_string(),
            ));
        }
    }
    let referenced = artifact
        .provider_records
        .iter()
        .map(|record| &record.packet_digest)
        .collect::<BTreeSet<_>>();
    for historical in &artifact.evidence_packet_history {
        if final_packets
            .get(&historical.case_id)
            .is_some_and(|final_packet| final_packet.packet_digest == historical.packet_digest)
            || !referenced.contains(&historical.packet_digest)
        {
            return Err(ReviewVerificationError::Evidence(
                "historical packet must be superseded and referenced by a provider call"
                    .to_string(),
            ));
        }
    }
    Ok(versions)
}

fn verify_reviews<'a>(
    artifact: &'a EngineeringReviewV2,
    cases: &BTreeMap<&'a StableId, &'a EngineeringCase>,
    packets: &BTreeMap<&'a StableId, &'a EvidencePacket>,
    definitions: &BTreeMap<&'a StableId, &'a ProbeDefinition>,
) -> Result<(), ReviewVerificationError> {
    for review in &artifact.reviewer_runs {
        let case = cases.get(&review.case_id).ok_or_else(|| {
            ReviewVerificationError::Reference("review case is absent".to_string())
        })?;
        let packet = packets.get(&review.case_id).ok_or_else(|| {
            ReviewVerificationError::Reference("review packet is absent".to_string())
        })?;
        let case_definitions = definitions
            .values()
            .filter(|definition| definition.case_id == review.case_id)
            .copied()
            .cloned()
            .collect::<Vec<_>>();
        verify_normalized_review(
            review,
            case,
            packet,
            &case_definitions,
            &artifact.probe_results,
        )?;
    }
    let review_claims = semantic_index(
        artifact
            .reviewer_runs
            .iter()
            .flat_map(|review| &review.claims),
        |claim| &claim.claim_id,
    )?;
    let claims = semantic_index(artifact.claims.iter(), |claim| &claim.claim_id)?;
    let review_recommendations = semantic_index(
        artifact
            .reviewer_runs
            .iter()
            .flat_map(|review| &review.recommendations)
            .chain(
                artifact
                    .execution_truth
                    .iter()
                    .flat_map(|case| &case.accepted_recommendations),
            ),
        |recommendation| &recommendation.recommendation_id,
    )?;
    let recommendations = semantic_index(artifact.recommendations.iter(), |recommendation| {
        &recommendation.recommendation_id
    })?;
    if review_claims != claims || review_recommendations != recommendations {
        return Err(ReviewVerificationError::Reference(
            "top-level claim or recommendation index is inconsistent".to_string(),
        ));
    }
    Ok(())
}

fn verify_normalized_review(
    review: &ReviewerCaseReview,
    case: &EngineeringCase,
    packet: &EvidencePacket,
    case_definitions: &[ProbeDefinition],
    probe_results: &[ProbeResult],
) -> Result<(), ReviewVerificationError> {
    let deterministic = probe_results
        .iter()
        .filter(|result| result.case_id == review.case_id && result.reviewer_run_id.is_none())
        .cloned()
        .collect::<Vec<_>>();
    let admitted = ReviewerCaseReview::admit(
        ReviewerCaseReviewPayload {
            case_conclusion: review.case_conclusion,
            case_position: review.case_position,
            model_resolved_probes: review.model_resolved_probes.clone(),
            claims: review.claims.clone(),
            recommendations: review.recommendations.clone(),
            preferred_recommendation_id: review.preferred_recommendation_id.clone(),
            unresolved_gaps: review.unresolved_gaps.clone(),
            proposed_closure: review.proposed_closure.reason,
        },
        ReviewerAdmissionContext {
            reviewer_run_id: &review.reviewer_run_id,
            route_target: &review.route_target,
            case,
            packet,
            probe_definitions: case_definitions,
            deterministic_probe_results: &deterministic,
        },
    )
    .map_err(|error| ReviewVerificationError::Review(error.to_string()))?;
    if admitted != *review {
        return Err(ReviewVerificationError::Review(
            "review normalization is stale".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn verify_deterministic_basis(
    artifact: &EngineeringReviewV2,
    case_id: &StableId,
    disposition: DeterministicDisposition,
) -> Result<(), ReviewVerificationError> {
    let mandatory_ids = artifact
        .probe_definitions
        .iter()
        .filter(|definition| {
            definition.case_id == *case_id && definition.origin == ProbeOrigin::Mandatory
        })
        .map(|definition| &definition.probe_id)
        .collect::<BTreeSet<_>>();
    let mandatory_results = artifact
        .probe_results
        .iter()
        .filter(|result| result.case_id == *case_id && mandatory_ids.contains(&result.probe_id))
        .collect::<Vec<_>>();
    let packet = artifact
        .evidence_packets
        .iter()
        .find(|packet| packet.case_id == *case_id)
        .ok_or_else(|| {
            ReviewVerificationError::Reference(
                "deterministic finalization has no evidence packet".to_string(),
            )
        })?;
    let supported = match disposition {
        DeterministicDisposition::NotSubstantiated => mandatory_results
            .iter()
            .any(|result| matches!(result.answer, ProbeAnswer::No | ProbeAnswer::NotApplicable)),
        DeterministicDisposition::EvidenceUnavailable => {
            packet.closure.state != ClosureState::Closed
                || mandatory_results
                    .iter()
                    .any(|result| result.answer == ProbeAnswer::Unknown)
        }
        DeterministicDisposition::ActionFullyDetermined => {
            !mandatory_results.is_empty()
                && mandatory_results
                    .iter()
                    .all(|result| result.answer.is_definitive())
        }
        DeterministicDisposition::Unresolved => false,
    };
    supported.then_some(()).ok_or_else(|| {
        ReviewVerificationError::Finalization(
            "deterministic finalization is not supported by mandatory probe results".to_string(),
        )
    })
}

pub(super) struct ChallengeVerification<'a> {
    pub(super) artifact: &'a EngineeringReviewV2,
    pub(super) case_id: &'a StableId,
    pub(super) case: &'a EngineeringCase,
    pub(super) review: &'a ReviewerCaseReview,
    pub(super) challenge: Option<&'a ChallengeOutcome>,
    pub(super) definitions: &'a BTreeMap<&'a StableId, &'a ProbeDefinition>,
}

pub(super) fn verify_case_challenge(
    context: ChallengeVerification<'_>,
) -> Result<(), ReviewVerificationError> {
    let ChallengeVerification {
        artifact,
        case_id,
        case,
        review,
        challenge,
        definitions,
    } = context;
    if review.recommendations.iter().any(|recommendation| {
        recommendation.family == crate::RecommendationFamily::LifecycleReownership
    }) && challenge.is_none()
    {
        return Err(ReviewVerificationError::Challenge(
            "lifecycle reownership requires a challenge".to_string(),
        ));
    }
    let Some(outcome) = challenge else {
        return Ok(());
    };
    let packet = artifact
        .evidence_packets
        .iter()
        .find(|packet| packet.case_id == *case_id)
        .ok_or_else(|| {
            ReviewVerificationError::Reference("challenge packet is absent".to_string())
        })?;
    verify_normalized_challenge(NormalizedChallengeVerification {
        outcome,
        case,
        packet,
        review,
        definitions,
        stale_challenge_message: "challenge normalization is stale",
        stale_response_message: "challenge response normalization is stale",
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewVerificationError {
    Contract(String),
    Identity(String),
    Agenda(String),
    Evidence(String),
    Probe(String),
    Review(String),
    Challenge(String),
    Finalization(String),
    Reference(String),
    FindingDisposition,
    Provider(String),
    Egress,
    Coverage,
}

impl fmt::Display for ReviewVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ReviewVerificationError {}

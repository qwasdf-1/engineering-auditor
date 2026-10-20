use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    compare_claims, compare_probe_results, compare_recommendations, ClaimCluster,
    ClaimClusterState, ProbeComparison, ProbeComparisonState, RecommendationFamilyComparison,
    RecommendationFamilyState,
};
use crate::{ContentDigest, EnsembleCaseRun, EnsembleIndependence, EnsembleReviewerSlot, StableId};

pub const ENSEMBLE_COMPARISON_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewComparisonOutcome {
    Converged,
    ConvergedWithVariants,
    MajorityWithMaterialObjection,
    Contested,
    SharedEvidenceGap,
    IncommensurableAlternatives,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonTargetType {
    Claim,
    Recommendation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeTarget {
    pub target_type: ComparisonTargetType,
    pub target_id: StableId,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDisagreement {
    pub dimension: String,
    pub reviewer_slots: Vec<EnsembleReviewerSlot>,
    pub subject_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleComparison {
    pub schema_version: u32,
    pub comparison_id: StableId,
    pub case_id: StableId,
    pub shared_packet_digest: ContentDigest,
    pub independence: EnsembleIndependence,
    pub probes: Vec<ProbeComparison>,
    pub claim_clusters: Vec<ClaimCluster>,
    pub recommendation_families: Vec<RecommendationFamilyComparison>,
    pub outcome: ReviewComparisonOutcome,
    pub material_disagreements: Vec<MaterialDisagreement>,
    pub shared_unknown_probe_ids: Vec<StableId>,
    pub challenge_targets: Vec<ChallengeTarget>,
    pub shadow_only: bool,
}

pub fn compare_ensemble_run(
    run: &EnsembleCaseRun,
    deterministic_probe_results: &[crate::ProbeResult],
) -> Result<EnsembleComparison, String> {
    run.validate()?;
    let reviews = run
        .pass_b
        .iter()
        .map(|pass| (pass.reviewer, &pass.review))
        .collect::<Vec<_>>();
    if deterministic_probe_results
        .iter()
        .any(|result| result.case_id != run.case_id)
    {
        return Err("deterministic comparison probes belong to another case".to_string());
    }
    let deterministic = deterministic_probe_results
        .iter()
        .map(|result| (&result.probe_id, result))
        .collect::<BTreeMap<_, _>>();
    if deterministic.len() != deterministic_probe_results.len() {
        return Err("deterministic comparison probes contain duplicate probe IDs".to_string());
    }
    let probe_ids = reviews
        .iter()
        .flat_map(|(_, review)| {
            review
                .model_resolved_probes
                .iter()
                .map(|item| item.probe_id.clone())
        })
        .chain(deterministic.keys().map(|id| (*id).clone()))
        .collect::<BTreeSet<_>>();
    let mut probes = Vec::new();
    for probe_id in probe_ids {
        let entries = reviews
            .iter()
            .filter_map(|(slot, review)| {
                review
                    .model_resolved_probes
                    .iter()
                    .find(|result| result.probe_id == probe_id)
                    .or_else(|| deterministic.get(&probe_id).copied())
                    .map(|result| (*slot, result))
            })
            .collect::<Vec<_>>();
        probes.push(compare_probe_results(probe_id, &entries)?);
    }
    let claim_inputs = reviews
        .iter()
        .map(|(slot, review)| (*slot, review.claims.as_slice()))
        .collect::<Vec<_>>();
    let recommendation_inputs = reviews
        .iter()
        .map(|(slot, review)| (*slot, review.recommendations.as_slice()))
        .collect::<Vec<_>>();
    let claim_clusters = compare_claims(&claim_inputs);
    let recommendation_families = compare_recommendations(&recommendation_inputs);
    let shared_unknown_probe_ids = probes
        .iter()
        .filter(|probe| probe.state == ProbeComparisonState::SharedUnknown)
        .map(|probe| probe.probe_id.clone())
        .collect::<Vec<_>>();
    let outcome = derive_outcome(
        &probes,
        &claim_clusters,
        &recommendation_families,
        reviews
            .iter()
            .all(|(_, review)| review.recommendations.is_empty()),
        reviews.iter().all(|(_, review)| {
            review.case_position == crate::CasePosition::EvidenceBlocked
                || review.case_conclusion == crate::CaseConclusion::InsufficientEvidence
        }),
    );
    let material_disagreements =
        material_disagreements(&probes, &claim_clusters, &recommendation_families);
    let challenge_targets = challenge_targets(&claim_clusters, &recommendation_families);
    let mut comparison = EnsembleComparison {
        schema_version: ENSEMBLE_COMPARISON_SCHEMA_VERSION,
        comparison_id: StableId::from_identity(
            "ensemble-comparison",
            ENSEMBLE_COMPARISON_SCHEMA_VERSION,
            &json!({"pending": true}),
        )
        .map_err(|error| error.to_string())?,
        case_id: run.case_id.clone(),
        shared_packet_digest: run.shared_packet_digest.clone(),
        independence: run.independence,
        probes,
        claim_clusters,
        recommendation_families,
        outcome,
        material_disagreements,
        shared_unknown_probe_ids,
        challenge_targets,
        shadow_only: true,
    };
    comparison.comparison_id = comparison.expected_id()?;
    Ok(comparison)
}

impl EnsembleComparison {
    pub fn expected_id(&self) -> Result<StableId, String> {
        StableId::from_identity(
            "ensemble-comparison",
            ENSEMBLE_COMPARISON_SCHEMA_VERSION,
            &json!({
                "case_id": self.case_id,
                "challenge_targets": self.challenge_targets,
                "claim_clusters": self.claim_clusters,
                "independence": self.independence,
                "material_disagreements": self.material_disagreements,
                "outcome": self.outcome,
                "probes": self.probes,
                "recommendation_families": self.recommendation_families,
                "shadow_only": self.shadow_only,
                "shared_packet_digest": self.shared_packet_digest,
                "shared_unknown_probe_ids": self.shared_unknown_probe_ids,
            }),
        )
        .map_err(|error| error.to_string())
    }

    pub fn validate(
        &self,
        run: &EnsembleCaseRun,
        deterministic_probe_results: &[crate::ProbeResult],
    ) -> Result<(), String> {
        let expected = compare_ensemble_run(run, deterministic_probe_results)?;
        if self != &expected || !self.shadow_only {
            return Err("ensemble comparison is stale or not shadow-only".to_string());
        }
        Ok(())
    }
}

fn derive_outcome(
    probes: &[ProbeComparison],
    claims: &[ClaimCluster],
    recommendations: &[RecommendationFamilyComparison],
    all_recommendations_empty: bool,
    all_evidence_blocked: bool,
) -> ReviewComparisonOutcome {
    if all_recommendations_empty
        && (all_evidence_blocked
            || (!probes.is_empty()
                && probes
                    .iter()
                    .all(|probe| probe.state == ProbeComparisonState::SharedUnknown)))
    {
        return ReviewComparisonOutcome::SharedEvidenceGap;
    }
    let contested_probe = probes.iter().any(|probe| {
        matches!(
            probe.state,
            ProbeComparisonState::Split
                | ProbeComparisonState::AllDifferent
                | ProbeComparisonState::EvidenceConflict
        )
    });
    let contested_claim = claims.iter().any(|cluster| {
        matches!(
            cluster.state,
            ClaimClusterState::MutuallyExclusive | ClaimClusterState::Unresolved
        )
    });
    let family_counts = recommendations
        .iter()
        .map(|family| {
            family
                .members
                .values()
                .filter(|members| !members.is_empty())
                .count()
        })
        .collect::<Vec<_>>();
    if recommendations.len() >= 3 && family_counts.iter().all(|count| *count == 1) {
        ReviewComparisonOutcome::IncommensurableAlternatives
    } else if recommendations
        .iter()
        .any(|family| family.state == RecommendationFamilyState::Contested)
        && family_counts.contains(&2)
    {
        ReviewComparisonOutcome::MajorityWithMaterialObjection
    } else if contested_probe || contested_claim {
        ReviewComparisonOutcome::Contested
    } else if recommendations
        .iter()
        .any(|family| family.state == RecommendationFamilyState::CompatibleVariant)
        || claims
            .iter()
            .any(|cluster| cluster.state == ClaimClusterState::ScopeVariant)
    {
        ReviewComparisonOutcome::ConvergedWithVariants
    } else {
        ReviewComparisonOutcome::Converged
    }
}

fn material_disagreements(
    probes: &[ProbeComparison],
    claims: &[ClaimCluster],
    recommendations: &[RecommendationFamilyComparison],
) -> Vec<MaterialDisagreement> {
    let all_slots = EnsembleReviewerSlot::ALL.to_vec();
    let mut values = Vec::new();
    values.extend(
        probes
            .iter()
            .filter(|&probe| {
                matches!(
                    probe.state,
                    ProbeComparisonState::Split
                        | ProbeComparisonState::AllDifferent
                        | ProbeComparisonState::EvidenceConflict
                )
            })
            .map(|probe| MaterialDisagreement {
                dimension: "probe".to_string(),
                reviewer_slots: all_slots.clone(),
                subject_ids: vec![probe.probe_id.to_string()],
            }),
    );
    values.extend(
        claims
            .iter()
            .filter(|&cluster| {
                matches!(
                    cluster.state,
                    ClaimClusterState::MutuallyExclusive
                        | ClaimClusterState::ScopeVariant
                        | ClaimClusterState::Unresolved
                )
            })
            .map(|cluster| MaterialDisagreement {
                dimension: "claim".to_string(),
                reviewer_slots: slots_with_members(&cluster.members),
                subject_ids: cluster
                    .members
                    .values()
                    .flatten()
                    .map(ToString::to_string)
                    .collect(),
            }),
    );
    values.extend(
        recommendations
            .iter()
            .filter(|family| family.state != RecommendationFamilyState::Common)
            .map(|family| MaterialDisagreement {
                dimension: "recommendation".to_string(),
                reviewer_slots: slots_with_members(&family.members),
                subject_ids: family
                    .members
                    .values()
                    .flatten()
                    .map(ToString::to_string)
                    .collect(),
            }),
    );
    values.sort_by(|left, right| {
        (&left.dimension, &left.subject_ids).cmp(&(&right.dimension, &right.subject_ids))
    });
    values
}

fn challenge_targets(
    claims: &[ClaimCluster],
    recommendations: &[RecommendationFamilyComparison],
) -> Vec<ChallengeTarget> {
    let mut targets = Vec::new();
    for cluster in claims
        .iter()
        .filter(|cluster| cluster.state != ClaimClusterState::Common)
    {
        targets.extend(
            cluster
                .members
                .values()
                .flatten()
                .cloned()
                .map(|target_id| ChallengeTarget {
                    target_type: ComparisonTargetType::Claim,
                    target_id,
                    reason: format!("claim cluster is {:?}", cluster.state),
                }),
        );
    }
    for family in recommendations
        .iter()
        .filter(|family| family.state != RecommendationFamilyState::Common)
    {
        targets.extend(family.members.values().flatten().cloned().map(|target_id| {
            ChallengeTarget {
                target_type: ComparisonTargetType::Recommendation,
                target_id,
                reason: format!("recommendation family is {:?}", family.state),
            }
        }));
    }
    targets.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    targets.dedup_by(|left, right| left.target_id == right.target_id);
    targets
}

fn slots_with_members(
    members: &BTreeMap<EnsembleReviewerSlot, Vec<StableId>>,
) -> Vec<EnsembleReviewerSlot> {
    members
        .iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(slot, _)| *slot)
        .collect()
}

//! An answer to one question, and the deterministic check that holds it to
//! the question: typed claims and recommendations that cite only issued
//! evidence, plus each kind's shape.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::super::view::source_span;
use super::{Question, QuestionKind};
use crate::{
    validate_claims, validate_recommendations, ClaimSlot, ClaimStatus, ContextualRecommendation,
    EngineeringClaim, EngineeringClaimKind, RecommendationBounds, RecommendationFamily, StableId,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswer {
    pub question_id: StableId,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    /// Subject IDs (hotspots) or recommendation IDs (the plan) in order;
    /// empty for the other kinds.
    #[serde(default)]
    pub ranking: Vec<String>,
}

impl QuestionAnswer {
    /// Every evidence ID the answer's claims and recommendations cite.
    pub(crate) fn cited_ids(&self) -> BTreeSet<&str> {
        let claims = self.claims.iter().flat_map(|claim| {
            claim
                .supporting_evidence_ids
                .iter()
                .chain(&claim.counter_evidence_ids)
        });
        let recommendations = self.recommendations.iter().flat_map(|step| {
            step.scope_ids
                .iter()
                .chain(&step.affected_finding_ids)
                .chain(
                    step.expected_benefits
                        .iter()
                        .flat_map(|benefit| &benefit.evidence_ids),
                )
                .chain(
                    step.verification_steps
                        .iter()
                        .flat_map(|verification| &verification.evidence_ids),
                )
        });
        claims.chain(recommendations).map(String::as_str).collect()
    }
}

/// Whether `id` is a source span inside the lines one known span of the
/// same file returned: the reviewer saw every line it names.
fn within_known_span(known: &BTreeSet<String>, id: &str) -> bool {
    let Some((path, start, end)) = source_span(id) else {
        return false;
    };
    start <= end
        && known
            .iter()
            .filter_map(|known| source_span(known))
            .any(|(shown, from, to)| shown == path && from <= start && end <= to)
}

impl Question {
    /// Checks an answer: claims and recommendations pass the record
    /// validators within this question's slots, families, and scope,
    /// citing only `known_evidence_ids`, what this question shows, or a
    /// source span narrowed to lines inside a shown one; then
    /// the kind's shape. The plan's recommendations may rest on claims
    /// from `earlier` answers to the questions it depends on.
    pub fn check_answer(
        &self,
        answer: &QuestionAnswer,
        known_evidence_ids: &BTreeSet<String>,
        earlier: &[QuestionAnswer],
    ) -> Result<(), String> {
        if answer.question_id != self.question_id {
            return Err(format!(
                "the answer is for {}, not {}",
                answer.question_id.as_str(),
                self.question_id.as_str()
            ));
        }
        let mut known = known_evidence_ids
            .union(&self.evidence_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let narrowed = answer
            .cited_ids()
            .into_iter()
            .filter(|id| !known.contains(*id) && within_known_span(&known, id))
            .map(str::to_string)
            .collect::<Vec<_>>();
        known.extend(narrowed);
        validate_claims(&self.question_id, &self.claim_slots, &known, &answer.claims)?;
        let mut premises = answer.claims.clone();
        premises.extend(
            earlier
                .iter()
                .filter(|earlier| self.depends_on.contains(&earlier.question_id))
                .flat_map(|earlier| earlier.claims.iter().cloned()),
        );
        // The plan has no subjects; a step may touch anything shown.
        let subject_ids = if self.kind == QuestionKind::RemediationPlan {
            known.iter().cloned().collect()
        } else {
            self.subjects
                .iter()
                .map(|subject| subject.id.clone())
                .collect::<Vec<_>>()
        };
        let bounds = RecommendationBounds {
            case_id: &self.question_id,
            subject_ids: &subject_ids,
            finding_ids: &self.finding_ids,
            families: &self.recommendation_families,
        };
        validate_recommendations(&bounds, &known, &premises, &answer.recommendations)?;
        let errors = self.shape_errors(answer);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn shape_errors(&self, answer: &QuestionAnswer) -> Vec<String> {
        let mut errors = Vec::new();
        match self.kind {
            QuestionKind::HotspotAssessment => {
                errors.extend(self.uncited_subjects(&answer.claims));
                errors.extend(ranking_error(
                    &answer.ranking,
                    self.subjects.iter().map(|subject| subject.id.as_str()),
                    "subject",
                ));
            }
            QuestionKind::SharedRootCause => {
                errors.extend(self.root_cause_errors(answer));
                errors.extend(ranking_error(&answer.ranking, [], "item"));
            }
            QuestionKind::ArchitectureReview => {
                errors.extend(self.uncited_subjects(&answer.claims));
                if !answer
                    .claims
                    .iter()
                    .any(|claim| claim.slot == ClaimSlot::IntendedStructure)
                {
                    errors.push("the answer states no intended structure".to_string());
                }
                errors.extend(ranking_error(&answer.ranking, [], "item"));
            }
            QuestionKind::RemediationPlan => {
                errors.extend(
                    answer
                        .recommendations
                        .iter()
                        .filter(|step| step.affected_finding_ids.is_empty())
                        .map(|step| {
                            format!(
                                "plan step {} resolves no finding: list the finding IDs it fixes in resolves, or drop the step",
                                step.recommendation_id.as_str()
                            )
                        }),
                );
                errors.extend(ranking_error(
                    &answer.ranking,
                    answer
                        .recommendations
                        .iter()
                        .map(|step| step.recommendation_id.as_str()),
                    "recommendation",
                ));
            }
        }
        errors
    }

    /// Subjects no claim cites, by the subject's ID or any evidence its
    /// row shows.
    fn uncited_subjects(&self, claims: &[EngineeringClaim]) -> Vec<String> {
        let cited = claims
            .iter()
            .flat_map(|claim| {
                claim
                    .supporting_evidence_ids
                    .iter()
                    .chain(&claim.counter_evidence_ids)
            })
            .collect::<BTreeSet<_>>();
        self.subjects
            .iter()
            .filter(|subject| !subject.evidence_ids.iter().any(|id| cited.contains(id)))
            .map(|subject| format!("no claim cites subject {}", subject.id))
            .collect()
    }

    /// A root cause that stands must explain two or more of the findings,
    /// and the answer proposes at most one shared fix.
    fn root_cause_errors(&self, answer: &QuestionAnswer) -> Vec<String> {
        let mut errors = Vec::new();
        for claim in answer.claims.iter().filter(|claim| {
            claim.kind == EngineeringClaimKind::RootCause
                && matches!(
                    claim.status,
                    ClaimStatus::Substantiated | ClaimStatus::PartiallySubstantiated
                )
        }) {
            let explained = claim
                .supporting_evidence_ids
                .iter()
                .filter(|id| self.finding_ids.contains(id))
                .count();
            if explained < 2 {
                errors.push(format!(
                    "root cause \"{}\" cites {explained} of the findings; a shared cause explains at least two",
                    claim.proposition
                ));
            }
        }
        let fixes = answer
            .recommendations
            .iter()
            .filter(|step| step.family == RecommendationFamily::SharedRootRemediation)
            .count();
        if fixes > 1 {
            errors.push(format!(
                "{fixes} shared_root_remediation recommendations; one shared cause gets one fix"
            ));
        }
        errors
    }
}

/// The ranking must list each expected ID exactly once and nothing else.
fn ranking_error<'a>(
    ranking: &[String],
    expected: impl IntoIterator<Item = &'a str>,
    noun: &str,
) -> Option<String> {
    let expected = expected.into_iter().collect::<BTreeSet<_>>();
    let ranked = ranking.iter().map(String::as_str).collect::<Vec<_>>();
    let distinct = ranked.iter().copied().collect::<BTreeSet<_>>();
    if distinct.len() == ranked.len() && distinct == expected {
        return None;
    }
    if expected.is_empty() {
        return Some("this question takes no ranking; send an empty ranking".to_string());
    }
    let listed = |label: String, ids: Vec<&str>| {
        (!ids.is_empty()).then(|| format!("{label} {}", ids.join(", ")))
    };
    let repeated = ranked
        .iter()
        .enumerate()
        .filter(|(index, id)| ranked[..*index].contains(id))
        .map(|(_, id)| *id)
        .collect::<BTreeSet<_>>();
    let parts = [
        listed(
            "missing".to_string(),
            expected.difference(&distinct).copied().collect(),
        ),
        listed(
            format!("not a {noun} ID"),
            distinct.difference(&expected).copied().collect(),
        ),
        listed("repeated".to_string(), repeated.into_iter().collect()),
    ];
    Some(format!(
        "the ranking must list every {noun} ID once and nothing else: {}",
        parts.into_iter().flatten().collect::<Vec<_>>().join("; ")
    ))
}

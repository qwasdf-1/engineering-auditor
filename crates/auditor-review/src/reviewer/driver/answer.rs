//! The `answer_question` tool: the reviewer sends one whole answer, naming
//! its claims and recommendations with refs of its own choosing; the host
//! turns refs into content-derived IDs and holds the answer to the
//! question with [`Question::check_answer`].

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::document::{BoundedDocument, Item, Row, Section};
use super::super::questions::QuestionKind;
use crate::{
    BenefitEvidenceMode, ClaimConfidence, ClaimDraft, ClaimSlot, ClaimStatus,
    ContextualRecommendation, DetectorAdviceRelationship, EngineeringClaim, EngineeringClaimKind,
    ExpectedBenefit, Materiality, Question, QuestionAnswer, RecommendationAlternative,
    RecommendationDraft, RecommendationFamily, StableId, ToolSpec, VerificationStep,
};

pub(super) const ANSWER_TOOL: &str = "answer_question";
/// Bytes the earlier answers shown to the plan may take.
const EARLIER_ANSWERS_BYTES: usize = 24 * 1024;

const CLAIM_KINDS: [EngineeringClaimKind; 4] = [
    EngineeringClaimKind::RootCause,
    EngineeringClaimKind::Actionability,
    EngineeringClaimKind::RecommendationPremise,
    EngineeringClaimKind::ExpectedBenefit,
];
const CLAIM_STATUSES: [ClaimStatus; 5] = [
    ClaimStatus::Substantiated,
    ClaimStatus::PartiallySubstantiated,
    ClaimStatus::NotSubstantiated,
    ClaimStatus::Blocked,
    ClaimStatus::Contested,
];
const CONFIDENCES: [ClaimConfidence; 3] = [
    ClaimConfidence::Low,
    ClaimConfidence::Medium,
    ClaimConfidence::High,
];
const RELATIONSHIPS: [DetectorAdviceRelationship; 6] = [
    DetectorAdviceRelationship::Preserves,
    DetectorAdviceRelationship::Consolidates,
    DetectorAdviceRelationship::Narrows,
    DetectorAdviceRelationship::Sequences,
    DetectorAdviceRelationship::Defers,
    DetectorAdviceRelationship::NotActionableWithCurrentEvidence,
];
const BENEFIT_MODES: [BenefitEvidenceMode; 5] = [
    BenefitEvidenceMode::DirectMeasurement,
    BenefitEvidenceMode::BoundedExtrapolation,
    BenefitEvidenceMode::StructuralInference,
    BenefitEvidenceMode::QualitativeOnly,
    BenefitEvidenceMode::Unknown,
];
const MATERIALITIES: [Materiality; 3] = [Materiality::Low, Materiality::Medium, Materiality::High];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerArguments {
    claims: Vec<ClaimArgument>,
    #[serde(default)]
    recommendations: Vec<RecommendationArgument>,
    #[serde(default)]
    ranking: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimArgument {
    #[serde(rename = "ref")]
    reference: String,
    slot: ClaimSlot,
    kind: EngineeringClaimKind,
    proposition: String,
    status: ClaimStatus,
    supporting_evidence_ids: Vec<String>,
    #[serde(default)]
    counter_evidence_ids: Vec<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    assumptions: Vec<String>,
    confidence: ClaimConfidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecommendationArgument {
    #[serde(rename = "ref")]
    reference: String,
    family: RecommendationFamily,
    summary: String,
    scope_ids: Vec<String>,
    premises: Vec<String>,
    resolves: Vec<String>,
    detector_advice_relationship: DetectorAdviceRelationship,
    #[serde(default)]
    preconditions: Vec<String>,
    #[serde(default)]
    expected_benefits: Vec<ExpectedBenefit>,
    #[serde(default)]
    risks: Vec<String>,
    #[serde(default)]
    costs: Vec<String>,
    #[serde(default)]
    alternatives: Vec<RecommendationAlternative>,
    no_alternative_reason: Option<String>,
    verification_steps: Vec<VerificationStep>,
    materiality: Materiality,
    materiality_basis: String,
    confidence: ClaimConfidence,
    patch: Option<String>,
}

/// An accepted answer and the IDs its refs became.
pub(super) struct Accepted {
    pub(super) answer: QuestionAnswer,
    pub(super) claim_ids: Vec<(String, StableId)>,
    pub(super) recommendation_ids: Vec<(String, StableId)>,
}

/// The tool, with this question's slots and families as the only choices.
pub(super) fn answer_tool(question: &Question) -> ToolSpec {
    let strings = |description: &str| json!({"type": "array", "items": {"type": "string"}, "description": description});
    let text = |description: &str| json!({"type": "string", "description": description});
    let choice = |values: Vec<String>| json!({"type": "string", "enum": values});
    let described = |values: Vec<String>, description: &str| json!({"type": "string", "enum": values, "description": description});
    let object = |properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
            "required": required,
        })
    };
    let claim = object(
        json!({
            "ref": text("A name for this claim, unique in the answer, such as c1."),
            "slot": described(names(&question.claim_slots), "What the claim is about: one of this question's claim slots."),
            "kind": described(names(&CLAIM_KINDS), "The claim's role, separate from its slot: root_cause names a cause behind findings, actionability says whether something can be acted on, recommendation_premise is a fact a recommendation rests on, expected_benefit is what a change gains."),
            "proposition": text("What the claim asserts, in one or two sentences."),
            "status": choice(names(&CLAIM_STATUSES)),
            "supporting_evidence_ids": strings("Evidence IDs shown to you in brackets (finding:, file:, symbol:, source-span:, ...) that support the claim; never paths or claim IDs."),
            "counter_evidence_ids": strings("Evidence IDs shown to you that cut against it."),
            "depends_on": strings("Refs of other claims in this answer it rests on."),
            "assumptions": strings("What the claim assumes that no evidence shows."),
            "confidence": choice(names(&CONFIDENCES)),
        }),
        &[
            "ref",
            "slot",
            "kind",
            "proposition",
            "status",
            "supporting_evidence_ids",
            "confidence",
        ],
    );
    let benefit = object(
        json!({
            "description": text("The benefit."),
            "evidence_mode": choice(names(&BENEFIT_MODES)),
            "numeric_value": text("A decimal number, only with a measurement or a bounded extrapolation."),
            "unit": text("The number's unit."),
            "extrapolation_model": text("How a bounded extrapolation was made."),
            "evidence_ids": strings("Evidence IDs behind the benefit."),
        }),
        &["description", "evidence_mode", "evidence_ids"],
    );
    let alternative = object(
        json!({"summary": text("Another way to act."), "tradeoffs": strings("Its tradeoffs.")}),
        &["summary", "tradeoffs"],
    );
    let step = object(
        json!({
            "method": text("How to verify the change."),
            "expected_observation": text("What verification should show."),
            "evidence_ids": strings("Evidence IDs the step reads."),
        }),
        &["method", "expected_observation", "evidence_ids"],
    );
    let recommendation = object(
        json!({
            "ref": text("A name for this recommendation, unique in the answer, such as r1."),
            "family": choice(names(&question.recommendation_families)),
            "summary": text("What to do."),
            "scope_ids": strings(scope_description(question.kind)),
            "premises": strings("Refs of claims in this answer, or claim IDs from earlier answers, that justify it."),
            "resolves": strings("Finding IDs (finding:...) of this question the change resolves."),
            "detector_advice_relationship": choice(names(&RELATIONSHIPS)),
            "preconditions": strings("What must hold first."),
            "expected_benefits": {"type": "array", "items": benefit},
            "risks": strings("Risks of the change."),
            "costs": strings("Costs of the change."),
            "alternatives": {"type": "array", "items": alternative},
            "no_alternative_reason": text("Why there is no alternative, when none is given."),
            "verification_steps": {"type": "array", "items": step, "minItems": 1},
            "materiality": choice(names(&MATERIALITIES)),
            "materiality_basis": text("Why it matters this much."),
            "confidence": choice(names(&CONFIDENCES)),
            "patch": text("Optional unified diff that carries the change out; EA applies it to a scratch copy and re-scans."),
        }),
        &[
            "ref",
            "family",
            "summary",
            "scope_ids",
            "premises",
            "resolves",
            "detector_advice_relationship",
            "verification_steps",
            "materiality",
            "materiality_basis",
            "confidence",
        ],
    );
    ToolSpec {
        name: ANSWER_TOOL,
        description: "Answer the question with typed claims and recommendations. Send the whole answer in one call; a rejected answer comes back with the reasons, and a corrected answer replaces it.",
        parameters: object(
            json!({
                "claims": {"type": "array", "items": claim},
                "recommendations": {"type": "array", "items": recommendation},
                "ranking": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": ranking_description(question.kind),
                },
            }),
            &["claims"],
        ),
        strict: false,
    }
}

fn scope_description(kind: QuestionKind) -> &'static str {
    match kind {
        QuestionKind::RemediationPlan => {
            "Evidence IDs shown to you that the change touches (finding:, file:, symbol:, module:, module-cycle:, ...), never paths."
        }
        _ => "IDs from this question's Subjects or Findings lists that the change touches, exactly as listed (file:..., symbol:..., finding:...), never paths.",
    }
}

fn ranking_description(kind: QuestionKind) -> &'static str {
    match kind {
        QuestionKind::HotspotAssessment => {
            "Every subject ID of the question once, most risky first."
        }
        QuestionKind::RemediationPlan => {
            "Every recommendation ref once, in the order to carry them out."
        }
        QuestionKind::SharedRootCause | QuestionKind::ArchitectureReview => "Leave empty.",
    }
}

/// Serialized enum names.
fn names<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| {
            serde_json::to_value(value)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
        })
        .collect()
}

/// Builds the answer from the tool's arguments and checks it; the error
/// lists everything to fix.
pub(super) fn accept(
    question: &Question,
    arguments: &Value,
    known_evidence_ids: &BTreeSet<String>,
    earlier: &[QuestionAnswer],
) -> Result<Accepted, String> {
    let arguments: AnswerArguments = serde_json::from_value(arguments.clone())
        .map_err(|error| format!("invalid arguments: {error}"))?;
    let built_claims = claims(question, &arguments.claims)?;
    let claim_ids = built_claims
        .iter()
        .map(|(reference, claim)| (reference.clone(), claim.claim_id.clone()))
        .collect::<Vec<_>>();
    let earlier_claims = earlier
        .iter()
        .filter(|answer| question.depends_on.contains(&answer.question_id))
        .flat_map(|answer| answer.claims.iter().map(|claim| claim.claim_id.clone()))
        .collect::<BTreeSet<_>>();
    let mut recommendation_ids = Vec::new();
    let mut recommendations = Vec::new();
    let mut seen = BTreeSet::new();
    for argument in arguments.recommendations {
        if argument.reference.trim().is_empty() || !seen.insert(argument.reference.clone()) {
            return Err(format!(
                "recommendation ref `{}` is empty or used twice",
                argument.reference
            ));
        }
        let premise_claim_ids = argument
            .premises
            .iter()
            .map(|premise| {
                claim_ids
                    .iter()
                    .find(|(reference, _)| reference == premise)
                    .map(|(_, id)| id.clone())
                    .or_else(|| {
                        earlier_claims
                            .iter()
                            .find(|id| id.as_str() == premise)
                            .cloned()
                    })
                    .ok_or_else(|| {
                        format!(
                            "recommendation `{}` premise `{premise}` is neither a claim ref of this answer nor a claim ID of an earlier answer",
                            argument.reference
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let reference = argument.reference.clone();
        let recommendation = ContextualRecommendation::from_draft(
            question.question_id.clone(),
            RecommendationDraft {
                family: argument.family,
                summary: argument.summary,
                scope_ids: argument.scope_ids,
                premise_claim_ids,
                affected_finding_ids: argument.resolves,
                detector_advice_relationship: argument.detector_advice_relationship,
                preconditions: argument.preconditions,
                expected_benefits: argument.expected_benefits,
                risks: argument.risks,
                costs: argument.costs,
                alternatives: argument.alternatives,
                no_alternative_reason: argument.no_alternative_reason,
                verification_steps: argument.verification_steps,
                materiality: argument.materiality,
                materiality_basis: argument.materiality_basis,
                confidence: argument.confidence,
                patch: argument.patch.filter(|patch| !patch.trim().is_empty()),
            },
        )
        .map_err(|error| format!("recommendation `{reference}`: {error}"))?;
        recommendation_ids.push((reference, recommendation.recommendation_id.clone()));
        recommendations.push(recommendation);
    }
    let ranking = arguments
        .ranking
        .into_iter()
        .map(|entry| {
            recommendation_ids
                .iter()
                .find(|(reference, _)| *reference == entry)
                .map_or(entry, |(_, id)| id.as_str().to_string())
        })
        .collect();
    let answer = QuestionAnswer {
        question_id: question.question_id.clone(),
        claims: built_claims.into_iter().map(|(_, claim)| claim).collect(),
        recommendations,
        ranking,
    };
    question
        .check_answer(&answer, known_evidence_ids, earlier)
        .map_err(|error| with_refs(error, &claim_ids, &recommendation_ids))?;
    Ok(Accepted {
        answer,
        claim_ids,
        recommendation_ids,
    })
}

/// The claims by ref, in the order the refs were given. A claim's ID covers
/// the IDs of the claims it depends on, so dependencies are built first.
fn claims(
    question: &Question,
    arguments: &[ClaimArgument],
) -> Result<Vec<(String, EngineeringClaim)>, String> {
    let refs = arguments
        .iter()
        .map(|claim| claim.reference.as_str())
        .collect::<Vec<_>>();
    if refs.iter().any(|reference| reference.trim().is_empty())
        || refs.iter().collect::<BTreeSet<_>>().len() != refs.len()
    {
        return Err("every claim needs a ref of its own".to_string());
    }
    for claim in arguments {
        if let Some(missing) = claim
            .depends_on
            .iter()
            .find(|dependency| !refs.contains(&dependency.as_str()))
        {
            return Err(format!(
                "claim `{}` depends on `{missing}`, which is not a claim ref of this answer",
                claim.reference
            ));
        }
    }
    let mut ids = BTreeMap::<&str, StableId>::new();
    let mut built = BTreeMap::<&str, EngineeringClaim>::new();
    while built.len() < arguments.len() {
        let ready = arguments.iter().find(|claim| {
            !built.contains_key(claim.reference.as_str())
                && claim
                    .depends_on
                    .iter()
                    .all(|dependency| ids.contains_key(dependency.as_str()))
        });
        let Some(claim) = ready else {
            return Err("the claims' depends_on refs form a cycle".to_string());
        };
        let record = EngineeringClaim::from_draft(question.question_id.clone(), draft(claim, &ids))
            .map_err(|error| format!("claim `{}`: {error}", claim.reference))?;
        ids.insert(&claim.reference, record.claim_id.clone());
        built.insert(&claim.reference, record);
    }
    Ok(refs
        .into_iter()
        .map(|reference| {
            let claim = built.remove(reference).expect("every ref was built");
            (reference.to_string(), claim)
        })
        .collect())
}

fn draft(claim: &ClaimArgument, ids: &BTreeMap<&str, StableId>) -> ClaimDraft {
    ClaimDraft {
        slot: claim.slot,
        kind: claim.kind,
        proposition: claim.proposition.clone(),
        status: claim.status,
        supporting_evidence_ids: claim.supporting_evidence_ids.clone(),
        counter_evidence_ids: claim.counter_evidence_ids.clone(),
        dependency_claim_ids: claim
            .depends_on
            .iter()
            .map(|dependency| ids[dependency.as_str()].clone())
            .collect(),
        assumptions: claim.assumptions.clone(),
        confidence: claim.confidence,
    }
}

/// The checker's reasons name host IDs; say which ref each one is.
fn with_refs(
    error: String,
    claim_ids: &[(String, StableId)],
    recommendation_ids: &[(String, StableId)],
) -> String {
    let named = claim_ids
        .iter()
        .chain(recommendation_ids)
        .filter(|(_, id)| error.contains(id.as_str()))
        .map(|(reference, id)| format!("`{reference}` is {}", id.as_str()))
        .collect::<Vec<_>>();
    let mut error = if named.is_empty() {
        error
    } else {
        format!("{error} ({})", named.join("; "))
    };
    // Unknown evidence is listed quoted; a claim's own ID is not.
    if error.contains("cites unknown evidence") {
        for (prefix, hint) in UNKNOWN_EVIDENCE_HINTS {
            if error.contains(prefix) {
                error.push_str(hint);
            }
        }
    }
    error
}

/// What to do about an unknown cited ID of each kind.
const UNKNOWN_EVIDENCE_HINTS: [(&str, &str); 2] = [
    (
        "\"engineering-claim:",
        "; a claim ID from an earlier answer is a premise for a recommendation, not evidence for a claim: cite the evidence that claim cites",
    ),
    (
        "\"source-span:",
        "; a source-span ID names exactly the lines a read returned: cite the ID the read issued, or read those lines first",
    ),
];

/// The tool result for an accepted answer.
pub(super) fn accepted_markdown(accepted: &Accepted) -> String {
    let mut markdown = String::from("# Answer accepted\n\n");
    for (reference, id) in accepted
        .claim_ids
        .iter()
        .chain(&accepted.recommendation_ids)
    {
        markdown.push_str(&format!("- `{reference}`: {}\n", id.as_str()));
    }
    markdown
}

/// The answers the plan builds on: each claim with its ID, so a plan step
/// can take it as a premise, and each recommendation with the findings it
/// resolves. Rows cite the evidence the answers cited.
pub(in crate::reviewer) fn earlier_answers(
    answers: &[(QuestionKind, &QuestionAnswer)],
    source_digest: &str,
) -> Result<super::super::document::Rendered, String> {
    let sections = answers
        .iter()
        .map(|(kind, answer)| {
            let claims = answer.claims.iter().map(|claim| {
                Row::new(
                    format!(
                        "claim {} ({}, {}): {}",
                        claim.claim_id.as_str(),
                        wire_name(&claim.slot),
                        wire_name(&claim.status),
                        claim.proposition
                    ),
                    claim.supporting_evidence_ids.clone(),
                )
            });
            let recommendations = answer.recommendations.iter().map(|step| {
                Row::new(
                    format!(
                        "recommendation {} ({}): {}",
                        step.recommendation_id.as_str(),
                        wire_name(&step.family),
                        step.summary
                    ),
                    step.affected_finding_ids.clone(),
                )
            });
            Section::new(
                format!("{kind:?} {}", answer.question_id.as_str()),
                vec![
                    Item::rows("Claims", claims, usize::MAX),
                    Item::rows("Recommendations", recommendations, usize::MAX),
                ],
            )
        })
        .collect();
    BoundedDocument::new("Earlier answers", sections).render(EARLIER_ANSWERS_BYTES, source_digest)
}

/// A serde enum's wire name.
pub(in crate::reviewer) fn wire_name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_choice_is_a_variant_name() {
        assert_eq!(names(&CLAIM_KINDS).len(), CLAIM_KINDS.len());
        assert_eq!(names(&CLAIM_STATUSES).len(), CLAIM_STATUSES.len());
        assert_eq!(names(&CONFIDENCES).len(), CONFIDENCES.len());
        assert_eq!(names(&RELATIONSHIPS).len(), RELATIONSHIPS.len());
        assert_eq!(names(&BENEFIT_MODES).len(), BENEFIT_MODES.len());
        assert_eq!(names(&MATERIALITIES).len(), MATERIALITIES.len());
    }
}

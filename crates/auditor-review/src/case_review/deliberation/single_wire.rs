use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::single::{
    CasePosition, ReviewerAdmissionContext, ReviewerCaseReviewPayload, SingleReviewerError,
};
use crate::case_review::probe::admissible_evidence_ids;
use crate::{
    BenefitEvidenceMode, CaseClosureReason, CaseConclusion, ClaimConfidence, ClaimDraft, ClaimSlot,
    ClaimStatus, ContextualRecommendation, DetectorAdviceRelationship, EngineeringCase,
    EngineeringClaim, EngineeringClaimKind, ExpectedBenefit, Materiality, ProbeAnswer,
    ProbeResolution, ProbeResult, RecommendationAlternative, RecommendationDraft,
    RecommendationFamily, ResolverKind, ResolverRecord, StableId, VerificationStep,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelProbeResultDraft {
    probe_id: StableId,
    answer: ProbeAnswer,
    evidence_ids: Vec<String>,
    explanation: String,
    limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelClaimDraft {
    claim_ref: String,
    slot: ClaimSlot,
    kind: EngineeringClaimKind,
    proposition: String,
    status: ClaimStatus,
    supporting_probe_ids: Vec<StableId>,
    counter_probe_ids: Vec<StableId>,
    dependency_claim_refs: Vec<String>,
    assumptions: Vec<String>,
    confidence: ClaimConfidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelRecommendationDraft {
    recommendation_ref: String,
    family: RecommendationFamily,
    summary: String,
    scope_ids: Vec<String>,
    premise_claim_refs: Vec<String>,
    affected_finding_ids: Vec<String>,
    detector_advice_relationship: DetectorAdviceRelationship,
    preconditions: Vec<String>,
    expected_benefits: Vec<WireExpectedBenefit>,
    risks: Vec<String>,
    costs: Vec<String>,
    alternatives: Vec<RecommendationAlternative>,
    no_alternative_reason: Option<String>,
    verification_steps: Vec<VerificationStep>,
    materiality: Materiality,
    materiality_basis: String,
    confidence: ClaimConfidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "evidence_mode")]
pub(super) enum WireExpectedBenefit {
    DirectMeasurement {
        description: String,
        numeric_value: serde_json::Number,
        unit: String,
        evidence_probe_ids: Vec<StableId>,
    },
    BoundedExtrapolation {
        description: String,
        numeric_value: serde_json::Number,
        unit: String,
        extrapolation_model: String,
        evidence_probe_ids: Vec<StableId>,
    },
    StructuralInference {
        description: String,
        evidence_probe_ids: Vec<StableId>,
    },
    QualitativeOnly {
        description: String,
        evidence_probe_ids: Vec<StableId>,
    },
    Unknown {
        description: String,
        evidence_probe_ids: Vec<StableId>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewerCaseReviewWirePayload {
    case_conclusion: CaseConclusion,
    case_position: CasePosition,
    model_resolved_probes: Vec<ModelProbeResultDraft>,
    claims: Vec<ModelClaimDraft>,
    recommendations: Vec<ModelRecommendationDraft>,
    preferred_recommendation_ref: Option<String>,
    unresolved_gaps: Vec<String>,
    proposed_closure: CaseClosureReason,
}

pub(super) struct WireAdmission {
    pub case_conclusion: CaseConclusion,
    pub case_position: CasePosition,
    pub model_probe_admission: Result<Vec<ProbeResult>, SingleReviewerError>,
    pub probe_answers: BTreeMap<StableId, ProbeAnswer>,
    pub claims: Vec<EngineeringClaim>,
    pub recommendations: Vec<ContextualRecommendation>,
    pub preferred_recommendation_id: Option<StableId>,
    pub unresolved_gaps: Vec<String>,
    pub proposed_closure: CaseClosureReason,
}

impl WireAdmission {
    pub(super) fn new(
        payload: ReviewerCaseReviewWirePayload,
        context: &ReviewerAdmissionContext<'_>,
    ) -> Result<Self, SingleReviewerError> {
        let ReviewerCaseReviewWirePayload {
            case_conclusion,
            case_position,
            model_resolved_probes,
            claims,
            recommendations,
            preferred_recommendation_ref,
            unresolved_gaps,
            proposed_closure,
        } = payload;
        let probe_answers = wire_probe_answers(&model_resolved_probes, context);
        let model_probe_admission = admit_model_probe_results(model_resolved_probes, context);
        let mut claim_ids = LocalIdentityIndex::new(
            SingleReviewerError::InvalidClaims,
            "claim local reference is invalid or duplicated",
        );
        let claims = admit_sequence(
            claims,
            &mut claim_ids,
            |draft| draft.claim_ref.clone(),
            |draft, known| claim_from_wire(draft, context.case, known),
            |claim| claim.claim_id.clone(),
        )?;
        let mut recommendation_ids = LocalIdentityIndex::new(
            SingleReviewerError::InvalidRecommendations,
            "recommendation local reference is invalid or duplicated",
        );
        let recommendations = admit_sequence(
            recommendations,
            &mut recommendation_ids,
            |draft| draft.recommendation_ref.clone(),
            |draft, _| recommendation_from_wire(draft, context.case, &claim_ids),
            |recommendation| recommendation.recommendation_id.clone(),
        )?;
        let preferred_recommendation_id = preferred_recommendation_ref
            .map(|reference| {
                recommendation_ids
                    .get(&reference)
                    .cloned()
                    .ok_or(SingleReviewerError::InvalidPreferredRecommendation)
            })
            .transpose()?;
        Ok(Self {
            case_conclusion,
            case_position,
            model_probe_admission,
            probe_answers,
            claims,
            recommendations,
            preferred_recommendation_id,
            unresolved_gaps,
            proposed_closure,
        })
    }

    pub(super) fn into_payload(self) -> ReviewerCaseReviewPayload {
        ReviewerCaseReviewPayload {
            case_conclusion: self.case_conclusion,
            case_position: self.case_position,
            model_resolved_probes: self
                .model_probe_admission
                .expect("validated wire admission has model probe results"),
            claims: self.claims,
            recommendations: self.recommendations,
            preferred_recommendation_id: self.preferred_recommendation_id,
            unresolved_gaps: self.unresolved_gaps,
            proposed_closure: self.proposed_closure,
        }
    }
}

fn wire_probe_answers(
    drafts: &[ModelProbeResultDraft],
    context: &ReviewerAdmissionContext<'_>,
) -> BTreeMap<StableId, ProbeAnswer> {
    context
        .deterministic_probe_results
        .iter()
        .map(|result| (result.probe_id.clone(), result.answer))
        .chain(
            drafts
                .iter()
                .map(|draft| (draft.probe_id.clone(), draft.answer)),
        )
        .collect()
}

fn admit_model_probe_results(
    drafts: Vec<ModelProbeResultDraft>,
    context: &ReviewerAdmissionContext<'_>,
) -> Result<Vec<ProbeResult>, SingleReviewerError> {
    let mut results = Vec::with_capacity(drafts.len());
    let mut errors = Vec::new();
    for draft in drafts {
        match model_probe_result(draft, context) {
            Ok(result) => results.push(result),
            Err(SingleReviewerError::InvalidModelProbe(detail)) => errors.push(detail),
            Err(error) => return Err(error),
        }
    }
    if errors.is_empty() {
        Ok(results)
    } else {
        errors.sort();
        errors.dedup();
        Err(SingleReviewerError::InvalidModelProbe(errors.join("; ")))
    }
}

fn model_probe_result(
    draft: ModelProbeResultDraft,
    context: &ReviewerAdmissionContext<'_>,
) -> Result<ProbeResult, SingleReviewerError> {
    let definition = context
        .probe_definitions
        .iter()
        .find(|definition| definition.probe_id == draft.probe_id)
        .ok_or_else(|| {
            SingleReviewerError::InvalidModelProbe(format!(
                "{} is not a declared case probe",
                draft.probe_id
            ))
        })?;
    let probe_id = draft.probe_id.clone();
    let admissible_evidence_ids = admissible_evidence_ids(definition, context.packet);
    ProbeResult::from_resolution(
        definition,
        ResolverRecord {
            kind: ResolverKind::ModelInterpretation,
            id: "case-reviewer-model-interpretation".to_string(),
            version: 1,
        },
        ProbeResolution::model(
            draft.answer,
            draft.evidence_ids,
            draft.explanation,
            draft.limitations,
            context.reviewer_run_id.clone(),
        ),
        context.packet,
    )
    .map_err(|error| {
        SingleReviewerError::InvalidModelProbe(format!(
            "{probe_id}: {error}; admissible evidence IDs are {admissible_evidence_ids:?}"
        ))
    })
}

fn claim_from_wire(
    draft: ModelClaimDraft,
    case: &EngineeringCase,
    known: &LocalIdentityIndex,
) -> Result<EngineeringClaim, SingleReviewerError> {
    let dependencies = known.resolve(
        &draft.dependency_claim_refs,
        SingleReviewerError::InvalidClaims,
        "claim dependency must reference an earlier local claim",
    )?;
    EngineeringClaim::from_draft(
        case.case_id.clone(),
        ClaimDraft {
            slot: draft.slot,
            kind: draft.kind,
            proposition: draft.proposition,
            status: draft.status,
            supporting_probe_ids: draft.supporting_probe_ids,
            counter_probe_ids: draft.counter_probe_ids,
            dependency_claim_ids: dependencies,
            assumptions: draft.assumptions,
            confidence: draft.confidence,
        },
    )
    .map_err(SingleReviewerError::InvalidClaims)
}

fn recommendation_from_wire(
    draft: ModelRecommendationDraft,
    case: &EngineeringCase,
    claims: &LocalIdentityIndex,
) -> Result<ContextualRecommendation, SingleReviewerError> {
    let premise_claim_ids = claims.resolve(
        &draft.premise_claim_refs,
        SingleReviewerError::InvalidRecommendations,
        "recommendation premise names an unknown local claim",
    )?;
    ContextualRecommendation::from_draft(
        case.case_id.clone(),
        RecommendationDraft {
            family: draft.family,
            summary: draft.summary,
            scope_ids: draft.scope_ids,
            premise_claim_ids,
            affected_finding_ids: draft.affected_finding_ids,
            detector_advice_relationship: draft.detector_advice_relationship,
            preconditions: draft.preconditions,
            expected_benefits: draft
                .expected_benefits
                .into_iter()
                .map(WireExpectedBenefit::into_expected_benefit)
                .collect(),
            risks: draft.risks,
            costs: draft.costs,
            alternatives: draft.alternatives,
            no_alternative_reason: draft.no_alternative_reason,
            verification_steps: draft.verification_steps,
            materiality: draft.materiality,
            materiality_basis: draft.materiality_basis,
            confidence: draft.confidence,
        },
    )
    .map_err(SingleReviewerError::InvalidRecommendations)
}

impl WireExpectedBenefit {
    pub(super) fn into_expected_benefit(self) -> ExpectedBenefit {
        match self {
            Self::DirectMeasurement {
                description,
                numeric_value,
                unit,
                evidence_probe_ids,
            } => ExpectedBenefit {
                description,
                evidence_mode: BenefitEvidenceMode::DirectMeasurement,
                numeric_value: Some(numeric_value.to_string()),
                unit: Some(unit),
                extrapolation_model: None,
                evidence_probe_ids,
            },
            Self::BoundedExtrapolation {
                description,
                numeric_value,
                unit,
                extrapolation_model,
                evidence_probe_ids,
            } => ExpectedBenefit {
                description,
                evidence_mode: BenefitEvidenceMode::BoundedExtrapolation,
                numeric_value: Some(numeric_value.to_string()),
                unit: Some(unit),
                extrapolation_model: Some(extrapolation_model),
                evidence_probe_ids,
            },
            Self::StructuralInference {
                description,
                evidence_probe_ids,
            } => qualitative_benefit(
                description,
                BenefitEvidenceMode::StructuralInference,
                evidence_probe_ids,
            ),
            Self::QualitativeOnly {
                description,
                evidence_probe_ids,
            } => qualitative_benefit(
                description,
                BenefitEvidenceMode::QualitativeOnly,
                evidence_probe_ids,
            ),
            Self::Unknown {
                description,
                evidence_probe_ids,
            } => qualitative_benefit(
                description,
                BenefitEvidenceMode::Unknown,
                evidence_probe_ids,
            ),
        }
    }
}

fn qualitative_benefit(
    description: String,
    evidence_mode: BenefitEvidenceMode,
    evidence_probe_ids: Vec<StableId>,
) -> ExpectedBenefit {
    ExpectedBenefit {
        description,
        evidence_mode,
        numeric_value: None,
        unit: None,
        extrapolation_model: None,
        evidence_probe_ids,
    }
}

fn admit_sequence<Draft, Value>(
    drafts: Vec<Draft>,
    identities: &mut LocalIdentityIndex,
    local_ref: impl Fn(&Draft) -> String,
    mut build: impl FnMut(Draft, &LocalIdentityIndex) -> Result<Value, SingleReviewerError>,
    stable_id: impl Fn(&Value) -> StableId,
) -> Result<Vec<Value>, SingleReviewerError> {
    let mut values = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let reference = local_ref(&draft);
        identities.ensure_available(&reference)?;
        let value = build(draft, identities)?;
        identities.insert(reference, stable_id(&value));
        values.push(value);
    }
    Ok(values)
}

struct LocalIdentityIndex {
    identities: BTreeMap<String, StableId>,
    invalid: fn(String) -> SingleReviewerError,
    duplicate_message: &'static str,
}

impl LocalIdentityIndex {
    fn new(invalid: fn(String) -> SingleReviewerError, duplicate_message: &'static str) -> Self {
        Self {
            identities: BTreeMap::new(),
            invalid,
            duplicate_message,
        }
    }

    fn ensure_available(&self, local_ref: &str) -> Result<(), SingleReviewerError> {
        if !valid_local_ref(local_ref) || self.identities.contains_key(local_ref) {
            return Err((self.invalid)(self.duplicate_message.to_string()));
        }
        Ok(())
    }

    fn insert(&mut self, local_ref: String, stable_id: StableId) {
        self.identities.insert(local_ref, stable_id);
    }

    fn resolve(
        &self,
        references: &[String],
        invalid: fn(String) -> SingleReviewerError,
        message: &str,
    ) -> Result<Vec<StableId>, SingleReviewerError> {
        references
            .iter()
            .map(|reference| {
                self.identities
                    .get(reference)
                    .cloned()
                    .ok_or_else(|| invalid(message.to_string()))
            })
            .collect()
    }

    fn get(&self, reference: &str) -> Option<&StableId> {
        self.identities.get(reference)
    }
}

fn valid_local_ref(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
        && value.len() <= 64
}

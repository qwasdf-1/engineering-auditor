use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    CaseConclusion, CaseInstantiationError, CaseTemplateDefinition, ChallengePolicy, ClaimSlot,
    ClosurePolicy, EvidenceContractTemplate, InferenceGuard, ProbeKind, RecommendationFamily,
    RoutePolicy, TemplateStatus,
};
use crate::case_review::agenda::validate_case_candidate;
use crate::case_review::identity::{canonical_json, CanonicalIdentity};
use crate::{
    CaseKind, ContentDigest, DecisionKind, ReviewCandidate, ReviewInput, RiskPosture, StableId,
};

pub const ENGINEERING_CASE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorCaseConstraints {
    pub constraint_ids: Vec<String>,
    pub risk_posture: Option<RiskPosture>,
    pub excluded_domains: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRef {
    pub probe_id: StableId,
    pub slot: String,
    pub kind: ProbeKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringCase {
    pub schema_version: u32,
    pub case_id: StableId,
    pub case_kind: CaseKind,
    pub template_version: u32,
    pub template_status: TemplateStatus,
    pub template_bundle_digest: ContentDigest,
    pub input_binding_id: StableId,
    pub candidate_id: StableId,
    pub decision_kind: DecisionKind,
    pub subject_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub selection_provenance_refs: Vec<String>,
    pub allowed_conclusions: Vec<CaseConclusion>,
    pub mandatory_probes: Vec<ProbeRef>,
    pub evidence_contract: EvidenceContractTemplate,
    pub operator_constraints: OperatorCaseConstraints,
    pub claim_slots: Vec<ClaimSlot>,
    pub inference_guards: Vec<InferenceGuard>,
    pub recommendation_families: Vec<RecommendationFamily>,
    pub route_policy: RoutePolicy,
    pub challenge_policy: ChallengePolicy,
    pub closure_policy: ClosurePolicy,
}

impl EngineeringCase {
    pub fn derive_id(
        case_kind: CaseKind,
        template_version: u32,
        candidate_id: &StableId,
        input_binding_id: &StableId,
        operator_constraints: &OperatorCaseConstraints,
    ) -> StableId {
        StableId::from_identity(
            "engineering-case",
            ENGINEERING_CASE_SCHEMA_VERSION,
            &case_identity(
                case_kind,
                template_version,
                candidate_id,
                input_binding_id,
                operator_constraints,
            ),
        )
        .expect("engineering-case is a valid typed prefix")
    }

    pub fn from_json(
        text: &str,
        template: &CaseTemplateDefinition,
        candidate: &ReviewCandidate,
        input: &ReviewInput,
    ) -> Result<Self, CaseInstantiationError> {
        let case: Self =
            serde_json::from_str(text).map_err(|error| CaseInstantiationError::InvalidCase {
                detail: error.to_string(),
            })?;
        let expected = build_case(template, TemplateStatus::Production, candidate, input)?;
        if case != expected {
            return Err(CaseInstantiationError::InvalidCase {
                detail: "engineering case does not match bound inputs".to_string(),
            });
        }
        Ok(case)
    }

    pub fn to_canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).expect("engineering case is serializable"))
    }

    pub fn allows_optional_probe(
        &self,
        template: &CaseTemplateDefinition,
        kind: ProbeKind,
    ) -> bool {
        self.case_kind == template.kind && template.allows_optional_probe(kind)
    }
}

impl CanonicalIdentity for EngineeringCase {
    fn identity_fields(&self) -> Value {
        case_identity(
            self.case_kind,
            self.template_version,
            &self.candidate_id,
            &self.input_binding_id,
            &self.operator_constraints,
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateNotEnabledReason {
    TemplateNotEnabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateNotEnabledRecord {
    pub record_id: StableId,
    pub candidate_id: StableId,
    pub case_kind: CaseKind,
    pub template_version: u32,
    pub input_binding_id: StableId,
    pub reason: TemplateNotEnabledReason,
}

impl TemplateNotEnabledRecord {
    pub(super) fn new(
        candidate: &ReviewCandidate,
        kind: CaseKind,
        version: u32,
        input: &ReviewInput,
    ) -> Self {
        let input_binding_id = input.binding().stable_id();
        let identity = json!({
            "candidate_id": candidate.candidate_id,
            "case_kind": kind,
            "input_binding_id": input_binding_id,
            "reason": TemplateNotEnabledReason::TemplateNotEnabled,
            "template_version": version,
        });
        Self {
            record_id: StableId::from_identity(
                "case-instantiation",
                ENGINEERING_CASE_SCHEMA_VERSION,
                &identity,
            )
            .expect("case-instantiation is a valid typed prefix"),
            candidate_id: candidate.candidate_id.clone(),
            case_kind: kind,
            template_version: version,
            input_binding_id,
            reason: TemplateNotEnabledReason::TemplateNotEnabled,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "record", rename_all = "snake_case")]
pub enum CaseInstantiationResult {
    Instantiated(Box<EngineeringCase>),
    TemplateNotEnabled(TemplateNotEnabledRecord),
}

pub(super) fn build_case(
    template: &CaseTemplateDefinition,
    template_status: TemplateStatus,
    candidate: &ReviewCandidate,
    input: &ReviewInput,
) -> Result<EngineeringCase, CaseInstantiationError> {
    validate_case_candidate_binding(template, candidate, input)?;
    let operator_constraints = operator_constraints(input);
    let input_binding_id = input.binding().stable_id();
    let case_id = EngineeringCase::derive_id(
        template.kind,
        template.version,
        &candidate.candidate_id,
        &input_binding_id,
        &operator_constraints,
    );
    let mandatory_probes = template
        .mandatory_probes
        .iter()
        .map(|probe| ProbeRef {
            probe_id: probe_id(&case_id, &probe.slot),
            slot: probe.slot.clone(),
            kind: probe.kind,
        })
        .collect();
    Ok(EngineeringCase {
        schema_version: ENGINEERING_CASE_SCHEMA_VERSION,
        case_id,
        case_kind: template.kind,
        template_version: template.version,
        template_status,
        template_bundle_digest: input.binding().case_template_bundle_digest.clone(),
        input_binding_id,
        candidate_id: candidate.candidate_id.clone(),
        decision_kind: candidate.decision_kind,
        subject_ids: candidate.subject_ids.clone(),
        finding_ids: candidate.finding_ids.clone(),
        selection_provenance_refs: candidate.selection_evidence_refs.clone(),
        allowed_conclusions: template.closure_policy.allowed_conclusions.clone(),
        mandatory_probes,
        evidence_contract: template.evidence_contract.clone(),
        operator_constraints,
        claim_slots: template.claim_slots.clone(),
        inference_guards: template.inference_guards.clone(),
        recommendation_families: template.recommendation_families.clone(),
        route_policy: template.route_policy.clone(),
        challenge_policy: template.challenge_policy.clone(),
        closure_policy: template.closure_policy.clone(),
    })
}

fn validate_case_candidate_binding(
    template: &CaseTemplateDefinition,
    candidate: &ReviewCandidate,
    input: &ReviewInput,
) -> Result<(), CaseInstantiationError> {
    candidate
        .validate()
        .map_err(|detail| CaseInstantiationError::InvalidCandidate { detail })?;
    if candidate.decision_kind != template.trigger.decision_kind
        || !candidate.eligible_case_kinds.contains(&template.kind)
        || !candidate
            .selection_reasons
            .iter()
            .any(|reason| template.trigger.selection_reasons_any.contains(reason))
    {
        return Err(CaseInstantiationError::TriggerMismatch);
    }
    if candidate.finding_ids.is_empty()
        || (template.kind != CaseKind::LifecycleMismatch && candidate.finding_ids.len() < 2)
        || candidate
            .finding_ids
            .iter()
            .any(|finding_id| input.finding(finding_id).is_none())
    {
        return Err(CaseInstantiationError::DanglingReference { kind: "finding" });
    }
    let connected = validate_case_candidate(input, candidate).map_err(|error| {
        CaseInstantiationError::InvalidCandidate {
            detail: error.to_string(),
        }
    })?;
    if candidate.subject_ids.len() < template.subject_policy.minimum_subjects || !connected {
        return Err(CaseInstantiationError::DisconnectedSubjects);
    }
    Ok(())
}

fn operator_constraints(input: &ReviewInput) -> OperatorCaseConstraints {
    let mut constraint_ids = input
        .operator_brief()
        .into_iter()
        .flat_map(|brief| {
            brief
                .constraints
                .iter()
                .map(|constraint| constraint.id.clone())
        })
        .collect::<Vec<_>>();
    let mut excluded_domains = input
        .operator_brief()
        .map(|brief| brief.excluded_domains.clone())
        .unwrap_or_default();
    constraint_ids.sort();
    constraint_ids.dedup();
    excluded_domains.sort();
    excluded_domains.dedup();
    OperatorCaseConstraints {
        constraint_ids,
        risk_posture: input
            .operator_brief()
            .map(|brief| brief.risk_posture.clone()),
        excluded_domains,
    }
}

fn case_identity(
    case_kind: CaseKind,
    template_version: u32,
    candidate_id: &StableId,
    input_binding_id: &StableId,
    operator_constraints: &OperatorCaseConstraints,
) -> Value {
    json!({
        "candidate_id": candidate_id,
        "case_kind": case_kind,
        "input_binding_id": input_binding_id,
        "operator_constraints": operator_constraints,
        "template_version": template_version,
    })
}

fn probe_id(case_id: &StableId, slot: &str) -> StableId {
    StableId::from_identity(
        "case-probe",
        ENGINEERING_CASE_SCHEMA_VERSION,
        &json!({"case_id": case_id, "slot": slot}),
    )
    .expect("case-probe is a valid typed prefix")
}

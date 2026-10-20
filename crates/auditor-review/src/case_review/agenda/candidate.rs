use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::case_review::identity::{canonical_json, CanonicalIdentity};
use crate::StableId;

pub const REVIEW_CANDIDATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    LifecycleMismatch,
    SharedRootCause,
    RecommendationConsolidation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    EvaluateLifecycleOwnership,
    EvaluateSharedRootCause,
    ReconcileRecommendations,
    ResolveCapabilityEvidenceGap,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    SameSubjectConcentration,
    RepeatedWorkLifecycleIntersection,
    RecommendationOverlap,
    RecommendationConflict,
    CrossLayerIntersection,
    MaterialCapabilityLimitation,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCandidateRef {
    pub candidate_id: StableId,
    pub selector_id: String,
    pub selector_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCandidate {
    pub schema_version: u32,
    pub candidate_id: StableId,
    pub selector_id: String,
    pub selector_version: u32,
    pub decision_kind: DecisionKind,
    pub subject_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub selection_evidence_refs: Vec<String>,
    pub eligible_case_kinds: Vec<CaseKind>,
    pub selection_reasons: Vec<SelectionReason>,
    pub source_candidates: Vec<SourceCandidateRef>,
}

pub struct CandidateSelection {
    pub selector_id: &'static str,
    pub selector_version: u32,
    pub decision_kind: DecisionKind,
    pub subject_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub selection_evidence_refs: Vec<String>,
    pub eligible_case_kinds: Vec<CaseKind>,
    pub selection_reasons: Vec<SelectionReason>,
}

pub(super) struct NormalizedCandidateFields {
    pub decision_kind: DecisionKind,
    pub subject_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub selection_evidence_refs: Vec<String>,
    pub eligible_case_kinds: Vec<CaseKind>,
    pub selection_reasons: Vec<SelectionReason>,
    pub source_candidates: Vec<SourceCandidateRef>,
}

impl ReviewCandidate {
    pub fn from_selection(mut selection: CandidateSelection) -> Result<Self, String> {
        normalize_set(&mut selection.subject_ids);
        normalize_set(&mut selection.finding_ids);
        normalize_set(&mut selection.selection_evidence_refs);
        normalize_set(&mut selection.eligible_case_kinds);
        normalize_set(&mut selection.selection_reasons);
        validate_selector(selection.selector_id, selection.selector_version)?;
        let raw_fields = json!({
            "decision_kind": selection.decision_kind,
            "eligible_case_kinds": selection.eligible_case_kinds,
            "finding_ids": selection.finding_ids,
            "selection_evidence_refs": selection.selection_evidence_refs,
            "selection_reasons": selection.selection_reasons,
            "selector_id": selection.selector_id,
            "selector_version": selection.selector_version,
            "subject_ids": selection.subject_ids,
        });
        let raw_id = StableId::from_identity(
            "selector-candidate",
            REVIEW_CANDIDATE_SCHEMA_VERSION,
            &raw_fields,
        )
        .map_err(|error| error.to_string())?;
        Self::from_normalized(NormalizedCandidateFields {
            decision_kind: selection.decision_kind,
            subject_ids: selection.subject_ids,
            finding_ids: selection.finding_ids,
            selection_evidence_refs: selection.selection_evidence_refs,
            eligible_case_kinds: selection.eligible_case_kinds,
            selection_reasons: selection.selection_reasons,
            source_candidates: vec![SourceCandidateRef {
                candidate_id: raw_id,
                selector_id: selection.selector_id.to_string(),
                selector_version: selection.selector_version,
            }],
        })
    }

    pub(super) fn from_normalized(mut fields: NormalizedCandidateFields) -> Result<Self, String> {
        normalize_set(&mut fields.subject_ids);
        normalize_set(&mut fields.finding_ids);
        normalize_set(&mut fields.selection_evidence_refs);
        normalize_set(&mut fields.eligible_case_kinds);
        normalize_set(&mut fields.selection_reasons);
        normalize_set(&mut fields.source_candidates);
        let primary = fields
            .source_candidates
            .first()
            .ok_or_else(|| "review candidate requires source provenance".to_string())?;
        validate_selector(&primary.selector_id, primary.selector_version)?;
        let mut candidate = Self {
            schema_version: REVIEW_CANDIDATE_SCHEMA_VERSION,
            candidate_id: StableId::from_identity(
                "review-candidate",
                REVIEW_CANDIDATE_SCHEMA_VERSION,
                &Value::Null,
            )
            .expect("review-candidate is a valid typed prefix"),
            selector_id: primary.selector_id.clone(),
            selector_version: primary.selector_version,
            decision_kind: fields.decision_kind,
            subject_ids: fields.subject_ids,
            finding_ids: fields.finding_ids,
            selection_evidence_refs: fields.selection_evidence_refs,
            eligible_case_kinds: fields.eligible_case_kinds,
            selection_reasons: fields.selection_reasons,
            source_candidates: fields.source_candidates,
        };
        candidate.candidate_id = candidate.expected_id();
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let candidate: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid review candidate: {error}"))?;
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn to_canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).expect("review candidate is serializable"))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_header()?;
        self.validate_collections()?;
        self.validate_sources()?;
        if self.candidate_id != self.expected_id() {
            return Err("review candidate stable id mismatch".to_string());
        }
        Ok(())
    }

    fn validate_header(&self) -> Result<(), String> {
        if self.schema_version != REVIEW_CANDIDATE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported review candidate schema version {}",
                self.schema_version
            ));
        }
        if !self.candidate_id.as_str().starts_with("review-candidate:") {
            return Err("review candidate id has the wrong typed prefix".to_string());
        }
        validate_selector(&self.selector_id, self.selector_version)?;
        Ok(())
    }

    fn validate_collections(&self) -> Result<(), String> {
        if self.subject_ids.is_empty()
            || self.selection_evidence_refs.is_empty()
            || self.eligible_case_kinds.is_empty()
            || self.selection_reasons.is_empty()
            || self.source_candidates.is_empty()
        {
            return Err(
                "review candidate requires subjects, evidence, case kinds, reasons, and provenance"
                    .to_string(),
            );
        }
        validate_string_set(&self.subject_ids, "subject ids")?;
        validate_string_set(&self.finding_ids, "finding ids")?;
        validate_string_set(&self.selection_evidence_refs, "selection evidence refs")?;
        validate_sorted_set(&self.eligible_case_kinds, "eligible case kinds")?;
        validate_sorted_set(&self.selection_reasons, "selection reasons")?;
        validate_sorted_set(&self.source_candidates, "source candidates")?;
        Ok(())
    }

    fn validate_sources(&self) -> Result<(), String> {
        if self.source_candidates.iter().any(|source| {
            !source
                .candidate_id
                .as_str()
                .starts_with("selector-candidate:")
                || validate_selector(&source.selector_id, source.selector_version).is_err()
        }) {
            return Err("review candidate contains invalid source selector identity".to_string());
        }
        let primary = &self.source_candidates[0];
        if self.selector_id != primary.selector_id
            || self.selector_version != primary.selector_version
        {
            return Err(
                "review candidate primary selector must match canonical source".to_string(),
            );
        }
        Ok(())
    }

    pub(super) fn expected_id(&self) -> StableId {
        StableId::from_identity(
            "review-candidate",
            REVIEW_CANDIDATE_SCHEMA_VERSION,
            &self.identity_fields(),
        )
        .expect("review-candidate is a valid typed prefix")
    }
}

impl CanonicalIdentity for ReviewCandidate {
    fn identity_fields(&self) -> Value {
        json!({
            "decision_kind": self.decision_kind,
            "eligible_case_kinds": self.eligible_case_kinds,
            "finding_ids": self.finding_ids,
            "selection_evidence_refs": self.selection_evidence_refs,
            "selection_reasons": self.selection_reasons,
            "source_candidates": self.source_candidates,
            "subject_ids": self.subject_ids,
        })
    }
}

fn normalize_set<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

fn validate_selector(id: &str, version: u32) -> Result<(), String> {
    if id.is_empty()
        || version == 0
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(
            "selector identity must be lowercase snake case with a positive version".into(),
        );
    }
    Ok(())
}

fn validate_string_set(values: &[String], label: &str) -> Result<(), String> {
    if values.iter().any(String::is_empty) {
        return Err(format!("review candidate {label} must be nonempty"));
    }
    validate_sorted_set(values, label)
}

fn validate_sorted_set<T: Ord>(values: &[T], label: &str) -> Result<(), String> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!(
            "review candidate {label} must be sorted and unique"
        ));
    }
    Ok(())
}

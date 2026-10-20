use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use super::candidate::{
    CandidateSelection, CaseKind, DecisionKind, ReviewCandidate, SelectionReason,
};
use super::index::{AgendaIndex, FindingFact};
use crate::ReviewInput;

pub const SELECTOR_VERSION: u32 = 1;

const LIFECYCLE_DETECTORS: &[&str] = &[
    "cache_miss_opportunity",
    "hot_path_linear_membership",
    "invariant_collection_copy_in_loop",
    "latest_only_unbounded_cache",
    "loop_artifact_rewrite",
    "loop_invariant_computation",
    "loop_invariant_regex_compilation",
    "n_plus_one_query_in_loop",
    "per_item_external_io",
    "repeated_collection_traversal",
    "repeated_expensive_initialization",
    "repeated_format_allocation",
    "repeated_parse_scan_serialize",
    "repeated_recursive_subproblem",
    "repeated_sort_in_loop",
    "request_path_repository_scan",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateSelectionError {
    DuplicateSelector {
        selector_id: String,
        version: u32,
    },
    InvalidSelector {
        selector_id: String,
        detail: String,
    },
    InvalidCandidate {
        selector_id: String,
        detail: String,
    },
    DanglingReference {
        selector_id: String,
        kind: &'static str,
        id: String,
    },
    InvalidInput {
        detail: String,
    },
    Policy {
        detail: String,
    },
    InvalidAgenda {
        detail: String,
    },
}

impl fmt::Display for CandidateSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSelector {
                selector_id,
                version,
            } => write!(
                formatter,
                "duplicate selector owner {selector_id} v{version}"
            ),
            Self::InvalidSelector {
                selector_id,
                detail,
            } => write!(formatter, "invalid selector {selector_id}: {detail}"),
            Self::InvalidCandidate {
                selector_id,
                detail,
            } => write!(
                formatter,
                "selector {selector_id} emitted invalid candidate: {detail}"
            ),
            Self::DanglingReference {
                selector_id,
                kind,
                id,
            } => write!(
                formatter,
                "selector {selector_id} emitted dangling {kind} reference {id}"
            ),
            Self::InvalidInput { detail } => write!(formatter, "invalid agenda input: {detail}"),
            Self::Policy { detail } => write!(formatter, "invalid agenda policy: {detail}"),
            Self::InvalidAgenda { detail } => write!(formatter, "invalid review agenda: {detail}"),
        }
    }
}

impl std::error::Error for CandidateSelectionError {}

pub trait CandidateSelector {
    fn id(&self) -> &'static str;
    fn version(&self) -> u32;
    fn select(&self, input: &ReviewInput) -> Result<Vec<ReviewCandidate>, CandidateSelectionError>;
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorManifestEntry {
    pub id: String,
    pub version: u32,
    pub order: usize,
}

pub struct SelectorRegistry {
    selectors: Vec<Box<dyn CandidateSelector>>,
}

impl SelectorRegistry {
    pub fn new(
        selectors: Vec<Box<dyn CandidateSelector>>,
    ) -> Result<Self, CandidateSelectionError> {
        let mut owners = BTreeSet::new();
        for selector in &selectors {
            if selector.id().is_empty() || selector.version() == 0 {
                return Err(CandidateSelectionError::InvalidSelector {
                    selector_id: selector.id().to_string(),
                    detail: "selector id and version must be nonempty".to_string(),
                });
            }
            if !owners.insert((selector.id(), selector.version())) {
                return Err(CandidateSelectionError::DuplicateSelector {
                    selector_id: selector.id().to_string(),
                    version: selector.version(),
                });
            }
        }
        Ok(Self { selectors })
    }

    pub fn built_in() -> Self {
        Self::new(
            BuiltInSelectorKind::ALL
                .iter()
                .copied()
                .map(|kind| Box::new(BuiltInSelector { kind }) as Box<dyn CandidateSelector>)
                .collect(),
        )
        .expect("built-in selector owners are unique")
    }

    pub fn manifest(&self) -> Vec<SelectorManifestEntry> {
        self.selectors
            .iter()
            .enumerate()
            .map(|(order, selector)| SelectorManifestEntry {
                id: selector.id().to_string(),
                version: selector.version(),
                order,
            })
            .collect()
    }

    pub(super) fn selectors(&self) -> &[Box<dyn CandidateSelector>] {
        &self.selectors
    }
}

impl Default for SelectorRegistry {
    fn default() -> Self {
        Self::built_in()
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BuiltInSelectorKind {
    SameSubjectConcentration,
    LifecycleIntersection,
    RecommendationOverlap,
    CrossLayerIntersection,
    MaterialCapabilityLimitation,
}

impl BuiltInSelectorKind {
    pub(super) const ALL: [Self; 5] = [
        Self::SameSubjectConcentration,
        Self::LifecycleIntersection,
        Self::RecommendationOverlap,
        Self::CrossLayerIntersection,
        Self::MaterialCapabilityLimitation,
    ];

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::SameSubjectConcentration => "same_subject_concentration",
            Self::LifecycleIntersection => "repeated_work_lifecycle_intersection",
            Self::RecommendationOverlap => "recommendation_overlap_or_conflict",
            Self::CrossLayerIntersection => "cross_layer_intersection",
            Self::MaterialCapabilityLimitation => "material_capability_limitation",
        }
    }

    pub(super) fn select(
        self,
        index: &AgendaIndex,
    ) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
        match self {
            Self::SameSubjectConcentration => same_subject_candidates(index, self.id()),
            Self::LifecycleIntersection => lifecycle_candidates(index, self.id()),
            Self::RecommendationOverlap => recommendation_candidates(index, self.id()),
            Self::CrossLayerIntersection => cross_layer_candidates(index, self.id()),
            Self::MaterialCapabilityLimitation => capability_candidates(index, self.id()),
        }
    }
}

struct BuiltInSelector {
    kind: BuiltInSelectorKind,
}

impl CandidateSelector for BuiltInSelector {
    fn id(&self) -> &'static str {
        self.kind.id()
    }

    fn version(&self) -> u32 {
        SELECTOR_VERSION
    }

    fn select(&self, input: &ReviewInput) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
        self.kind.select(&AgendaIndex::from_input(input)?)
    }
}

fn same_subject_candidates(
    index: &AgendaIndex,
    selector_id: &'static str,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    grouped_findings(index, |fact| Some(fact.subject_id.clone()))
        .into_values()
        .filter(|facts| facts.len() >= 2)
        .map(|facts| {
            build_candidate(
                selector_id,
                DecisionKind::EvaluateSharedRootCause,
                facts,
                CaseKind::SharedRootCause,
                SelectionReason::SameSubjectConcentration,
            )
        })
        .collect()
}

fn lifecycle_candidates(
    index: &AgendaIndex,
    selector_id: &'static str,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    let allowed = LIFECYCLE_DETECTORS.iter().copied().collect::<BTreeSet<_>>();
    grouped_findings(index, |fact| {
        allowed
            .contains(fact.detector.as_str())
            .then(|| fact.subject_id.clone())
    })
    .into_values()
    .map(|facts| {
        build_candidate(
            selector_id,
            DecisionKind::EvaluateLifecycleOwnership,
            facts,
            CaseKind::LifecycleMismatch,
            SelectionReason::RepeatedWorkLifecycleIntersection,
        )
    })
    .collect()
}

fn recommendation_candidates(
    index: &AgendaIndex,
    selector_id: &'static str,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    grouped_findings(index, |fact| {
        Some(format!("{}\0{}", fact.subject_id, fact.recommendation_key))
    })
    .into_values()
    .filter(|facts| facts.len() >= 2)
    .map(|facts| {
        build_candidate(
            selector_id,
            DecisionKind::ReconcileRecommendations,
            facts,
            CaseKind::RecommendationConsolidation,
            SelectionReason::RecommendationOverlap,
        )
    })
    .collect()
}

fn cross_layer_candidates(
    index: &AgendaIndex,
    selector_id: &'static str,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    grouped_findings(index, |fact| Some(fact.subject_id.clone()))
        .into_values()
        .filter(|facts| {
            facts
                .iter()
                .map(|fact| fact.layer.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                >= 2
        })
        .map(|facts| {
            build_candidate(
                selector_id,
                DecisionKind::EvaluateSharedRootCause,
                facts,
                CaseKind::SharedRootCause,
                SelectionReason::CrossLayerIntersection,
            )
        })
        .collect()
}

fn capability_candidates(
    index: &AgendaIndex,
    selector_id: &'static str,
) -> Result<Vec<ReviewCandidate>, CandidateSelectionError> {
    index
        .capabilities()
        .iter()
        .filter(|fact| {
            fact.eligible_files > 0
                && matches!(fact.status.as_str(), "missing_input" | "unavailable")
        })
        .map(|fact| {
            ReviewCandidate::from_selection(CandidateSelection {
                selector_id,
                selector_version: SELECTOR_VERSION,
                decision_kind: DecisionKind::ResolveCapabilityEvidenceGap,
                subject_ids: vec![fact.subject_id.clone()],
                finding_ids: Vec::new(),
                selection_evidence_refs: fact.evidence_refs.clone(),
                eligible_case_kinds: vec![CaseKind::SharedRootCause],
                selection_reasons: vec![SelectionReason::MaterialCapabilityLimitation],
            })
            .map_err(|detail| CandidateSelectionError::InvalidCandidate {
                selector_id: selector_id.to_string(),
                detail,
            })
        })
        .collect()
}

fn grouped_findings(
    index: &AgendaIndex,
    key: impl Fn(&FindingFact) -> Option<String>,
) -> BTreeMap<String, Vec<&FindingFact>> {
    let mut grouped = BTreeMap::<String, Vec<&FindingFact>>::new();
    for fact in index.findings() {
        if let Some(key) = key(fact) {
            grouped.entry(key).or_default().push(fact);
        }
    }
    grouped
}

fn build_candidate(
    selector_id: &'static str,
    decision_kind: DecisionKind,
    facts: Vec<&FindingFact>,
    case_kind: CaseKind,
    reason: SelectionReason,
) -> Result<ReviewCandidate, CandidateSelectionError> {
    ReviewCandidate::from_selection(CandidateSelection {
        selector_id,
        selector_version: SELECTOR_VERSION,
        decision_kind,
        subject_ids: facts.iter().map(|fact| fact.subject_id.clone()).collect(),
        finding_ids: facts.iter().map(|fact| fact.id.clone()).collect(),
        selection_evidence_refs: facts
            .iter()
            .flat_map(|fact| fact.evidence_refs.iter().cloned())
            .collect(),
        eligible_case_kinds: vec![case_kind],
        selection_reasons: vec![reason],
    })
    .map_err(|detail| CandidateSelectionError::InvalidCandidate {
        selector_id: selector_id.to_string(),
        detail,
    })
}

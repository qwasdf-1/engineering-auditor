use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::case_validation::validate_case;
use super::identity::generated_item_id;
use super::neighborhood::case_neighborhood;
use super::{EvidenceError, EvidenceItem, EvidenceItemKind, EvidencePacketBudget};
use crate::{
    repository_map::map_symbol_neighborhood, EngineeringCase, ReviewInput, SourceExcerpt,
    SourceLookup, StableId, TrustLabel,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLookupRequestKind {
    SymbolNeighborhood,
    FindingSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLookupRequest {
    pub case_id: StableId,
    pub probe_id: StableId,
    pub kind: EvidenceLookupRequestKind,
    pub target_id: String,
    pub decision_relevance: String,
    pub hops: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLookupResult {
    pub request_id: StableId,
    pub items: Vec<EvidenceItem>,
}

impl EvidenceLookupResult {
    pub(crate) fn from_request(
        request: &EvidenceLookupRequest,
        mut items: Vec<EvidenceItem>,
    ) -> Result<Self, EvidenceError> {
        let identity = serde_json::to_value(request)
            .map_err(|error| EvidenceError::InvalidLookup(error.to_string()))?;
        let request_id = StableId::from_identity("evidence-lookup", 1, &identity)
            .map_err(|error| EvidenceError::InvalidLookup(error.to_string()))?;
        items.sort_by(|left, right| left.id.cmp(&right.id));
        items.dedup_by(|left, right| left.id == right.id);
        Ok(Self { request_id, items })
    }
}

pub struct EvidenceLookup<'a> {
    case: &'a EngineeringCase,
    input: &'a ReviewInput,
    source: SourceLookup<'a>,
    budget: EvidencePacketBudget,
    allowed_findings: BTreeSet<String>,
    allowed_symbols: BTreeSet<String>,
    probe_ids: BTreeSet<StableId>,
    seen: BTreeSet<StableId>,
    requests: usize,
}

impl<'a> EvidenceLookup<'a> {
    pub fn new(
        repo_root: &Path,
        case: &'a EngineeringCase,
        input: &'a ReviewInput,
        budget: EvidencePacketBudget,
    ) -> Result<Self, EvidenceError> {
        validate_case(case, input)?;
        let budget = budget.tightened_by(input.operator_brief())?;
        let neighborhood = case_neighborhood(case, input, budget.maximum_graph_hops)?;
        let source = SourceLookup::new(repo_root, input.repository_map(), budget.lookup_budget())?;
        Ok(Self {
            case,
            input,
            source,
            budget,
            allowed_findings: neighborhood.finding_ids.into_iter().collect(),
            allowed_symbols: neighborhood.map_symbol_ids.into_iter().collect(),
            probe_ids: case
                .mandatory_probes
                .iter()
                .map(|probe| probe.probe_id.clone())
                .collect(),
            seen: BTreeSet::new(),
            requests: 0,
        })
    }

    pub fn initial_finding_sources(&mut self) -> Result<Vec<EvidenceItem>, EvidenceError> {
        let finding_ids = self
            .allowed_findings
            .iter()
            .take(self.budget.initial_source_excerpts)
            .cloned()
            .collect::<Vec<_>>();
        let mut items = finding_ids
            .iter()
            .map(|finding_id| self.finding_source_item(finding_id))
            .collect::<Result<Vec<_>, _>>()?;
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(items)
    }

    pub fn execute(
        &mut self,
        request: &EvidenceLookupRequest,
    ) -> Result<EvidenceLookupResult, EvidenceError> {
        self.validate_request(request)?;
        let request_id = Self::request_id(request)?;
        if !self.seen.insert(request_id.clone()) {
            return Err(EvidenceError::InvalidLookup(
                "duplicate lookup request".to_string(),
            ));
        }
        self.requests += 1;
        let excerpts = match request.kind {
            EvidenceLookupRequestKind::SymbolNeighborhood => self
                .source
                .symbol_neighborhood(&request.target_id, request.hops)?,
            EvidenceLookupRequestKind::FindingSource => {
                let finding_id = request.target_id.strip_prefix("finding:").ok_or_else(|| {
                    EvidenceError::InvalidLookup(
                        "finding lookup target must use finding:<id>".to_string(),
                    )
                })?;
                return Ok(EvidenceLookupResult {
                    request_id,
                    items: vec![self.finding_source_item(finding_id)?],
                });
            }
        };
        let mut items = excerpts
            .into_iter()
            .map(source_item)
            .collect::<Result<Vec<_>, _>>()?;
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(EvidenceLookupResult { request_id, items })
    }

    fn request_id(request: &EvidenceLookupRequest) -> Result<StableId, EvidenceError> {
        let identity = serde_json::to_value(request)
            .map_err(|error| EvidenceError::InvalidLookup(error.to_string()))?;
        StableId::from_identity("evidence-lookup", 1, &identity)
            .map_err(|error| EvidenceError::InvalidLookup(error.to_string()))
    }

    fn finding_source_item(&mut self, finding_id: &str) -> Result<EvidenceItem, EvidenceError> {
        let finding = self
            .input
            .finding(finding_id)
            .ok_or_else(|| EvidenceError::InvalidLookup("unknown finding target".to_string()))?;
        let location = finding["locations"]
            .as_array()
            .and_then(|locations| locations.first())
            .ok_or_else(|| {
                EvidenceError::InvalidLookup("finding has no source location".to_string())
            })?;
        let path = location["path"].as_str().ok_or_else(|| {
            EvidenceError::InvalidLookup("finding location path is absent".to_string())
        })?;
        let line = location["line"]
            .as_u64()
            .and_then(|line| usize::try_from(line).ok());
        source_item(
            self.source
                .read_source_excerpt_for_finding(finding_id, path, line)?,
        )
    }

    fn validate_request(&self, request: &EvidenceLookupRequest) -> Result<(), EvidenceError> {
        let target = request.target_id.as_str();
        let checks = [
            (
                request.case_id != self.case.case_id,
                "lookup names a different case",
            ),
            (
                !self.probe_ids.contains(&request.probe_id),
                "lookup names an unknown case probe",
            ),
            (
                request.decision_relevance.trim().is_empty(),
                "lookup requires a decision-relevance reason",
            ),
            (
                request.hops > self.budget.maximum_graph_hops,
                "lookup exceeds the case graph-hop ceiling",
            ),
            (
                self.requests >= self.budget.maximum_lookup_requests,
                "lookup request budget is exhausted",
            ),
            (
                target.contains('/') || target.contains('\\') || target.starts_with('.'),
                "raw paths and search expressions are not lookup targets",
            ),
        ];
        let violation = checks
            .into_iter()
            .find(|(violated, _)| *violated)
            .map(|(_, message)| message)
            .or_else(|| {
                (!self.target_is_allowed(request))
                    .then_some("lookup target is unknown or outside the case neighborhood")
            });
        violation.map_or(Ok(()), |message| {
            Err(EvidenceError::InvalidLookup(message.to_string()))
        })
    }

    fn target_is_allowed(&self, request: &EvidenceLookupRequest) -> bool {
        match request.kind {
            EvidenceLookupRequestKind::SymbolNeighborhood => {
                request.target_id.starts_with("symbol:")
                    && self.allowed_symbols.contains(&request.target_id)
                    && self.symbol_expansion_is_allowed(&request.target_id, request.hops)
            }
            EvidenceLookupRequestKind::FindingSource => request
                .target_id
                .strip_prefix("finding:")
                .is_some_and(|id| self.allowed_findings.contains(id)),
        }
    }

    fn symbol_expansion_is_allowed(&self, target_id: &str, hops: usize) -> bool {
        let selected = map_symbol_neighborhood(
            self.input.repository_map(),
            &BTreeSet::from([target_id.to_string()]),
            hops,
        );
        selected.is_subset(&self.allowed_symbols)
    }
}

fn source_item(excerpt: SourceExcerpt) -> Result<EvidenceItem, EvidenceError> {
    let content = serde_json::to_value(&excerpt)
        .map_err(|error| EvidenceError::InvalidPacket(error.to_string()))?;
    EvidenceItem::new(
        generated_item_id("source-span", &content)?,
        EvidenceItemKind::SourceSpan,
        TrustLabel::RepositorySource,
        format!(
            "repository-source#{}:{}-{}",
            excerpt.path, excerpt.start_line, excerpt.end_line
        ),
        content,
    )
}

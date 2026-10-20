use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::report_index::parse_report;
use super::{CandidateSelectionError, ReviewCandidate};
use crate::ReviewInput;

#[derive(Clone, Debug)]
pub(super) struct FindingFact {
    pub id: String,
    pub detector: String,
    pub recommendation_key: String,
    pub subject_id: String,
    pub evidence_refs: Vec<String>,
    pub layer: String,
}

#[derive(Clone, Debug)]
pub(super) struct CapabilityFact {
    pub subject_id: String,
    pub evidence_refs: Vec<String>,
    pub layer: String,
    pub status: String,
    pub eligible_files: usize,
}

#[derive(Clone, Debug)]
pub(super) struct AgendaIndex {
    findings: BTreeMap<String, FindingFact>,
    capabilities: Vec<CapabilityFact>,
    node_ids: BTreeSet<String>,
    adjacency: BTreeMap<String, BTreeSet<String>>,
}

impl AgendaIndex {
    pub fn from_input(input: &ReviewInput) -> Result<Self, CandidateSelectionError> {
        Self::from_report(input.report())
    }

    pub(super) fn from_report(report: &Value) -> Result<Self, CandidateSelectionError> {
        let parsed = parse_report(report)?;
        Ok(Self {
            findings: parsed.findings,
            capabilities: parsed.capabilities,
            node_ids: parsed.node_ids,
            adjacency: parsed.adjacency,
        })
    }

    pub fn findings(&self) -> impl Iterator<Item = &FindingFact> {
        self.findings.values()
    }

    pub fn capabilities(&self) -> &[CapabilityFact] {
        &self.capabilities
    }

    pub fn validate_candidate(
        &self,
        candidate: &ReviewCandidate,
    ) -> Result<(), CandidateSelectionError> {
        candidate
            .validate()
            .map_err(|detail| CandidateSelectionError::InvalidCandidate {
                selector_id: candidate.selector_id.clone(),
                detail,
            })?;
        for finding_id in &candidate.finding_ids {
            if !self.findings.contains_key(finding_id) {
                return Err(dangling(candidate, "finding", finding_id));
            }
        }
        for subject_id in &candidate.subject_ids {
            if !self.node_ids.contains(subject_id) {
                return Err(dangling(candidate, "subject", subject_id));
            }
        }
        for evidence_ref in &candidate.selection_evidence_refs {
            if !self.node_ids.contains(evidence_ref) {
                return Err(dangling(candidate, "evidence", evidence_ref));
            }
        }
        Ok(())
    }

    pub fn connected_components(&self, subjects: &[String]) -> Vec<Vec<String>> {
        let allowed = subjects.iter().cloned().collect::<BTreeSet<_>>();
        let mut remaining = allowed.clone();
        let mut components = Vec::new();
        while let Some(start) = remaining.pop_first() {
            let mut component = BTreeSet::from([start.clone()]);
            let mut queue = VecDeque::from([start]);
            while let Some(subject) = queue.pop_front() {
                for neighbor in self.adjacency.get(&subject).into_iter().flatten() {
                    if allowed.contains(neighbor) && remaining.remove(neighbor) {
                        component.insert(neighbor.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
            components.push(component.into_iter().collect());
        }
        components
    }

    pub fn finding_subject(&self, finding_id: &str) -> Option<&str> {
        self.findings
            .get(finding_id)
            .map(|fact| fact.subject_id.as_str())
    }

    pub fn evidence_for_findings<'a>(
        &'a self,
        finding_ids: impl Iterator<Item = &'a String>,
    ) -> Vec<String> {
        let mut evidence = finding_ids
            .filter_map(|id| self.findings.get(id))
            .flat_map(|fact| fact.evidence_refs.iter().cloned())
            .collect::<Vec<_>>();
        evidence.sort();
        evidence.dedup();
        evidence
    }

    pub fn domains_for_candidate(&self, candidate: &ReviewCandidate) -> BTreeSet<&'static str> {
        let mut domains = candidate
            .finding_ids
            .iter()
            .filter_map(|id| self.findings.get(id))
            .map(|fact| domain_for_layer(&fact.layer))
            .collect::<BTreeSet<_>>();
        if domains.is_empty() {
            domains.extend(
                self.capabilities
                    .iter()
                    .filter(|fact| candidate.subject_ids.contains(&fact.subject_id))
                    .map(|fact| domain_for_layer(&fact.layer)),
            );
        }
        domains
    }

    pub fn all_finding_ids(&self) -> BTreeSet<&str> {
        self.findings.keys().map(String::as_str).collect()
    }

    pub fn contains_node(&self, id: &str) -> bool {
        self.node_ids.contains(id)
    }
}

fn dangling(candidate: &ReviewCandidate, kind: &'static str, id: &str) -> CandidateSelectionError {
    CandidateSelectionError::DanglingReference {
        selector_id: candidate.selector_id.clone(),
        kind,
        id: id.to_string(),
    }
}

fn domain_for_layer(layer: &str) -> &'static str {
    match layer {
        "L1" => "correctness",
        "L2" => "structure",
        "L3" => "algorithmic_cost",
        "L4" => "systems_economics",
        "L5" => "optimization",
        _ => "evidence_coverage",
    }
}

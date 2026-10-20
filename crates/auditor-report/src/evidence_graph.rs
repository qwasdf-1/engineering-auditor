use std::collections::{BTreeMap, BTreeSet};

use auditor_core::{stable_digest, AnalysisFile, DetectorContract, Finding};

use crate::evidence_support::{
    finding_properties, finding_symbol, source_properties, support_node,
};
use crate::{ExternalInput, EVIDENCE_GRAPH_SCHEMA_VERSION};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GraphNodeKind {
    File,
    Symbol,
    Fact,
    Measurement,
    Finding,
    Detector,
    ExternalInput,
}

impl GraphNodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Symbol => "symbol",
            Self::Fact => "fact",
            Self::Measurement => "measurement",
            Self::Finding => "finding",
            Self::Detector => "detector",
            Self::ExternalInput => "external_input",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GraphEdgeKind {
    Contains,
    References,
    DerivedFrom,
    Supports,
    ProducedBy,
}

impl GraphEdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::References => "references",
            Self::DerivedFrom => "derived_from",
            Self::Supports => "supports",
            Self::ProducedBy => "produced_by",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub kind: GraphNodeKind,
    pub label: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: GraphEdgeKind,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphCompleteness {
    pub complete: bool,
    pub files: usize,
    pub findings: usize,
    pub supported_findings: usize,
    pub produced_findings: usize,
    pub exact_fact_spans: usize,
    pub external_inputs: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceGraph {
    pub schema_version: u32,
    pub completeness: GraphCompleteness,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl EvidenceGraph {
    pub fn new() -> Self {
        Self {
            schema_version: EVIDENCE_GRAPH_SCHEMA_VERSION,
            completeness: GraphCompleteness {
                complete: true,
                ..GraphCompleteness::default()
            },
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub(crate) fn build(
        files: &[AnalysisFile],
        detectors: &[DetectorContract],
        findings: &[Finding],
        external_inputs: &[ExternalInput],
    ) -> Self {
        let mut builder = GraphBuilder::default();
        for file in files {
            builder.add_file(file);
        }
        for detector in detectors {
            builder.add_detector(detector);
        }
        for finding in findings {
            builder.add_finding(finding);
        }
        builder.add_external_inputs(external_inputs);
        builder.finish(files.len(), findings.len(), external_inputs.len())
    }
}

impl Default for EvidenceGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct GraphBuilder {
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeSet<GraphEdge>,
    exact_fact_spans: usize,
    supported_findings: BTreeSet<String>,
    produced_findings: BTreeSet<String>,
}

impl GraphBuilder {
    fn add_file(&mut self, file: &AnalysisFile) {
        let id = file_node_id(&file.path);
        self.nodes.entry(id.clone()).or_insert_with(|| GraphNode {
            id,
            kind: GraphNodeKind::File,
            label: file.path.clone(),
            properties: BTreeMap::from([
                ("analysis_method".to_string(), file.analysis_method.clone()),
                ("bytes".to_string(), file.bytes.to_string()),
                (
                    "extractor_version".to_string(),
                    file.extractor_version.to_string(),
                ),
                ("language".to_string(), file.language.clone()),
                ("source_role".to_string(), file.source_role.clone()),
            ]),
        });
    }

    fn add_detector(&mut self, detector: &DetectorContract) {
        let id = detector_node_id(&detector.id);
        self.nodes.insert(
            id.clone(),
            GraphNode {
                id,
                kind: GraphNodeKind::Detector,
                label: detector.id.clone(),
                properties: BTreeMap::from([
                    ("category".to_string(), detector.category.clone()),
                    ("layer".to_string(), detector.layer.clone()),
                    ("profile".to_string(), detector.profile.clone()),
                ]),
            },
        );
    }

    fn add_finding(&mut self, finding: &Finding) {
        let finding_id = finding_node_id(&finding.id);
        self.nodes.insert(
            finding_id.clone(),
            GraphNode {
                id: finding_id.clone(),
                kind: GraphNodeKind::Finding,
                label: finding.title.clone(),
                properties: finding_properties(finding),
            },
        );
        let detector_id = detector_node_id(&finding.detector);
        self.add_edge(&finding_id, &detector_id, GraphEdgeKind::ProducedBy);
        self.produced_findings.insert(finding_id.clone());

        for location in &finding.locations {
            let file_id = self.ensure_location_file(finding, &location.path);
            self.add_edge(&file_id, &finding_id, GraphEdgeKind::Contains);
        }

        let (support_id, support_kind, properties) = support_node(finding);
        if properties.get("span_precision").map(String::as_str) == Some("exact") {
            self.exact_fact_spans += 1;
        }
        self.nodes.insert(
            support_id.clone(),
            GraphNode {
                id: support_id.clone(),
                kind: support_kind,
                label: format!("{} evidence", finding.detector),
                properties,
            },
        );

        if let Some(symbol) = finding_symbol(finding) {
            let symbol_id = symbol_node_id(finding.primary_path(), symbol);
            self.nodes
                .entry(symbol_id.clone())
                .or_insert_with(|| GraphNode {
                    id: symbol_id.clone(),
                    kind: GraphNodeKind::Symbol,
                    label: symbol.to_string(),
                    properties: BTreeMap::from([
                        ("language".to_string(), finding.provenance.language.clone()),
                        ("path".to_string(), finding.primary_path().to_string()),
                    ]),
                });
            let file_id = self.ensure_location_file(finding, finding.primary_path());
            self.add_edge(&file_id, &symbol_id, GraphEdgeKind::Contains);
            self.add_edge(&support_id, &symbol_id, GraphEdgeKind::DerivedFrom);
        } else {
            let file_id = self.ensure_location_file(finding, finding.primary_path());
            self.add_edge(&support_id, &file_id, GraphEdgeKind::DerivedFrom);
        }
        self.add_edge(&support_id, &finding_id, GraphEdgeKind::Supports);
        self.supported_findings.insert(finding_id);
    }

    fn ensure_location_file(&mut self, finding: &Finding, path: &str) -> String {
        let id = file_node_id(path);
        self.nodes.entry(id.clone()).or_insert_with(|| GraphNode {
            id: id.clone(),
            kind: GraphNodeKind::File,
            label: path.to_string(),
            properties: source_properties(finding),
        });
        id
    }

    fn add_external_inputs(&mut self, inputs: &[ExternalInput]) {
        let mut request_id = None;
        for input in inputs {
            let id = external_node_id(input);
            if input.kind == "scan_request" {
                request_id = Some(id.clone());
            }
            self.nodes.insert(
                id.clone(),
                GraphNode {
                    id,
                    kind: GraphNodeKind::ExternalInput,
                    label: input.kind.clone(),
                    properties: BTreeMap::from([
                        ("completeness".to_string(), input.completeness.clone()),
                        ("digest".to_string(), input.digest.clone()),
                        (
                            "schema_version".to_string(),
                            input.schema_version.to_string(),
                        ),
                        ("source".to_string(), input.source.clone()),
                    ]),
                },
            );
        }
        if let Some(request_id) = request_id {
            let file_ids = self
                .nodes
                .values()
                .filter(|node| node.kind == GraphNodeKind::File)
                .map(|node| node.id.clone())
                .collect::<Vec<_>>();
            for file_id in file_ids {
                self.add_edge(&request_id, &file_id, GraphEdgeKind::References);
            }
            for input in inputs.iter().filter(|input| input.kind != "scan_request") {
                self.add_edge(
                    &external_node_id(input),
                    &request_id,
                    GraphEdgeKind::References,
                );
            }
        }
    }

    fn add_edge(&mut self, from: &str, to: &str, kind: GraphEdgeKind) {
        self.edges.insert(GraphEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            properties: BTreeMap::new(),
        });
    }

    fn finish(
        self,
        discovered_files: usize,
        findings: usize,
        external_inputs: usize,
    ) -> EvidenceGraph {
        let represented_files = self
            .nodes
            .values()
            .filter(|node| node.kind == GraphNodeKind::File)
            .count();
        let supported = self.supported_findings.len();
        let produced = self.produced_findings.len();
        EvidenceGraph {
            schema_version: EVIDENCE_GRAPH_SCHEMA_VERSION,
            completeness: GraphCompleteness {
                complete: represented_files >= discovered_files
                    && supported == findings
                    && produced == findings,
                files: represented_files,
                findings,
                supported_findings: supported,
                produced_findings: produced,
                exact_fact_spans: self.exact_fact_spans,
                external_inputs,
            },
            nodes: self.nodes.into_values().collect(),
            edges: self.edges.into_iter().collect(),
        }
    }
}

fn file_node_id(path: &str) -> String {
    format!("file:{}", stable_digest([path]))
}

fn detector_node_id(detector: &str) -> String {
    format!("detector:{detector}")
}

fn finding_node_id(finding: &str) -> String {
    format!("finding:{finding}")
}

fn symbol_node_id(path: &str, symbol: &str) -> String {
    format!("symbol:{}", stable_digest([path, symbol]))
}

fn external_node_id(input: &ExternalInput) -> String {
    format!(
        "external_input:{}",
        stable_digest([
            input.kind.as_str(),
            input.source.as_str(),
            input.digest.as_str()
        ])
    )
}

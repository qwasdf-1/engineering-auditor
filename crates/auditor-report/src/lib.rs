use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use auditor_core::{
    stable_digest, stable_digest_bytes, AnalysisFile, CapabilityExecution, DetectorContract,
    Finding, RepositoryMapArtifact, Severity, SuppressedFinding, SEMANTIC_CONTRACT_VERSION,
};

mod baseline;
mod baseline_match;
mod coverage;
mod evidence_graph;
mod evidence_support;
mod graph_validation;
mod human;
mod json;
mod parser;
pub mod sarif;

pub use baseline::{
    parse_baseline_document, parse_legacy_baseline_reasons, BaselineDocument, BaselineEntry,
};
pub use baseline_match::{match_baseline, BaselineMatch, CarriedFinding, CarryPass, MatchSubject};
pub use evidence_graph::{
    EvidenceGraph, GraphCompleteness, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind,
};
pub use human::{
    report_markdown_document, report_to_human, report_to_human_plain, ColorMode, HumanFooter,
    NoReviewArtifactLoader, ReviewArtifactLoader, ReviewContestedClaim, ReviewDigestSummary,
    ReviewRecommendation,
};
pub use parser::{
    extract_finding_ids, parse_report_findings, parse_report_for_digest, parse_report_identity,
    parse_repository_map_reference, validate_report_json, ParsedFinding, ReportIdentity,
};

pub type DetectorReport = DetectorContract;
pub type FileReport = AnalysisFile;

pub const REPORT_SCHEMA_VERSION: u32 = 5;
pub const EVIDENCE_GRAPH_SCHEMA_VERSION: u32 = 2;
pub const BASELINE_SCHEMA_VERSION: u32 = 2;
pub const REPOSITORY_MAP_SCHEMA_VERSION: u32 = 1;
pub const ENGINEERING_REVIEW_SCHEMA_VERSION: u32 = 1;
pub const ENGINEERING_REVIEW_V2_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryMapReference {
    pub schema_version: u32,
    pub status: String,
    pub path: Option<String>,
    pub digest: Option<String>,
    pub source_snapshot_digest: Option<String>,
    pub completeness: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
}

impl RepositoryMapReference {
    pub fn absent() -> Self {
        Self {
            schema_version: REPOSITORY_MAP_SCHEMA_VERSION,
            status: "absent".to_string(),
            path: None,
            digest: None,
            source_snapshot_digest: None,
            completeness: "not_generated".to_string(),
            file_count: 0,
            symbol_count: 0,
            edge_count: 0,
        }
    }

    pub fn available(artifact: RepositoryMapArtifact) -> Self {
        Self {
            schema_version: REPOSITORY_MAP_SCHEMA_VERSION,
            status: "present".to_string(),
            path: Some(artifact.path),
            digest: Some(artifact.digest),
            source_snapshot_digest: Some(artifact.source_snapshot_digest),
            completeness: artifact.completeness,
            file_count: artifact.file_count,
            symbol_count: artifact.symbol_count,
            edge_count: artifact.edge_count,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineeringReviewEnvelope {
    pub schema_version: u32,
    pub status: String,
    pub review_id: Option<String>,
    pub input_report_digest: Option<String>,
    pub path: Option<String>,
    pub digest: Option<String>,
    pub completeness: String,
    pub assessment_count: usize,
    pub material_recommendation_count: usize,
    pub contested_count: usize,
    pub revision_count: usize,
    pub case_count: usize,
    pub accepted_recommendation_count: usize,
    pub blocked_case_count: usize,
    pub contested_case_count: usize,
    pub challenge_response_count: usize,
}

impl EngineeringReviewEnvelope {
    pub fn absent() -> Self {
        Self {
            schema_version: ENGINEERING_REVIEW_SCHEMA_VERSION,
            status: "absent".to_string(),
            review_id: None,
            input_report_digest: None,
            path: None,
            digest: None,
            completeness: "not_requested".to_string(),
            assessment_count: 0,
            material_recommendation_count: 0,
            contested_count: 0,
            revision_count: 0,
            case_count: 0,
            accepted_recommendation_count: 0,
            blocked_case_count: 0,
            contested_case_count: 0,
            challenge_response_count: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindingGroup {
    pub category: String,
    pub count: usize,
    pub highest_severity: Severity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineDiff {
    pub schema_version: u32,
    pub baseline_path: Option<String>,
    pub compared: bool,
    pub new_findings: usize,
    pub resolved_findings: usize,
    pub unchanged_findings: usize,
    /// Accepted findings whose ID changed but whose identity was carried by a
    /// unique deterministic match (renamed symbol, moved file, ordinal shift).
    pub carried: Vec<CarriedFinding>,
}

impl BaselineDiff {
    pub fn none() -> Self {
        Self {
            schema_version: BASELINE_SCHEMA_VERSION,
            baseline_path: None,
            compared: false,
            new_findings: 0,
            resolved_findings: 0,
            unchanged_findings: 0,
            carried: Vec::new(),
        }
    }

    fn from_match(baseline_path: impl Into<String>, matched: BaselineMatch) -> Self {
        Self {
            schema_version: BASELINE_SCHEMA_VERSION,
            baseline_path: Some(baseline_path.into()),
            compared: true,
            new_findings: matched.new.len(),
            resolved_findings: matched.resolved.len(),
            unchanged_findings: matched.unchanged.len(),
            carried: matched.carried,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalInput {
    pub kind: String,
    pub source: String,
    pub schema_version: u32,
    pub digest: String,
    pub completeness: String,
    pub contract_relevant: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalInputMetadata {
    pub kind: String,
    pub source: String,
    pub schema_version: u32,
    pub completeness: String,
    pub contract_relevant: bool,
}

impl ExternalInputMetadata {
    pub fn analysis(
        kind: impl Into<String>,
        source: impl Into<String>,
        schema_version: u32,
        completeness: impl Into<String>,
    ) -> Self {
        Self::new(kind, source, schema_version, completeness, true)
    }

    pub fn supplemental(
        kind: impl Into<String>,
        source: impl Into<String>,
        schema_version: u32,
        completeness: impl Into<String>,
    ) -> Self {
        Self::new(kind, source, schema_version, completeness, false)
    }

    fn new(
        kind: impl Into<String>,
        source: impl Into<String>,
        schema_version: u32,
        completeness: impl Into<String>,
        contract_relevant: bool,
    ) -> Self {
        Self {
            kind: kind.into(),
            source: source.into(),
            schema_version,
            completeness: completeness.into(),
            contract_relevant,
        }
    }
}

impl ExternalInput {
    pub fn from_text(metadata: ExternalInputMetadata, text: &str) -> Self {
        Self {
            kind: metadata.kind,
            source: metadata.source,
            schema_version: metadata.schema_version,
            digest: stable_digest_bytes(text.as_bytes()),
            completeness: metadata.completeness,
            contract_relevant: metadata.contract_relevant,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilityCoverage {
    pub semantic: usize,
    pub exact_lexical: usize,
    pub partial: usize,
    pub unsupported: usize,
    pub not_applicable: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnalysisCoverage {
    pub requested_method: String,
    pub files: usize,
    pub source_files: usize,
    pub detector_language_claims: usize,
    pub capability_modes: CapabilityCoverage,
    pub findings: usize,
    pub line_precise_findings: usize,
    pub repository_findings: usize,
    pub exact_fact_spans: usize,
    pub external_inputs: usize,
    pub graph_complete: bool,
    /// Files parsed only after quarantining declarations with syntax errors.
    pub syntax_recovered_files: usize,
    /// Files whose syntax errors kept them out of structural analysis.
    pub syntax_unparsed_files: usize,
    /// Findings inline `auditor-ignore` comments removed.
    pub suppressed_findings: usize,
}

#[derive(Clone, Debug)]
pub struct AuditReport {
    pub schema_version: u32,
    pub semantic_contract_version: u32,
    pub finding_id_version: u32,
    pub source_role_version: u32,
    pub analysis_contract_digest: String,
    pub scope: String,
    pub profile: String,
    pub repo_root: String,
    pub analysis_method: String,
    pub scanned_files: usize,
    pub languages: Vec<(String, usize)>,
    pub source_roles: Vec<(String, usize)>,
    pub files: Vec<FileReport>,
    pub detectors: Vec<DetectorReport>,
    pub capability_executions: Vec<CapabilityExecution>,
    pub repository_map: RepositoryMapReference,
    pub engineering_review: EngineeringReviewEnvelope,
    pub finding_groups: Vec<FindingGroup>,
    pub analysis_coverage: AnalysisCoverage,
    pub external_inputs: Vec<ExternalInput>,
    pub baseline: BaselineDiff,
    pub findings: Vec<Finding>,
    /// Findings inline `auditor-ignore` comments removed, with their reasons.
    pub suppressed_findings: Vec<SuppressedFinding>,
    pub evidence_graph: EvidenceGraph,
}

impl AuditReport {
    pub fn empty(profile: impl Into<String>, repo_root: impl Into<String>) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            semantic_contract_version: SEMANTIC_CONTRACT_VERSION,
            finding_id_version: auditor_core::FINDING_ID_VERSION,
            source_role_version: 1,
            analysis_contract_digest: String::new(),
            scope: "full".to_string(),
            profile: profile.into(),
            repo_root: repo_root.into(),
            analysis_method: "auto".to_string(),
            scanned_files: 0,
            languages: Vec::new(),
            source_roles: Vec::new(),
            files: Vec::new(),
            detectors: Vec::new(),
            capability_executions: Vec::new(),
            repository_map: RepositoryMapReference::absent(),
            engineering_review: EngineeringReviewEnvelope::absent(),
            finding_groups: Vec::new(),
            analysis_coverage: AnalysisCoverage::default(),
            external_inputs: Vec::new(),
            baseline: BaselineDiff::none(),
            findings: Vec::new(),
            suppressed_findings: Vec::new(),
            evidence_graph: EvidenceGraph::new(),
        }
    }

    pub fn set_analysis(
        &mut self,
        files: Vec<FileReport>,
        detectors: Vec<DetectorReport>,
        capability_executions: Vec<CapabilityExecution>,
        findings: Vec<Finding>,
        analysis_method: impl Into<String>,
    ) {
        self.analysis_method = analysis_method.into();
        self.scanned_files = files.len();
        self.languages = counted(files.iter().map(|file| file.language.as_str()));
        self.source_roles = counted(files.iter().map(|file| file.source_role.as_str()));
        self.files = files;
        self.detectors = detectors;
        self.capability_executions = capability_executions;
        self.findings = findings;
    }

    pub fn finalize(&mut self) {
        self.findings.sort_by(|left, right| {
            (
                left.primary_path(),
                left.primary_line().unwrap_or(0),
                left.detector.as_str(),
                left.title.as_str(),
            )
                .cmp(&(
                    right.primary_path(),
                    right.primary_line().unwrap_or(0),
                    right.detector.as_str(),
                    right.title.as_str(),
                ))
        });
        self.files.sort_by(|left, right| left.path.cmp(&right.path));
        self.finding_groups = group_findings(&self.findings);
        let counts = detector_counts(&self.findings);
        for detector in &mut self.detectors {
            detector.finding_count = *counts.get(&detector.id).unwrap_or(&0);
        }
        self.detectors.sort_by(|left, right| left.id.cmp(&right.id));
        self.capability_executions
            .sort_by(|left, right| left.detector_id.cmp(&right.detector_id));
        sort_external_inputs(&mut self.external_inputs);
        self.analysis_coverage = coverage::analysis_coverage(self);
        self.analysis_contract_digest = analysis_contract_digest(self);
        self.refresh_graph();
    }

    pub fn apply_baseline_file(&mut self, baseline_path: &Path) -> Result<(), String> {
        let text = fs::read_to_string(baseline_path)
            .map_err(|error| format!("failed to read baseline document: {error}"))?;
        let baseline = parse_baseline_document(&text)?;
        if baseline.identity() != self.identity() {
            return Err(
                "baseline identity is incompatible with the current analysis contract".to_string(),
            );
        }
        let accepted = baseline
            .entries
            .iter()
            .map(BaselineEntry::match_subject)
            .collect::<Vec<_>>();
        let current = self
            .findings
            .iter()
            .map(|finding| MatchSubject {
                id: &finding.id,
                detector: &finding.detector,
                title: &finding.title,
                path: finding.primary_path(),
                message: &finding.message,
                identity: &finding.identity,
            })
            .collect::<Vec<_>>();
        let matched = match_baseline(&accepted, &current);
        self.baseline = BaselineDiff::from_match(baseline_path.display().to_string(), matched);
        self.external_inputs.push(ExternalInput::from_text(
            ExternalInputMetadata::supplemental(
                "accepted_baseline",
                baseline_path.display().to_string(),
                BASELINE_SCHEMA_VERSION,
                "reviewed",
            ),
            &text,
        ));
        sort_external_inputs(&mut self.external_inputs);
        self.analysis_coverage.external_inputs = self.external_inputs.len();
        self.refresh_graph();
        Ok(())
    }

    pub fn identity(&self) -> ReportIdentity {
        ReportIdentity {
            schema_version: self.schema_version,
            semantic_contract_version: self.semantic_contract_version,
            evidence_graph_schema_version: self.evidence_graph.schema_version,
            finding_id_version: self.finding_id_version,
            source_role_version: self.source_role_version,
            profile: self.profile.clone(),
            scope: self.scope.clone(),
            analysis_method: self.analysis_method.clone(),
            analysis_contract_digest: self.analysis_contract_digest.clone(),
        }
    }

    pub fn to_json(&self) -> String {
        json::report_to_json(self)
    }

    fn refresh_graph(&mut self) {
        self.evidence_graph = EvidenceGraph::build(
            &self.files,
            &self.detectors,
            &self.findings,
            &self.external_inputs,
        );
        self.analysis_coverage.exact_fact_spans = self.evidence_graph.completeness.exact_fact_spans;
        self.analysis_coverage.graph_complete = self.evidence_graph.completeness.complete;
    }
}

fn counted<'a>(values: impl Iterator<Item = &'a str>) -> Vec<(String, usize)> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value.to_string()).or_default() += 1;
    }
    counts.into_iter().collect()
}

fn sort_external_inputs(inputs: &mut [ExternalInput]) {
    inputs.sort_by(|left, right| {
        (&left.kind, &left.source, &left.digest).cmp(&(&right.kind, &right.source, &right.digest))
    });
}

fn analysis_contract_digest(report: &AuditReport) -> String {
    let mut values = vec![
        report.schema_version.to_string(),
        report.semantic_contract_version.to_string(),
        EVIDENCE_GRAPH_SCHEMA_VERSION.to_string(),
        report.finding_id_version.to_string(),
        report.source_role_version.to_string(),
        report.profile.clone(),
        report.analysis_method.clone(),
    ];
    for detector in &report.detectors {
        values.extend([
            detector.id.clone(),
            detector.category.clone(),
            detector.default_severity.as_str().to_string(),
            detector.profile.clone(),
            detector.maturity.clone(),
            detector.layer.clone(),
        ]);
        values.extend(detector.emission_roles.iter().cloned());
        values.extend(detector.context_roles.iter().cloned());
        for claim in &detector.language_support {
            values.push(claim.language.clone());
            values.push(claim.mode.clone());
            values.push(claim.limitation.clone().unwrap_or_default());
        }
    }
    for input in report
        .external_inputs
        .iter()
        .filter(|input| input.contract_relevant)
    {
        values.extend([
            input.kind.clone(),
            input.schema_version.to_string(),
            input.completeness.clone(),
            input.digest.clone(),
        ]);
    }
    stable_digest(values.iter().map(String::as_str))
}

fn detector_counts(findings: &[Finding]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.detector.clone()).or_default() += 1;
    }
    counts
}

fn group_findings(findings: &[Finding]) -> Vec<FindingGroup> {
    let mut groups: BTreeMap<String, (usize, Severity)> = BTreeMap::new();
    for finding in findings {
        let entry = groups
            .entry(finding.category.clone())
            .or_insert((0, Severity::Advisory));
        entry.0 += 1;
        entry.1 = entry.1.max(finding.severity);
    }
    groups
        .into_iter()
        .map(|(category, (count, highest_severity))| FindingGroup {
            category,
            count,
            highest_severity,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_core::{
        CapabilityExecutionStatus, Confidence, FindingProvenance, LanguageCapability,
    };

    fn detector() -> DetectorReport {
        DetectorReport {
            id: "test_detector".to_string(),
            category: "structural".to_string(),
            default_severity: Severity::Medium,
            profile: "gating".to_string(),
            maturity: "v0".to_string(),
            layer: "L2".to_string(),
            emission_roles: vec!["production".to_string()],
            context_roles: vec!["production".to_string(), "test".to_string()],
            language_support: vec![LanguageCapability {
                language: "rust".to_string(),
                mode: "semantic".to_string(),
                limitation: None,
            }],
            finding_count: 0,
        }
    }

    fn finding() -> Finding {
        auditor_core::finding!(
            "test_detector",
            "structural",
            "Test finding",
            "test message",
            Severity::Medium,
            Confidence::High,
            "src/lib.rs",
            Some(5),
        )
        .with_recommendation("fix it")
        .with_evidence("fact_span", "5:1-5:8")
        .with_provenance(FindingProvenance {
            language: "rust".to_string(),
            source_role: "production".to_string(),
            analysis_method: "ast".to_string(),
            extractor_version: 1,
            origin: "native".to_string(),
            span_precision: "line".to_string(),
        })
    }

    fn report() -> AuditReport {
        let mut report = AuditReport::empty("gating", ".");
        report.external_inputs.push(ExternalInput::from_text(
            ExternalInputMetadata::analysis("scan_request", "cli", 1, "complete"),
            "full:auto",
        ));
        report.set_analysis(
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                language: "rust".to_string(),
                catalog_language: "rust".to_string(),
                bytes: 42,
                source_role: "production".to_string(),
                analysis_method: "ast".to_string(),
                extractor_version: 1,
                syntax: None,
            }],
            vec![detector()],
            vec![CapabilityExecution {
                detector_id: "test_detector".to_string(),
                layer: "L2".to_string(),
                status: CapabilityExecutionStatus::Executed,
                eligible_files: 1,
                analyzed_files: 1,
                unsupported_files: 0,
                not_applicable_files: 0,
                required_inputs: Vec::new(),
                missing_inputs: Vec::new(),
                limitations: Vec::new(),
                failure_reason: None,
            }],
            vec![finding()],
            "ast",
        );
        report.finalize();
        report
    }

    #[test]
    fn report_v5_is_deterministic_and_valid() {
        let report = report();
        let first = report.to_json();
        let second = report.to_json();
        assert_eq!(first, second);
        assert!(first.contains("\"schema_version\": 5"));
        assert!(first.contains("\"status\": \"executed\""));
        assert!(first.contains("\"semantic_contract_version\": 1"));
        validate_report_json(&first).unwrap();
    }

    /// `parse_report_for_digest` reconstructs every renderer-relevant field
    /// of an `AuditReport` from a saved JSON report. Round-trip a
    /// canonical report through `to_json` -> `parse_report_for_digest` ->
    /// `report_to_human` and assert the digest carries the header, totals,
    /// detector table, per-finding section, and the populated finding.
    #[test]
    fn parse_report_for_digest_reconstructs_renderer_fields() {
        let report = report();
        let machine = report.to_json();
        validate_report_json(&machine).unwrap();
        let reconstructed = parse_report_for_digest(&machine).expect("digest parse succeeds");
        // Renderer-relevant fields round-trip.
        assert_eq!(reconstructed.schema_version, report.schema_version);
        assert_eq!(reconstructed.finding_id_version, report.finding_id_version);
        assert_eq!(reconstructed.profile, report.profile);
        assert_eq!(reconstructed.scope, report.scope);
        assert_eq!(reconstructed.repo_root, report.repo_root);
        assert_eq!(reconstructed.analysis_method, report.analysis_method);
        assert_eq!(reconstructed.scanned_files, report.scanned_files);
        assert_eq!(reconstructed.languages, report.languages);
        assert_eq!(reconstructed.source_roles, report.source_roles);
        assert_eq!(reconstructed.detectors.len(), report.detectors.len());
        assert_eq!(
            reconstructed.capability_executions.len(),
            report.capability_executions.len()
        );
        assert_eq!(reconstructed.findings.len(), report.findings.len());
        let finding = &reconstructed.findings[0];
        assert_eq!(finding.id, report.findings[0].id);
        assert_eq!(finding.detector, "test_detector");
        assert_eq!(finding.primary_path(), "src/lib.rs");
        assert_eq!(finding.primary_line(), Some(5));
        assert_eq!(finding, &report.findings[0]);
        // Only catalogs and the full graph, which the renderer does not
        // expose, remain intentionally unmaterialized.
        assert!(reconstructed.files.is_empty());
        assert_eq!(reconstructed.external_inputs, report.external_inputs);
        assert_eq!(reconstructed.repository_map, report.repository_map);
        assert_eq!(reconstructed.analysis_coverage, report.analysis_coverage);
        // The reconstructed report renders a populated digest.
        let footer = HumanFooter {
            sibling_path: "audit.audit.json".to_string(),
            sibling_digest: "0".repeat(64),
        };
        let digest = report_to_human(
            &reconstructed,
            ColorMode::Plain,
            &footer,
            false,
            &NoReviewArtifactLoader,
        );
        assert!(digest.contains("# Engineering Auditor"));
        assert!(digest.contains("## Severity Totals"));
        assert!(digest.contains("## Per-Detector Breakdown"));
        // Findings section carries the finding row.
        assert!(digest.contains("test_detector"));
        assert!(digest.contains("src/lib.rs"));
        assert!(digest.contains("Test finding"));
    }

    /// The renderer adapter rejects missing renderer-relevant contract
    /// fields instead of inventing values that look authoritative.
    #[test]
    fn parse_report_for_digest_rejects_incomplete_renderer_surface() {
        let bad = serde_json::json!({
            "schema_version": 5,
            "semantic_contract_version": 1,
            "evidence_graph": {"schema_version": 2},
            "finding_id_version": auditor_core::FINDING_ID_VERSION,
            "source_role_version": 1,
            "profile": "gating",
            "scope": "full",
            "analysis_method": "auto",
            "analysis_contract_digest": "deadbeef",
            "repo_root": ".",
            "engineering_review": {
                "status": "absent",
                "completeness": "not_requested",
            },
            "baseline": {"compared": false},
            "findings": "not-an-array",
            "detectors": "not-an-array",
        })
        .to_string();
        let error = parse_report_for_digest(&bad).expect_err("incomplete report must fail");
        assert!(error.contains("repository_map"), "error: {error}");
    }

    #[test]
    fn parse_report_for_digest_rejects_missing_repo_root() {
        let bad = serde_json::json!({
            "schema_version": 5,
            "semantic_contract_version": 1,
            "evidence_graph": {"schema_version": 2},
            "finding_id_version": auditor_core::FINDING_ID_VERSION,
            "source_role_version": 1,
            "profile": "gating",
            "scope": "full",
            "analysis_method": "auto",
            "analysis_contract_digest": "deadbeef",
            "engineering_review": {
                "status": "absent",
                "completeness": "not_requested",
            },
            "baseline": {"compared": false},
        })
        .to_string();
        let error = parse_report_for_digest(&bad).expect_err("must reject");
        assert!(error.contains("repo_root"), "error: {error}");
    }

    #[test]
    fn evidence_graph_v2_has_typed_support_and_exact_fact_span() {
        let report = report();
        assert_eq!(report.evidence_graph.schema_version, 2);
        assert!(report.evidence_graph.completeness.complete);
        assert_eq!(report.evidence_graph.completeness.exact_fact_spans, 1);
        assert!(report
            .evidence_graph
            .edges
            .iter()
            .any(|edge| edge.kind == GraphEdgeKind::Supports));
        assert!(report
            .evidence_graph
            .nodes
            .iter()
            .any(|node| matches!(node.kind, GraphNodeKind::Fact | GraphNodeKind::Measurement)));
    }

    #[test]
    fn report_parser_reads_own_findings_and_rejects_v4() {
        let report = report();
        let parsed = parse_report_findings(&report.to_json()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].line, Some(5));
        let v4 = report
            .to_json()
            .replacen("\"schema_version\": 5", "\"schema_version\": 4", 1);
        assert!(parse_report_identity(&v4).is_err());
    }

    #[test]
    fn report_v5_rejects_a_catalog_without_exact_execution_truth() {
        let report = report().to_json();
        let mut value: serde_json::Value = serde_json::from_str(&report).unwrap();
        value["capability_executions"] = serde_json::json!([]);
        assert!(validate_report_json(&serde_json::to_string(&value).unwrap()).is_err());

        let mut value: serde_json::Value = serde_json::from_str(&report).unwrap();
        value["capability_executions"][0]["status"] = serde_json::json!("not_applicable");
        assert!(validate_report_json(&serde_json::to_string(&value).unwrap()).is_err());
    }

    #[test]
    fn standalone_baseline_round_trip_is_explicit() {
        let report = report();
        let document = BaselineDocument::from_report(&report, "reviewed maintained debt").unwrap();
        let json = document.to_json();
        let parsed = parse_baseline_document(&json).unwrap();
        assert_eq!(parsed.identity(), report.identity());
        assert_eq!(parsed.entries.len(), 1);
        assert!(parse_baseline_document(&report.to_json()).is_err());
    }

    #[test]
    fn analysis_digest_binds_analysis_inputs_but_not_policy_artifacts() {
        let base = report();
        let mut policy = report();
        policy.external_inputs.push(ExternalInput::from_text(
            ExternalInputMetadata::supplemental(
                "accepted_baseline",
                "baseline.json",
                1,
                "reviewed",
            ),
            "reviewed debt",
        ));
        policy.finalize();
        assert_eq!(
            base.analysis_contract_digest,
            policy.analysis_contract_digest
        );

        let mut graph = report();
        graph.external_inputs.push(ExternalInput::from_text(
            ExternalInputMetadata::analysis("repo_graph", "repo-graph.json", 1, "authoritative"),
            "authoritative graph",
        ));
        graph.finalize();
        assert_ne!(
            base.analysis_contract_digest,
            graph.analysis_contract_digest
        );
    }

    #[test]
    fn map_and_review_envelopes_do_not_change_analysis_or_finding_identity() {
        let base = report();
        let mut enriched = base.clone();
        enriched.repository_map = RepositoryMapReference::available(RepositoryMapArtifact {
            path: "audit.repository-map.digest.json".to_string(),
            digest: "map-digest".to_string(),
            source_snapshot_digest: "snapshot-digest".to_string(),
            completeness: "complete_with_explicit_unresolved_edges".to_string(),
            file_count: 1,
            symbol_count: 2,
            edge_count: 1,
        });
        enriched.engineering_review = EngineeringReviewEnvelope {
            schema_version: ENGINEERING_REVIEW_SCHEMA_VERSION,
            status: "complete".to_string(),
            review_id: Some("review:identity".to_string()),
            input_report_digest: Some("input-digest".to_string()),
            path: Some("audit.engineering-review.digest.json".to_string()),
            digest: Some("review-digest".to_string()),
            completeness: "complete".to_string(),
            assessment_count: 3,
            material_recommendation_count: 1,
            contested_count: 0,
            revision_count: 0,
            case_count: 0,
            accepted_recommendation_count: 0,
            blocked_case_count: 0,
            contested_case_count: 0,
            challenge_response_count: 0,
        };
        enriched.finalize();
        assert_eq!(
            base.analysis_contract_digest,
            enriched.analysis_contract_digest
        );
        assert_eq!(base.findings, enriched.findings);
        assert_eq!(base.evidence_graph, enriched.evidence_graph);
    }

    #[test]
    fn stability_policy_classifies_every_contract_schema() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let policy = fs::read_to_string(root.join("docs/reference/stability.md")).unwrap();
        let classified = policy
            .lines()
            .filter_map(|line| {
                let cells = line.trim_matches('|').split('|').map(str::trim);
                let cells = cells.collect::<Vec<_>>();
                let schema = cells.first()?.strip_prefix('`')?.strip_suffix('`')?;
                let class = *cells.get(1)?;
                (schema.ends_with(".schema.json")
                    && ["stable", "experimental", "internal"].contains(&class))
                .then(|| schema.to_string())
            })
            .collect::<Vec<_>>();
        let unique = classified
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), classified.len(), "schema classified twice");
        let schemas = fs::read_dir(root.join("contracts"))
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.ends_with(".schema.json"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique, schemas);
    }

    #[test]
    fn checked_in_contract_schemas_publish_current_versions() {
        let report_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/audit-report.schema.json"))
                .unwrap();
        let graph_schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/evidence-graph.schema.json"
        ))
        .unwrap();
        let baseline_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/baseline.schema.json")).unwrap();
        assert_eq!(
            report_schema["properties"]["schema_version"]["const"],
            REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            graph_schema["properties"]["schema_version"]["const"],
            EVIDENCE_GRAPH_SCHEMA_VERSION
        );
        assert_eq!(
            baseline_schema["properties"]["schema_version"]["const"],
            BASELINE_SCHEMA_VERSION
        );

        for (name, schema) in [
            (
                "audit-gate",
                include_str!("../../../contracts/audit-gate.schema.json"),
            ),
            (
                "audit-report",
                include_str!("../../../contracts/audit-report.schema.json"),
            ),
            (
                "baseline",
                include_str!("../../../contracts/baseline.schema.json"),
            ),
            (
                "co-edit-journal",
                include_str!("../../../contracts/co-edit-journal.schema.json"),
            ),
            (
                "detector-manifest",
                include_str!("../../../contracts/detector-manifest.schema.json"),
            ),
            (
                "donor-manifest",
                include_str!("../../../contracts/donor-manifest.schema.json"),
            ),
            (
                "evidence-graph",
                include_str!("../../../contracts/evidence-graph.schema.json"),
            ),
            (
                "repository-graph",
                include_str!("../../../contracts/repository-graph.schema.json"),
            ),
            (
                "repository-map",
                include_str!("../../../contracts/repository-map.schema.json"),
            ),
            (
                "review-brief",
                include_str!("../../../contracts/review-brief.schema.json"),
            ),
            (
                "review-dispositions",
                include_str!("../../../contracts/review-dispositions.schema.json"),
            ),
        ] {
            serde_json::from_str::<serde_json::Value>(schema)
                .unwrap_or_else(|error| panic!("invalid {name} schema: {error}"));
        }

        let contract_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
        for name in [
            "case-template-bundle-v1",
            "case-template-v1",
            "challenge-record-v1",
            "challenge-response-v1",
            "contextual-recommendation-v1",
            "engineering-case-v1",
            "engineering-claim-v1",
            "engineering-review-v2",
            "evidence-packet-v1",
            "probe-definition-v1",
            "probe-result-v1",
            "review-agenda-v1",
            "review-candidate-v1",
            "review-eval-corpus-v2",
            "review-prompt-bundle-v2",
            "review-qualification-v2",
            "review-routes-v2",
            "reviewer-case-review-v1",
        ] {
            let schema = fs::read_to_string(contract_root.join(format!("{name}.schema.json")))
                .unwrap_or_else(|error| panic!("cannot read {name} schema: {error}"));
            serde_json::from_str::<serde_json::Value>(&schema)
                .unwrap_or_else(|error| panic!("invalid {name} schema: {error}"));
        }
    }
}

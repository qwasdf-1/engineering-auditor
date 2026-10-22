//! `analysis_coverage`: what a report's scan covered, counted from the
//! report itself.

use crate::{AnalysisCoverage, AuditReport, CapabilityCoverage};

pub(crate) fn analysis_coverage(report: &AuditReport) -> AnalysisCoverage {
    let mut modes = CapabilityCoverage::default();
    for claim in report
        .detectors
        .iter()
        .flat_map(|detector| &detector.language_support)
    {
        match claim.mode.as_str() {
            "semantic" => modes.semantic += 1,
            "exact_lexical" => modes.exact_lexical += 1,
            "partial" => modes.partial += 1,
            "unsupported" => modes.unsupported += 1,
            "not_applicable" => modes.not_applicable += 1,
            _ => {}
        }
    }
    AnalysisCoverage {
        requested_method: report.analysis_method.clone(),
        files: report.files.len(),
        source_files: report
            .files
            .iter()
            .filter(|file| !matches!(file.catalog_language.as_str(), "markdown" | "structured"))
            .count(),
        detector_language_claims: report
            .detectors
            .iter()
            .map(|detector| detector.language_support.len())
            .sum(),
        capability_modes: modes,
        findings: report.findings.len(),
        line_precise_findings: report
            .findings
            .iter()
            .filter(|finding| finding.provenance.span_precision == "line")
            .count(),
        repository_findings: report
            .findings
            .iter()
            .filter(|finding| finding.provenance.span_precision == "repository")
            .count(),
        exact_fact_spans: 0,
        external_inputs: report.external_inputs.len(),
        graph_complete: false,
        syntax_recovered_files: syntax_count(report, "recovered"),
        syntax_unparsed_files: syntax_count(report, "unparsed"),
        suppressed_findings: report.suppressed_findings.len(),
    }
}

fn syntax_count(report: &AuditReport, status: &str) -> usize {
    report
        .files
        .iter()
        .filter(|file| file.syntax.as_deref() == Some(status))
        .count()
}

use std::collections::BTreeMap;

use auditor_core::{stable_digest, EvidenceValue, Finding};

use crate::GraphNodeKind;

pub(super) fn source_properties(finding: &Finding) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "analysis_method".to_string(),
            finding.provenance.analysis_method.clone(),
        ),
        (
            "extractor_version".to_string(),
            finding.provenance.extractor_version.to_string(),
        ),
        ("language".to_string(), finding.provenance.language.clone()),
        (
            "source_role".to_string(),
            finding.provenance.source_role.clone(),
        ),
    ])
}

pub(super) fn finding_properties(finding: &Finding) -> BTreeMap<String, String> {
    let mut properties = source_properties(finding);
    properties.extend([
        ("origin".to_string(), finding.provenance.origin.clone()),
        ("path".to_string(), finding.primary_path().to_string()),
        (
            "severity".to_string(),
            finding.severity.as_str().to_string(),
        ),
        (
            "span_precision".to_string(),
            finding.provenance.span_precision.clone(),
        ),
    ]);
    add_line_span(&mut properties, finding);
    properties
}

pub(super) fn support_node(finding: &Finding) -> (String, GraphNodeKind, BTreeMap<String, String>) {
    let kind = support_kind(finding);
    let evidence = finding
        .evidence
        .iter()
        .map(|(key, value)| format!("{key}={}", value.to_json()))
        .collect::<Vec<_>>();
    let mut properties = source_properties(finding);
    properties.extend([
        (
            "evidence_digest".to_string(),
            stable_digest(evidence.iter().map(String::as_str)),
        ),
        (
            "evidence_keys".to_string(),
            finding.evidence_keys().join(","),
        ),
        ("origin".to_string(), finding.provenance.origin.clone()),
    ]);
    if let Some(span) = exact_evidence_span(finding) {
        span.add_to(&mut properties);
    } else {
        properties.insert(
            "span_precision".to_string(),
            finding.provenance.span_precision.clone(),
        );
        add_line_span(&mut properties, finding);
    }
    (
        format!("{}:{}", kind.as_str(), finding.id),
        kind,
        properties,
    )
}

fn support_kind(finding: &Finding) -> GraphNodeKind {
    if finding
        .evidence
        .values()
        .any(|value| matches!(value, EvidenceValue::Integer(_) | EvidenceValue::Float(_)))
    {
        GraphNodeKind::Measurement
    } else {
        GraphNodeKind::Fact
    }
}

fn add_line_span(properties: &mut BTreeMap<String, String>, finding: &Finding) {
    if let Some(line) = finding.primary_line() {
        properties.insert("start_line".to_string(), line.to_string());
        properties.insert("end_line".to_string(), line.to_string());
    }
}

pub(super) fn finding_symbol(finding: &Finding) -> Option<&str> {
    for key in [
        "function",
        "symbol",
        "class",
        "method",
        "parameter",
        "receiver",
    ] {
        if let Some(value) = finding.identity.get(key) {
            return Some(value);
        }
        if let Some(EvidenceValue::String(value)) = finding.evidence.get(key) {
            return Some(value);
        }
    }
    None
}

#[derive(Clone, Copy)]
struct EvidenceSpan {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl EvidenceSpan {
    fn parse(value: &str) -> Option<Self> {
        let (start, end) = value.split_once('-')?;
        let (start_line, start_column) = start.split_once(':')?;
        let (end_line, end_column) = end.split_once(':')?;
        let span = Self {
            start_line: start_line.parse().ok()?,
            start_column: start_column.parse().ok()?,
            end_line: end_line.parse().ok()?,
            end_column: end_column.parse().ok()?,
        };
        (span.start_line > 0
            && span.start_column > 0
            && span.end_line > 0
            && span.end_column > 0
            && (span.start_line, span.start_column) <= (span.end_line, span.end_column))
            .then_some(span)
    }

    fn add_to(self, properties: &mut BTreeMap<String, String>) {
        properties.extend([
            ("end_column".to_string(), self.end_column.to_string()),
            ("end_line".to_string(), self.end_line.to_string()),
            ("span_precision".to_string(), "exact".to_string()),
            ("start_column".to_string(), self.start_column.to_string()),
            ("start_line".to_string(), self.start_line.to_string()),
        ]);
    }
}

fn exact_evidence_span(finding: &Finding) -> Option<EvidenceSpan> {
    finding.evidence.iter().find_map(|(key, value)| {
        if !key.ends_with("span") {
            return None;
        }
        let EvidenceValue::String(value) = value else {
            return None;
        };
        EvidenceSpan::parse(value)
    })
}

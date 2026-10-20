//! Canonical typed Markdown projection for `AuditReport`.
//!
//! Human CLI output, persisted report views, and Layer 6 model projections
//! share the renderer in `auditor_core::typed_markdown`. This module owns the
//! full-report document shape; consumers may select typed sections before
//! rendering without introducing another Markdown serializer.

use crate::AuditReport;
use auditor_core::{
    render_typed_markdown, EvidenceValue, Finding, MarkdownDocument, MarkdownField,
    MarkdownSection, MarkdownValue, Severity,
};
use std::collections::BTreeMap;

/// Retained for CLI compatibility. Canonical Markdown is deliberately
/// ANSI-free so persisted, human, and model-facing renders are byte-identical.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorMode {
    Auto,
    #[default]
    Plain,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HumanFooter {
    pub sibling_path: String,
    pub sibling_digest: String,
}

pub trait ReviewArtifactLoader {
    fn load(&self, artifact_path: &str) -> Option<ReviewDigestSummary>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReviewDigestSummary {
    pub review_id: String,
    pub material_recommendations: Vec<ReviewRecommendation>,
    pub contested_claims: Vec<ReviewContestedClaim>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReviewRecommendation {
    pub id: String,
    pub title: String,
    pub priority: String,
    pub rationale: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReviewContestedClaim {
    pub claim_id: String,
    pub objection_summary: String,
}

#[derive(Debug, Default)]
pub struct NoReviewArtifactLoader;

impl ReviewArtifactLoader for NoReviewArtifactLoader {
    fn load(&self, _artifact_path: &str) -> Option<ReviewDigestSummary> {
        None
    }
}

/// Build the canonical full-report document before rendering.
///
/// This typed boundary is reusable by case-scoped projections: selection
/// happens over sections and fields, while Markdown syntax remains owned by
/// one renderer.
pub fn report_markdown_document(
    report: &AuditReport,
    footer: &HumanFooter,
    summary_only: bool,
    review_loader: &dyn ReviewArtifactLoader,
) -> MarkdownDocument {
    let mut sections = vec![
        summary_section(report),
        coverage_section(report),
        severity_section(report),
        detector_section(report),
    ];
    if !summary_only {
        sections.push(findings_section(report));
        if let Some(section) = review_section(report, review_loader) {
            sections.push(section);
        }
        if !report.external_inputs.is_empty() {
            sections.push(external_inputs_section(report));
        }
    }
    sections.push(integrity_section(report, footer));
    MarkdownDocument::new("Engineering Auditor Report", sections)
}

pub fn report_to_human(
    report: &AuditReport,
    _color: ColorMode,
    footer: &HumanFooter,
    summary_only: bool,
    review_loader: &dyn ReviewArtifactLoader,
) -> String {
    render_typed_markdown(&report_markdown_document(
        report,
        footer,
        summary_only,
        review_loader,
    ))
    .expect("rendering Markdown into a String cannot fail")
}

pub fn report_to_human_plain(report: &AuditReport) -> String {
    report_to_human(
        report,
        ColorMode::Plain,
        &HumanFooter::default(),
        false,
        &NoReviewArtifactLoader,
    )
}

fn field(label: &str, value: impl ToString) -> MarkdownField {
    MarkdownField::new(label, MarkdownValue::text(value.to_string()))
}

fn optional_field(label: &str, value: Option<&str>) -> MarkdownField {
    field(label, value.unwrap_or_default())
}

fn summary_section(report: &AuditReport) -> MarkdownSection {
    MarkdownSection::new(
        "Summary",
        vec![
            field("Profile", &report.profile),
            field("Scope", &report.scope),
            field("Repository root", &report.repo_root),
            field("Analysis method", &report.analysis_method),
            field("Scanned files", report.scanned_files),
            MarkdownField::new(
                "Languages",
                MarkdownValue::list(
                    report
                        .languages
                        .iter()
                        .map(|(language, count)| format!("{language}: {count}")),
                ),
            ),
            MarkdownField::new(
                "Source roles",
                MarkdownValue::list(
                    report
                        .source_roles
                        .iter()
                        .map(|(role, count)| format!("{role}: {count}")),
                ),
            ),
            field("Detector contracts", report.detectors.len()),
            field("Capability executions", report.capability_executions.len()),
            field("Findings", report.findings.len()),
            field("Baseline compared", report.baseline.compared),
            optional_field("Baseline path", report.baseline.baseline_path.as_deref()),
            field("New findings", report.baseline.new_findings),
            field("Resolved findings", report.baseline.resolved_findings),
            field("Carried findings", report.baseline.carried.len()),
            field("Unchanged findings", report.baseline.unchanged_findings),
        ],
    )
}

fn coverage_section(report: &AuditReport) -> MarkdownSection {
    let coverage = &report.analysis_coverage;
    let mut fields = vec![field("Requested method", &coverage.requested_method)];
    fields.extend(number_fields(&[
        ("Files", coverage.files),
        ("Source files", coverage.source_files),
        (
            "Detector language claims",
            coverage.detector_language_claims,
        ),
        ("Findings", coverage.findings),
        ("Line-precise findings", coverage.line_precise_findings),
        ("Repository findings", coverage.repository_findings),
        ("Exact fact spans", coverage.exact_fact_spans),
        ("External inputs", coverage.external_inputs),
    ]));
    fields.extend([
        field("Evidence graph complete", coverage.graph_complete),
        MarkdownField::new(
            "Capability modes",
            MarkdownValue::fields(vec![
                field("Semantic", coverage.capability_modes.semantic),
                field("Exact lexical", coverage.capability_modes.exact_lexical),
                field("Partial", coverage.capability_modes.partial),
                field("Unsupported", coverage.capability_modes.unsupported),
                field("Not applicable", coverage.capability_modes.not_applicable),
            ]),
        ),
    ]);
    MarkdownSection::new("Analysis Coverage", fields)
}

fn number_fields(values: &[(&str, usize)]) -> Vec<MarkdownField> {
    values
        .iter()
        .map(|&(label, value)| field(label, value))
        .collect()
}

fn severity_section(report: &AuditReport) -> MarkdownSection {
    let mut counts = BTreeMap::from([
        (Severity::Critical, 0usize),
        (Severity::High, 0),
        (Severity::Medium, 0),
        (Severity::Low, 0),
        (Severity::Advisory, 0),
    ]);
    for finding in &report.findings {
        *counts.entry(finding.severity).or_default() += 1;
    }
    MarkdownSection::new(
        "Severity Totals",
        vec![
            field("Critical", counts[&Severity::Critical]),
            field("High", counts[&Severity::High]),
            field("Medium", counts[&Severity::Medium]),
            field("Low", counts[&Severity::Low]),
            field("Advisory", counts[&Severity::Advisory]),
            field("Total", report.findings.len()),
        ],
    )
}

fn detector_section(report: &AuditReport) -> MarkdownSection {
    let mut rows: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    for finding in &report.findings {
        let counts = rows.entry(finding.detector.clone()).or_default();
        counts[match finding.severity {
            Severity::Critical => 0,
            Severity::High => 1,
            Severity::Medium => 2,
            Severity::Low => 3,
            Severity::Advisory => 4,
        }] += 1;
    }
    let items: Vec<MarkdownValue> = rows
        .into_iter()
        .map(|(detector, counts)| {
            MarkdownValue::fields(vec![
                field("Detector", detector),
                field("Critical", counts[0]),
                field("High", counts[1]),
                field("Medium", counts[2]),
                field("Low", counts[3]),
                field("Advisory", counts[4]),
                field("Total", counts.iter().sum::<usize>()),
            ])
        })
        .collect();
    let detectors = if items.is_empty() {
        MarkdownValue::text("No detectors produced findings.")
    } else {
        MarkdownValue::Items(items)
    };
    MarkdownSection::new(
        "Per-Detector Breakdown",
        vec![MarkdownField::new("Detectors", detectors)],
    )
}

fn findings_section(report: &AuditReport) -> MarkdownSection {
    MarkdownSection::new(
        "Findings by Detector",
        vec![MarkdownField::new(
            "Findings",
            MarkdownValue::Items(report.findings.iter().map(finding_value).collect()),
        )],
    )
}

fn finding_value(finding: &Finding) -> MarkdownValue {
    let locations = finding
        .locations
        .iter()
        .map(|location| match location.line {
            Some(line) => format!("{}:{line}", location.path),
            None => location.path.clone(),
        });
    let evidence = finding
        .evidence
        .iter()
        .map(|(name, value)| MarkdownField::new(name, evidence_value(value)))
        .collect();
    let identity = finding
        .identity
        .iter()
        .map(|(name, value)| field(name, value))
        .collect();
    MarkdownValue::fields(vec![
        field("Finding ID", &finding.id),
        field("Detector", &finding.detector),
        field("Category", &finding.category),
        field("Severity", finding.severity.as_str()),
        field("Confidence", finding.confidence.as_str()),
        MarkdownField::new("Locations", MarkdownValue::list(locations)),
        field("Title", &finding.title),
        field("Message", &finding.message),
        field("Recommendation", &finding.recommendation),
        MarkdownField::new("Evidence", MarkdownValue::fields(evidence)),
        MarkdownField::new("Identity", MarkdownValue::fields(identity)),
        MarkdownField::new(
            "Provenance",
            MarkdownValue::fields(vec![
                field("Language", &finding.provenance.language),
                field("Source role", &finding.provenance.source_role),
                field("Analysis method", &finding.provenance.analysis_method),
                field("Extractor version", finding.provenance.extractor_version),
                field("Origin", &finding.provenance.origin),
                field("Span precision", &finding.provenance.span_precision),
            ]),
        ),
    ])
}

fn evidence_value(value: &EvidenceValue) -> MarkdownValue {
    match value {
        EvidenceValue::String(value) => MarkdownValue::text(value),
        EvidenceValue::Integer(value) => MarkdownValue::text(value.to_string()),
        EvidenceValue::Float(value) => MarkdownValue::text(value.to_string()),
        EvidenceValue::Bool(value) => MarkdownValue::text(value.to_string()),
        EvidenceValue::StringList(values) => MarkdownValue::list(values.clone()),
        EvidenceValue::IntegerList(values) => {
            MarkdownValue::list(values.iter().map(ToString::to_string))
        }
    }
}

fn review_section(
    report: &AuditReport,
    review_loader: &dyn ReviewArtifactLoader,
) -> Option<MarkdownSection> {
    let envelope = &report.engineering_review;
    if envelope.status == "absent" {
        return None;
    }
    let loaded = envelope
        .path
        .as_deref()
        .and_then(|path| review_loader.load(path));
    let recommendations = loaded.as_ref().map_or_else(Vec::new, |summary| {
        project_review_items(&summary.material_recommendations, |recommendation| {
            vec![
                field("Recommendation ID", &recommendation.id),
                field("Title", &recommendation.title),
                field("Priority", &recommendation.priority),
                field("Rationale", &recommendation.rationale),
            ]
        })
    });
    let contested = loaded.as_ref().map_or_else(Vec::new, |summary| {
        project_review_items(&summary.contested_claims, |claim| {
            vec![
                field("Claim ID", &claim.claim_id),
                field("Objection", &claim.objection_summary),
            ]
        })
    });
    let mut fields = review_envelope_fields(envelope);
    fields.push(field(
        "Artifact detail",
        if envelope.path.is_some() && loaded.is_none() {
            "Full recommendations remain in the sibling review artifact."
        } else {
            ""
        },
    ));
    fields.push(MarkdownField::new(
        "Material Recommendations",
        MarkdownValue::Items(recommendations),
    ));
    fields.push(MarkdownField::new(
        "Contested Claims",
        MarkdownValue::Items(contested),
    ));
    Some(MarkdownSection::new("Layer 6 Review", fields))
}

fn review_envelope_fields(envelope: &crate::EngineeringReviewEnvelope) -> Vec<MarkdownField> {
    let mut fields = vec![optional_field("Review ID", envelope.review_id.as_deref())];
    fields.extend(
        [
            ("Status", envelope.status.clone()),
            ("Completeness", envelope.completeness.clone()),
            ("Assessments", envelope.assessment_count.to_string()),
            (
                "Material recommendation count",
                envelope.material_recommendation_count.to_string(),
            ),
            ("Contested count", envelope.contested_count.to_string()),
            ("Revision count", envelope.revision_count.to_string()),
        ]
        .into_iter()
        .map(|(label, value)| field(label, value)),
    );
    fields.push(optional_field("Artifact path", envelope.path.as_deref()));
    fields
}

fn project_review_items<T>(
    items: &[T],
    fields: impl Fn(&T) -> Vec<MarkdownField>,
) -> Vec<MarkdownValue> {
    items
        .iter()
        .map(|item| MarkdownValue::fields(fields(item)))
        .collect()
}

fn external_inputs_section(report: &AuditReport) -> MarkdownSection {
    MarkdownSection::new(
        "External Inputs",
        vec![MarkdownField::new(
            "Inputs",
            MarkdownValue::Items(
                report
                    .external_inputs
                    .iter()
                    .map(|input| {
                        MarkdownValue::fields(vec![
                            field("Kind", &input.kind),
                            field("Source", &input.source),
                            field("Schema version", input.schema_version),
                            field("Digest", &input.digest),
                            field("Completeness", &input.completeness),
                            field("Contract relevant", input.contract_relevant),
                        ])
                    })
                    .collect(),
            ),
        )],
    )
}

fn integrity_section(report: &AuditReport, footer: &HumanFooter) -> MarkdownSection {
    MarkdownSection::new(
        "Integrity",
        vec![
            field("Machine sibling", &footer.sibling_path),
            field("Machine digest", &footer.sibling_digest),
            field("Analysis contract digest", &report.analysis_contract_digest),
            field("Report schema version", report.schema_version),
            field(
                "Semantic contract version",
                report.semantic_contract_version,
            ),
            field("Finding ID version", report.finding_id_version),
            field("Source role version", report.source_role_version),
            field("Repository map status", &report.repository_map.status),
            optional_field("Repository map path", report.repository_map.path.as_deref()),
            optional_field(
                "Repository map digest",
                report.repository_map.digest.as_deref(),
            ),
            optional_field(
                "Source snapshot digest",
                report.repository_map.source_snapshot_digest.as_deref(),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineeringReviewEnvelope, ENGINEERING_REVIEW_SCHEMA_VERSION};
    use auditor_core::Confidence;

    fn add_finding(report: &mut AuditReport) {
        report.findings.push(auditor_core::finding!(
            "detector",
            "structural",
            "Repeated work",
            "A complete message.",
            Severity::High,
            Confidence::High,
            "src/lib.rs",
            Some(4),
        ));
    }

    #[test]
    fn canonical_report_is_deterministic_complete_and_ansi_free() {
        let mut report = AuditReport::empty("gating", ".");
        add_finding(&mut report);
        report.finalize();
        let first = report_to_human_plain(&report);
        assert_eq!(first, report_to_human_plain(&report));
        assert!(first.contains("# Engineering Auditor Report"));
        assert!(first.contains("## Severity Totals"));
        assert!(first.contains("## Per-Detector Breakdown"));
        assert!(first.contains("## Findings by Detector"));
        assert!(first.contains("**Finding ID:**"));
        assert!(!first.contains('\x1b'));
        assert_eq!(
            first,
            report_to_human(
                &report,
                ColorMode::Auto,
                &HumanFooter::default(),
                false,
                &NoReviewArtifactLoader,
            )
        );
    }

    #[test]
    fn summary_projection_omits_detailed_sections() {
        let mut report = AuditReport::empty("full", "/repo");
        add_finding(&mut report);
        report.finalize();
        let markdown = report_to_human(
            &report,
            ColorMode::Plain,
            &HumanFooter::default(),
            true,
            &NoReviewArtifactLoader,
        );
        assert!(markdown.contains("## Severity Totals"));
        assert!(!markdown.contains("## Findings by Detector"));
    }

    #[test]
    fn review_section_uses_typed_nested_records() {
        let mut report = AuditReport::empty("full", "/repo");
        report.engineering_review = EngineeringReviewEnvelope {
            schema_version: ENGINEERING_REVIEW_SCHEMA_VERSION,
            status: "complete".to_string(),
            review_id: Some("review:abcd".to_string()),
            input_report_digest: Some("input".to_string()),
            path: Some("review.json".to_string()),
            digest: Some("digest".to_string()),
            completeness: "complete".to_string(),
            assessment_count: 4,
            material_recommendation_count: 1,
            contested_count: 1,
            revision_count: 0,
            case_count: 1,
            accepted_recommendation_count: 1,
            blocked_case_count: 0,
            contested_case_count: 1,
            challenge_response_count: 1,
        };
        struct Loader;
        impl ReviewArtifactLoader for Loader {
            fn load(&self, _: &str) -> Option<ReviewDigestSummary> {
                Some(ReviewDigestSummary {
                    review_id: "review:abcd".to_string(),
                    material_recommendations: vec![ReviewRecommendation {
                        id: "recommendation:1".to_string(),
                        title: "Move stable work".to_string(),
                        priority: "high".to_string(),
                        rationale: "The evidence supports the change.".to_string(),
                    }],
                    contested_claims: vec![ReviewContestedClaim {
                        claim_id: "claim:1".to_string(),
                        objection_summary: "Benefit was not measured.".to_string(),
                    }],
                })
            }
        }
        report.finalize();
        let markdown = report_to_human(
            &report,
            ColorMode::Plain,
            &HumanFooter::default(),
            false,
            &Loader,
        );
        assert!(markdown.contains("## Layer 6 Review"));
        assert!(markdown.contains("**Recommendation ID:** recommendation:1"));
        assert!(markdown.contains("**Claim ID:** claim:1"));
    }
}

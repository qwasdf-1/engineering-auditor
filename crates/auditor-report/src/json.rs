use auditor_core::{EvidenceValue, Finding};
use serde_json::{json, Map, Value};

use crate::{AuditReport, DetectorReport, EvidenceGraph, ExternalInput, FileReport};

pub(super) fn report_to_json(report: &AuditReport) -> String {
    serde_json::to_string_pretty(&report_value(report)).expect("report values are serializable")
}

fn report_value(report: &AuditReport) -> Value {
    json!({
        "analysis_contract_digest": report.analysis_contract_digest,
        "analysis_coverage": coverage_value(report),
        "analysis_method": report.analysis_method,
        "baseline": baseline_value(report),
        "capability_executions": report.capability_executions.iter().map(|execution| json!({
            "analyzed_files": execution.analyzed_files,
            "detector_id": execution.detector_id,
            "eligible_files": execution.eligible_files,
            "failure_reason": execution.failure_reason,
            "layer": execution.layer,
            "limitations": execution.limitations,
            "missing_inputs": execution.missing_inputs,
            "not_applicable_files": execution.not_applicable_files,
            "required_inputs": execution.required_inputs,
            "status": execution.status.as_str(),
            "unsupported_files": execution.unsupported_files,
        })).collect::<Vec<_>>(),
        "detectors": report.detectors.iter().map(detector_value).collect::<Vec<_>>(),
        "engineering_review": engineering_review_value(report),
        "evidence_graph": graph_value(&report.evidence_graph),
        "external_inputs": report.external_inputs.iter().map(external_input_value).collect::<Vec<_>>(),
        "files": report.files.iter().map(file_value).collect::<Vec<_>>(),
        "finding_groups": report.finding_groups.iter().map(|group| json!({
            "category": group.category,
            "count": group.count,
            "highest_severity": group.highest_severity.as_str(),
        })).collect::<Vec<_>>(),
        "finding_id_version": report.finding_id_version,
        "findings": report.findings.iter().map(finding_value).collect::<Vec<_>>(),
        "languages": report.languages.iter().map(|(language, count)| json!({
            "count": count,
            "language": language,
        })).collect::<Vec<_>>(),
        "profile": report.profile,
        "repo_root": report.repo_root,
        "repository_map": {
            "completeness": report.repository_map.completeness,
            "digest": report.repository_map.digest,
            "edge_count": report.repository_map.edge_count,
            "file_count": report.repository_map.file_count,
            "path": report.repository_map.path,
            "schema_version": report.repository_map.schema_version,
            "source_snapshot_digest": report.repository_map.source_snapshot_digest,
            "status": report.repository_map.status,
            "symbol_count": report.repository_map.symbol_count,
        },
        "scanned_files": report.scanned_files,
        "schema_version": report.schema_version,
        "scope": report.scope,
        "semantic_contract_version": report.semantic_contract_version,
        "source_role_version": report.source_role_version,
        "source_roles": report.source_roles.iter().map(|(role, count)| json!({
            "count": count,
            "role": role,
        })).collect::<Vec<_>>(),
    })
}

fn engineering_review_value(report: &AuditReport) -> Value {
    let review = &report.engineering_review;
    let mut value = json!({
        "completeness": review.completeness,
        "digest": review.digest,
        "input_report_digest": review.input_report_digest,
        "path": review.path,
        "review_id": review.review_id,
        "schema_version": review.schema_version,
        "status": review.status,
    });
    let object = value.as_object_mut().expect("review envelope is an object");
    let counts = if review.schema_version == crate::ENGINEERING_REVIEW_V2_SCHEMA_VERSION {
        vec![
            ("case_count", review.case_count),
            (
                "accepted_recommendation_count",
                review.accepted_recommendation_count,
            ),
            ("blocked_case_count", review.blocked_case_count),
            ("contested_case_count", review.contested_case_count),
            ("challenge_response_count", review.challenge_response_count),
        ]
    } else {
        vec![
            ("assessment_count", review.assessment_count),
            (
                "material_recommendation_count",
                review.material_recommendation_count,
            ),
            ("contested_count", review.contested_count),
            ("revision_count", review.revision_count),
        ]
    };
    for (field, count) in counts {
        object.insert(field.to_string(), count.into());
    }
    value
}

fn coverage_value(report: &AuditReport) -> Value {
    let coverage = &report.analysis_coverage;
    let modes = &coverage.capability_modes;
    let capability_modes = [
        ("exact_lexical", modes.exact_lexical),
        ("not_applicable", modes.not_applicable),
        ("partial", modes.partial),
        ("semantic", modes.semantic),
        ("unsupported", modes.unsupported),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), json!(value)))
    .collect::<Map<_, _>>();
    let mut value = json!({
        "capability_modes": capability_modes,
        "detector_language_claims": coverage.detector_language_claims,
        "exact_fact_spans": coverage.exact_fact_spans,
        "external_inputs": coverage.external_inputs,
        "files": coverage.files,
        "findings": coverage.findings,
        "graph_complete": coverage.graph_complete,
        "line_precise_findings": coverage.line_precise_findings,
        "repository_findings": coverage.repository_findings,
        "requested_method": coverage.requested_method,
        "source_files": coverage.source_files,
    });
    for (key, count) in [
        ("syntax_recovered_files", coverage.syntax_recovered_files),
        ("syntax_unparsed_files", coverage.syntax_unparsed_files),
    ] {
        if count > 0 {
            value[key] = json!(count);
        }
    }
    value
}

fn baseline_value(report: &AuditReport) -> Value {
    json!({
        "baseline_path": report.baseline.baseline_path,
        "carried": report
            .baseline
            .carried
            .iter()
            .map(|carried| json!({
                "baseline_id": carried.baseline_id,
                "current_id": carried.current_id,
                "pass": carried.pass.as_str(),
            }))
            .collect::<Vec<_>>(),
        "carried_findings": report.baseline.carried.len(),
        "compared": report.baseline.compared,
        "new_findings": report.baseline.new_findings,
        "resolved_findings": report.baseline.resolved_findings,
        "schema_version": report.baseline.schema_version,
        "unchanged_findings": report.baseline.unchanged_findings,
    })
}

fn detector_value(detector: &DetectorReport) -> Value {
    json!({
        "category": detector.category,
        "context_roles": detector.context_roles,
        "default_severity": detector.default_severity.as_str(),
        "emission_roles": detector.emission_roles,
        "finding_count": detector.finding_count,
        "id": detector.id,
        "language_support": detector.language_support.iter().map(|claim| json!({
            "language": claim.language,
            "limitation": claim.limitation,
            "mode": claim.mode,
        })).collect::<Vec<_>>(),
        "layer": detector.layer,
        "maturity": detector.maturity,
        "profile": detector.profile,
    })
}

fn file_value(file: &FileReport) -> Value {
    let mut value = json!({
        "analysis_method": file.analysis_method,
        "bytes": file.bytes,
        "catalog_language": file.catalog_language,
        "extractor_version": file.extractor_version,
        "language": file.language,
        "path": file.path,
        "source_role": file.source_role,
    });
    if let Some(syntax) = &file.syntax {
        value["syntax"] = json!(syntax);
    }
    value
}

fn external_input_value(input: &ExternalInput) -> Value {
    json!({
        "completeness": input.completeness,
        "contract_relevant": input.contract_relevant,
        "digest": input.digest,
        "kind": input.kind,
        "schema_version": input.schema_version,
        "source": input.source,
    })
}

fn finding_value(finding: &Finding) -> Value {
    let evidence = finding
        .evidence
        .iter()
        .map(|(key, value)| (key.clone(), evidence_value(value)))
        .collect::<Map<_, _>>();
    json!({
        "category": finding.category,
        "confidence": finding.confidence.as_str(),
        "detector": finding.detector,
        "evidence": evidence,
        "evidence_keys": finding.evidence_keys(),
        "id": finding.id,
        "identity": finding.identity,
        "locations": finding.locations.iter().map(|location| {
            let mut value = json!({"path": location.path});
            if let Some(line) = location.line {
                value["line"] = json!(line);
            }
            value
        }).collect::<Vec<_>>(),
        "message": finding.message,
        "provenance": {
            "analysis_method": finding.provenance.analysis_method,
            "extractor_version": finding.provenance.extractor_version,
            "language": finding.provenance.language,
            "origin": finding.provenance.origin,
            "source_role": finding.provenance.source_role,
            "span_precision": finding.provenance.span_precision,
        },
        "recommendation": finding.recommendation,
        "severity": finding.severity.as_str(),
        "title": finding.title,
    })
}

fn evidence_value(value: &EvidenceValue) -> Value {
    match value {
        EvidenceValue::String(value) => json!(value),
        EvidenceValue::Integer(value) => json!(value),
        EvidenceValue::Float(value) if value.is_finite() => json!(value),
        EvidenceValue::Float(_) => json!(0.0),
        EvidenceValue::Bool(value) => json!(value),
        EvidenceValue::StringList(value) => json!(value),
        EvidenceValue::IntegerList(value) => json!(value),
    }
}

fn graph_value(graph: &EvidenceGraph) -> Value {
    json!({
        "completeness": {
            "complete": graph.completeness.complete,
            "exact_fact_spans": graph.completeness.exact_fact_spans,
            "external_inputs": graph.completeness.external_inputs,
            "files": graph.completeness.files,
            "findings": graph.completeness.findings,
            "produced_findings": graph.completeness.produced_findings,
            "supported_findings": graph.completeness.supported_findings,
        },
        "edges": graph.edges.iter().map(|edge| json!({
            "from": edge.from,
            "kind": edge.kind.as_str(),
            "properties": edge.properties,
            "to": edge.to,
        })).collect::<Vec<_>>(),
        "nodes": graph.nodes.iter().map(|node| json!({
            "id": node.id,
            "kind": node.kind.as_str(),
            "label": node.label,
            "properties": node.properties,
        })).collect::<Vec<_>>(),
        "schema_version": graph.schema_version,
    })
}

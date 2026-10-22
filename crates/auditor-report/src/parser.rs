use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::graph_validation;

#[path = "parser_validation.rs"]
mod validation;
use crate::{
    BASELINE_SCHEMA_VERSION, ENGINEERING_REVIEW_SCHEMA_VERSION, EVIDENCE_GRAPH_SCHEMA_VERSION,
    REPORT_SCHEMA_VERSION,
};
use validation::{
    validate_artifact_envelopes, validate_baseline_diff, validate_capability_executions,
    validate_coverage_and_contracts, validate_finding_provenance,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedFinding {
    pub id: String,
    pub detector: String,
    pub category: String,
    pub title: String,
    pub message: String,
    pub recommendation: String,
    pub severity: String,
    pub confidence: String,
    pub path: String,
    pub line: Option<usize>,
    pub identity: BTreeMap<String, String>,
}

impl ParsedFinding {
    pub fn match_subject(&self) -> crate::MatchSubject<'_> {
        crate::MatchSubject {
            id: &self.id,
            detector: &self.detector,
            title: &self.title,
            path: &self.path,
            message: &self.message,
            identity: &self.identity,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportIdentity {
    pub schema_version: u32,
    pub semantic_contract_version: u32,
    pub evidence_graph_schema_version: u32,
    pub finding_id_version: u32,
    pub source_role_version: u32,
    pub profile: String,
    pub scope: String,
    pub analysis_method: String,
    pub analysis_contract_digest: String,
}

pub fn parse_report_identity(report_json: &str) -> Result<ReportIdentity, String> {
    let value = parse_value(report_json)?;
    let object = required_object(&value, "report")?;
    let graph = required_object_field(object, "evidence_graph")?;
    let identity = ReportIdentity {
        schema_version: required_u32(object, "schema_version")?,
        semantic_contract_version: required_u32(object, "semantic_contract_version")?,
        evidence_graph_schema_version: required_u32(graph, "schema_version")?,
        finding_id_version: required_u32(object, "finding_id_version")?,
        source_role_version: required_u32(object, "source_role_version")?,
        profile: required_string(object, "profile")?,
        scope: required_string(object, "scope")?,
        analysis_method: required_string(object, "analysis_method")?,
        analysis_contract_digest: required_string(object, "analysis_contract_digest")?,
    };
    validate_identity(&identity)?;
    Ok(identity)
}

pub fn parse_report_findings(report_json: &str) -> Result<Vec<ParsedFinding>, String> {
    let value = parse_value(report_json)?;
    let object = required_object(&value, "report")?;
    let findings = object
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires findings array".to_string())?;
    findings.iter().map(parse_finding).collect()
}

pub fn parse_repository_map_reference(
    report_json: &str,
) -> Result<crate::RepositoryMapReference, String> {
    let value = parse_value(report_json)?;
    let object = required_object(&value, "report")?;
    let map = required_object_field(object, "repository_map")?;
    Ok(crate::RepositoryMapReference {
        schema_version: required_u32(map, "schema_version")?,
        status: required_string(map, "status")?,
        path: optional_string(map, "path")?,
        digest: optional_string(map, "digest")?,
        source_snapshot_digest: optional_string(map, "source_snapshot_digest")?,
        completeness: required_string(map, "completeness")?,
        file_count: required_usize(map, "file_count")?,
        symbol_count: required_usize(map, "symbol_count")?,
        edge_count: required_usize(map, "edge_count")?,
    })
}

pub fn extract_finding_ids(report_json: &str) -> BTreeSet<String> {
    parse_report_findings(report_json)
        .map(|findings| findings.into_iter().map(|finding| finding.id).collect())
        .unwrap_or_default()
}

pub fn validate_report_json(report_json: &str) -> Result<(), String> {
    let identity = parse_report_identity(report_json)?;
    let value = parse_value(report_json)?;
    let object = required_object(&value, "report")?;
    validate_baseline_diff(object)?;
    validate_coverage_and_contracts(object)?;
    validate_capability_executions(object)?;
    validate_artifact_envelopes(object)?;
    let findings = parse_report_findings(report_json)?;
    validate_finding_provenance(object)?;
    graph_validation::validate_graph(object, &findings)?;
    if identity.analysis_contract_digest.is_empty() {
        return Err("report analysis contract digest must not be empty".to_string());
    }
    Ok(())
}

/// Reconstruct every [`AuditReport`] field used by the canonical typed
/// Markdown renderer from a validated saved report.
///
/// This is the entry point for `auditor digest --report <machine.json>` and
/// for `scan --review --format human` rendering the digest of a reviewed
/// report. The full `to_json` serializer remains the canonical byte
/// contract; this parser is a renderer adapter, not a contract peer, and
/// does not reconstruct the full evidence graph or file catalog.
pub fn parse_report_for_digest(report_json: &str) -> Result<crate::AuditReport, String> {
    let value = parse_value(report_json)?;
    let object = required_object(&value, "report")?;
    let identity = parse_report_identity(report_json)?;

    let mut report = crate::AuditReport::empty(
        identity.profile.clone(),
        required_string(object, "repo_root")?,
    );
    report.schema_version = identity.schema_version;
    report.semantic_contract_version = identity.semantic_contract_version;
    report.finding_id_version = identity.finding_id_version;
    report.source_role_version = identity.source_role_version;
    report.analysis_contract_digest = identity.analysis_contract_digest;
    report.scope = identity.scope;
    report.profile = identity.profile;
    repo_root_into(&mut report.repo_root, object);
    report.analysis_method = identity.analysis_method;
    report.scanned_files = object
        .get("scanned_files")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    report.languages = parse_language_counts(object)?;
    report.source_roles = parse_count_pairs(object, "source_roles", "role")?;
    // Detectors and capability_executions do not need their full body for the
    // renderer; only the counts mater (render_footer/summary_header use
    // `.len()`). Stub them as empty so the JSON-shape cost stays minimal.
    let detector_count = object
        .get("detectors")
        .and_then(Value::as_array)
        .map(|array| array.len())
        .unwrap_or(0);
    let capability_count = object
        .get("capability_executions")
        .and_then(Value::as_array)
        .map(|array| array.len())
        .unwrap_or(0);
    report.detectors = (0..detector_count)
        .map(|_| empty_detector_report())
        .collect();
    report.capability_executions = (0..capability_count)
        .map(|_| auditor_core::CapabilityExecution {
            detector_id: String::new(),
            layer: String::new(),
            status: auditor_core::CapabilityExecutionStatus::NotApplicable,
            eligible_files: 0,
            analyzed_files: 0,
            unsupported_files: 0,
            not_applicable_files: 0,
            required_inputs: Vec::new(),
            missing_inputs: Vec::new(),
            limitations: Vec::new(),
            failure_reason: None,
        })
        .collect();
    report.engineering_review = parse_engineering_review_envelope(object)?;
    report.repository_map = parse_repository_map_reference(report_json)?;
    report.analysis_coverage = parse_analysis_coverage(object)?;
    report.external_inputs = parse_external_inputs(object)?;
    report.baseline = parse_baseline_diff(object);
    report.findings = parse_findings_for_digest(object)?;
    report.suppressed_findings = parse_suppressed_findings(object);
    Ok(report)
}

/// The optional `suppressed_findings` list; absent when nothing was
/// suppressed.
fn parse_suppressed_findings(object: &Map<String, Value>) -> Vec<auditor_core::SuppressedFinding> {
    let text = |entry: &Value, key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    object
        .get("suppressed_findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| auditor_core::SuppressedFinding {
            id: text(entry, "id"),
            detector: text(entry, "detector"),
            path: text(entry, "path"),
            line: entry.get("line").and_then(Value::as_u64).unwrap_or(0) as usize,
            reason: text(entry, "reason"),
        })
        .collect()
}

fn repo_root_into(slot: &mut String, object: &Map<String, Value>) {
    if let Some(root) = object.get("repo_root").and_then(Value::as_str) {
        *slot = root.to_string();
    }
}

fn parse_language_counts(object: &Map<String, Value>) -> Result<Vec<(String, usize)>, String> {
    parse_count_pairs(object, "languages", "language")
}

fn parse_count_pairs(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Vec<(String, usize)>, String> {
    let Some(array) = object.get(field).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        let entry = required_object(entry, &format!("{field} entry"))?;
        let label = required_string(entry, label)?;
        let count = entry
            .get("count")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{field} entry requires integer count"))?;
        out.push((label, count));
    }
    Ok(out)
}

fn parse_analysis_coverage(object: &Map<String, Value>) -> Result<crate::AnalysisCoverage, String> {
    let coverage = required_object_field(object, "analysis_coverage")?;
    let modes = required_object_field(coverage, "capability_modes")?;
    Ok(crate::AnalysisCoverage {
        requested_method: required_string(coverage, "requested_method")?,
        files: required_usize(coverage, "files")?,
        source_files: required_usize(coverage, "source_files")?,
        detector_language_claims: required_usize(coverage, "detector_language_claims")?,
        capability_modes: crate::CapabilityCoverage {
            semantic: required_usize(modes, "semantic")?,
            exact_lexical: required_usize(modes, "exact_lexical")?,
            partial: required_usize(modes, "partial")?,
            unsupported: required_usize(modes, "unsupported")?,
            not_applicable: required_usize(modes, "not_applicable")?,
        },
        findings: required_usize(coverage, "findings")?,
        line_precise_findings: required_usize(coverage, "line_precise_findings")?,
        repository_findings: required_usize(coverage, "repository_findings")?,
        exact_fact_spans: required_usize(coverage, "exact_fact_spans")?,
        external_inputs: required_usize(coverage, "external_inputs")?,
        graph_complete: coverage
            .get("graph_complete")
            .and_then(Value::as_bool)
            .ok_or_else(|| "analysis_coverage requires boolean graph_complete".to_string())?,
        syntax_recovered_files: optional_count(coverage, "syntax_recovered_files"),
        syntax_unparsed_files: optional_count(coverage, "syntax_unparsed_files"),
        suppressed_findings: optional_count(coverage, "suppressed_findings"),
    })
}

fn optional_count(object: &Map<String, Value>, key: &str) -> usize {
    object
        .get(key)
        .and_then(Value::as_u64)
        .map_or(0, |count| count as usize)
}

fn parse_external_inputs(object: &Map<String, Value>) -> Result<Vec<crate::ExternalInput>, String> {
    let inputs = object
        .get("external_inputs")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires external_inputs array".to_string())?;
    inputs
        .iter()
        .map(|value| {
            let input = required_object(value, "external input")?;
            Ok(crate::ExternalInput {
                kind: required_string(input, "kind")?,
                source: required_string(input, "source")?,
                schema_version: required_u32(input, "schema_version")?,
                digest: required_string(input, "digest")?,
                completeness: required_string(input, "completeness")?,
                contract_relevant: input
                    .get("contract_relevant")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| {
                        "external input requires boolean contract_relevant".to_string()
                    })?,
            })
        })
        .collect()
}

fn parse_engineering_review_envelope(
    object: &Map<String, Value>,
) -> Result<crate::EngineeringReviewEnvelope, String> {
    let envelope = required_object_field(object, "engineering_review")?;
    let optional_nullable =
        |envelope: &Map<String, Value>, field: &str| -> Result<Option<String>, String> {
            match envelope.get(field) {
                Some(Value::Null) => Ok(None),
                Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
                None => Ok(None),
                Some(_) => Err(format!(
                    "engineering_review requires nonempty string or null {field}"
                )),
            }
        };
    Ok(crate::EngineeringReviewEnvelope {
        schema_version: envelope
            .get("schema_version")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(ENGINEERING_REVIEW_SCHEMA_VERSION),
        status: required_string(envelope, "status")?,
        review_id: optional_nullable(envelope, "review_id")?,
        input_report_digest: optional_nullable(envelope, "input_report_digest")?,
        path: optional_nullable(envelope, "path")?,
        digest: optional_nullable(envelope, "digest")?,
        completeness: envelope
            .get("completeness")
            .and_then(Value::as_str)
            .unwrap_or("not_requested")
            .to_string(),
        assessment_count: envelope
            .get("assessment_count")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        material_recommendation_count: envelope
            .get("material_recommendation_count")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        contested_count: envelope
            .get("contested_count")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        revision_count: envelope
            .get("revision_count")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        case_count: optional_usize(envelope, "case_count"),
        accepted_recommendation_count: optional_usize(envelope, "accepted_recommendation_count"),
        blocked_case_count: optional_usize(envelope, "blocked_case_count"),
        contested_case_count: optional_usize(envelope, "contested_case_count"),
        challenge_response_count: optional_usize(envelope, "challenge_response_count"),
    })
}

fn optional_usize(object: &Map<String, Value>, field: &str) -> usize {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn parse_baseline_diff(object: &Map<String, Value>) -> crate::BaselineDiff {
    let Some(envelope) = object.get("baseline").and_then(Value::as_object) else {
        return crate::BaselineDiff::none();
    };
    crate::BaselineDiff {
        schema_version: envelope
            .get("schema_version")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(BASELINE_SCHEMA_VERSION),
        baseline_path: envelope
            .get("baseline_path")
            .and_then(Value::as_str)
            .map(str::to_string),
        compared: envelope
            .get("compared")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        new_findings: envelope
            .get("new_findings")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        resolved_findings: envelope
            .get("resolved_findings")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
        carried: envelope
            .get("carried")
            .and_then(Value::as_array)
            .map(|carried| carried.iter().filter_map(parse_carried).collect())
            .unwrap_or_default(),
        unchanged_findings: envelope
            .get("unchanged_findings")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0),
    }
}

fn parse_findings_for_digest(
    object: &Map<String, Value>,
) -> Result<Vec<auditor_core::Finding>, String> {
    let Some(array) = object.get("findings").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    array.iter().map(parse_finding_for_digest).collect()
}

fn parse_finding_for_digest(value: &Value) -> Result<auditor_core::Finding, String> {
    use auditor_core::{Finding, FindingProvenance};
    let object = required_object(value, "finding")?;
    let locations = parse_finding_locations(object)?;
    let detector = required_string(object, "detector")?;
    let category = required_string(object, "category")?;
    let title = required_string(object, "title")?;
    let message = required_string(object, "message")?;
    let recommendation = required_string(object, "recommendation")?;
    let severity = parse_finding_severity(&required_string(object, "severity")?)?;
    let confidence = parse_finding_confidence(&required_string(object, "confidence")?)?;
    let id = required_string(object, "id")?;
    let evidence = required_object_field(object, "evidence")?
        .iter()
        .map(|(key, value)| Ok((key.clone(), parse_evidence_value(value)?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let identity = parse_identity(object)?;
    let provenance = required_object_field(object, "provenance")?;
    Ok(Finding {
        id,
        detector,
        category,
        title,
        message,
        recommendation,
        severity,
        confidence,
        locations,
        evidence,
        identity,
        provenance: FindingProvenance {
            language: required_string(provenance, "language")?,
            source_role: required_string(provenance, "source_role")?,
            analysis_method: required_string(provenance, "analysis_method")?,
            extractor_version: required_u32(provenance, "extractor_version")?,
            origin: required_string(provenance, "origin")?,
            span_precision: required_string(provenance, "span_precision")?,
        },
    })
}

fn parse_finding_locations(
    object: &Map<String, Value>,
) -> Result<Vec<auditor_core::Location>, String> {
    let locations = object
        .get("locations")
        .and_then(Value::as_array)
        .ok_or_else(|| "finding requires locations array".to_string())?
        .iter()
        .map(|location| {
            let location = required_object(location, "finding location")?;
            Ok(auditor_core::Location {
                path: required_string(location, "path")?,
                line: location
                    .get("line")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if locations.is_empty() {
        return Err("finding requires at least one location".to_string());
    }
    Ok(locations)
}

fn parse_finding_severity(value: &str) -> Result<auditor_core::Severity, String> {
    Ok(match value {
        "advisory" => auditor_core::Severity::Advisory,
        "low" => auditor_core::Severity::Low,
        "medium" => auditor_core::Severity::Medium,
        "high" => auditor_core::Severity::High,
        "critical" => auditor_core::Severity::Critical,
        other => return Err(format!("unknown severity `{other}`")),
    })
}

fn parse_finding_confidence(value: &str) -> Result<auditor_core::Confidence, String> {
    Ok(match value {
        "low" => auditor_core::Confidence::Low,
        "medium" => auditor_core::Confidence::Medium,
        "high" => auditor_core::Confidence::High,
        other => return Err(format!("unknown confidence `{other}`")),
    })
}

fn parse_evidence_value(value: &Value) -> Result<auditor_core::EvidenceValue, String> {
    use auditor_core::EvidenceValue;
    match value {
        Value::String(value) => Ok(EvidenceValue::String(value.clone())),
        Value::Number(value) => value
            .as_i64()
            .map(EvidenceValue::Integer)
            .or_else(|| value.as_f64().map(EvidenceValue::Float))
            .ok_or_else(|| "finding evidence number is outside supported range".to_string()),
        Value::Bool(value) => Ok(EvidenceValue::Bool(*value)),
        Value::Array(values) if values.is_empty() => Ok(EvidenceValue::StringList(Vec::new())),
        Value::Array(values) if values.iter().all(Value::is_string) => {
            Ok(EvidenceValue::StringList(
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
            ))
        }
        Value::Array(values) if values.iter().all(Value::is_i64) => Ok(EvidenceValue::IntegerList(
            values.iter().filter_map(Value::as_i64).collect(),
        )),
        _ => Err("finding evidence must be a scalar or homogeneous list".to_string()),
    }
}

fn empty_detector_report() -> crate::DetectorReport {
    use auditor_core::DetectorContract;
    DetectorContract {
        id: String::new(),
        category: String::new(),
        default_severity: auditor_core::Severity::Advisory,
        profile: String::new(),
        maturity: String::new(),
        layer: String::new(),
        emission_roles: Vec::new(),
        context_roles: Vec::new(),
        language_support: Vec::new(),
        finding_count: 0,
    }
}

fn parse_finding(value: &Value) -> Result<ParsedFinding, String> {
    let object = required_object(value, "finding")?;
    let locations = object
        .get("locations")
        .and_then(Value::as_array)
        .ok_or_else(|| "finding requires locations array".to_string())?;
    let location = locations
        .first()
        .ok_or_else(|| "finding requires at least one location".to_string())?;
    let location = required_object(location, "finding location")?;
    Ok(ParsedFinding {
        id: required_string(object, "id")?,
        detector: required_string(object, "detector")?,
        category: required_string(object, "category")?,
        title: required_string(object, "title")?,
        message: required_string(object, "message")?,
        recommendation: required_string(object, "recommendation")?,
        severity: required_string(object, "severity")?,
        confidence: required_string(object, "confidence")?,
        path: required_string(location, "path")?,
        line: location
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok()),
        identity: parse_identity(object)?,
    })
}

fn parse_identity(object: &Map<String, Value>) -> Result<BTreeMap<String, String>, String> {
    string_map(object.get("identity"), "finding identity")
}

/// A JSON object whose values are all strings, such as a finding identity.
pub(crate) fn string_map(
    value: Option<&Value>,
    field: &str,
) -> Result<BTreeMap<String, String>, String> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{field} must be an object"))?
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| format!("{field} `{key}` must be a string"))
        })
        .collect()
}

fn validate_identity(identity: &ReportIdentity) -> Result<(), String> {
    if identity.schema_version != REPORT_SCHEMA_VERSION
        || identity.semantic_contract_version != auditor_core::SEMANTIC_CONTRACT_VERSION
        || identity.evidence_graph_schema_version != EVIDENCE_GRAPH_SCHEMA_VERSION
        || identity.finding_id_version != auditor_core::FINDING_ID_VERSION
        || identity.source_role_version == 0
        || !matches!(identity.profile.as_str(), "gating" | "full")
        || !matches!(identity.scope.as_str(), "full" | "changed")
        || !matches!(identity.analysis_method.as_str(), "auto" | "ast" | "token")
        || identity.analysis_contract_digest.is_empty()
    {
        return Err(format!(
            "unsupported report identity: schema={}, semantic_contract={}, graph_schema={}, finding_id={}, profile={}, scope={}, method={}",
            identity.schema_version,
            identity.semantic_contract_version,
            identity.evidence_graph_schema_version,
            identity.finding_id_version,
            identity.profile,
            identity.scope,
            identity.analysis_method,
        ));
    }
    Ok(())
}

fn parse_carried(value: &Value) -> Option<crate::CarriedFinding> {
    Some(crate::CarriedFinding {
        baseline_id: value.get("baseline_id")?.as_str()?.to_string(),
        current_id: value.get("current_id")?.as_str()?.to_string(),
        pass: crate::CarryPass::parse(value.get("pass")?.as_str()?)?,
    })
}

fn parse_value(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("invalid report JSON: {error}"))
}

pub(super) fn required_object<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

pub(super) fn required_object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, String> {
    object
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("report requires {field} object"))
}

pub(super) fn required_string(object: &Map<String, Value>, field: &str) -> Result<String, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("object requires nonempty {field}"))
}

fn required_u32(object: &Map<String, Value>, field: &str) -> Result<u32, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("object requires integer {field}"))
}

fn required_usize(object: &Map<String, Value>, field: &str) -> Result<usize, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("object requires nonnegative integer {field}"))
}

fn optional_string(object: &Map<String, Value>, field: &str) -> Result<Option<String>, String> {
    let Some(raw) = object.get(field) else {
        return Err(format!("object requires nonempty string or null {field}"));
    };
    let parsed = serde_json::from_value::<Option<String>>(raw.clone())
        .map_err(|_| format!("object requires nonempty string or null {field}"))?;
    if parsed.as_deref().is_some_and(str::is_empty) {
        return Err(format!("object requires nonempty string or null {field}"));
    }
    Ok(parsed)
}

fn string_array(object: &Map<String, Value>, field: &str) -> Result<Vec<String>, String> {
    let values = object
        .get(field)
        .cloned()
        .ok_or_else(|| format!("object requires {field} array"))?;
    let values = serde_json::from_value::<Vec<String>>(values)
        .map_err(|_| format!("object requires {field} array"))?;
    if values.iter().any(String::is_empty) {
        return Err(format!("object requires nonempty strings in {field}"));
    }
    Ok(values)
}

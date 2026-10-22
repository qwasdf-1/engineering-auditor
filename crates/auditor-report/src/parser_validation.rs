//! Structural checks of a whole report: its baseline diff, coverage and
//! detector contracts, capability executions, artifact envelopes, and
//! finding provenance.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::{
    optional_string, required_object, required_object_field, required_string, required_u32,
    required_usize, string_array,
};
use crate::{
    BASELINE_SCHEMA_VERSION, ENGINEERING_REVIEW_SCHEMA_VERSION, REPOSITORY_MAP_SCHEMA_VERSION,
};

pub(super) fn validate_baseline_diff(report: &Map<String, Value>) -> Result<(), String> {
    let baseline = required_object_field(report, "baseline")?;
    if required_u32(baseline, "schema_version")? != BASELINE_SCHEMA_VERSION {
        return Err("unsupported report baseline diff schema".to_string());
    }
    Ok(())
}

pub(super) fn validate_coverage_and_contracts(report: &Map<String, Value>) -> Result<(), String> {
    required_object_field(report, "analysis_coverage")?;
    report
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires files array".to_string())?;
    let detectors = report
        .get("detectors")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires detectors array".to_string())?;
    for detector in detectors {
        let detector = required_object(detector, "detector contract")?;
        for field in ["emission_roles", "context_roles", "language_support"] {
            detector
                .get(field)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("detector contract requires {field} array"))?;
        }
    }
    Ok(())
}

pub(super) fn validate_capability_executions(report: &Map<String, Value>) -> Result<(), String> {
    let detectors = report
        .get("detectors")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires detectors array".to_string())?;
    let detector_ids = detectors
        .iter()
        .map(|detector| required_string(required_object(detector, "detector contract")?, "id"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let detector_layers = detectors
        .iter()
        .map(|detector| {
            let detector = required_object(detector, "detector contract")?;
            Ok((
                required_string(detector, "id")?,
                required_string(detector, "layer")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    if detector_ids.len() != detectors.len() {
        return Err("report detector ids must be unique".to_string());
    }
    let executions = report
        .get("capability_executions")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires capability_executions array".to_string())?;
    let mut execution_ids = BTreeSet::new();
    let mut executed_ids = BTreeSet::new();
    for execution in executions {
        if let Some(detector_id) = validate_capability_execution(
            execution,
            &detector_ids,
            &detector_layers,
            &mut execution_ids,
        )? {
            executed_ids.insert(detector_id);
        }
    }
    if execution_ids != detector_ids {
        return Err("report requires exactly one capability execution per detector".to_string());
    }
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires findings array".to_string())?;
    for finding in findings {
        let detector = required_string(required_object(finding, "finding")?, "detector")?;
        if !executed_ids.contains(&detector) {
            return Err(format!(
                "finding references a detector that was not executed: {detector}"
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ExecutionCounts {
    eligible: usize,
    analyzed: usize,
    unsupported: usize,
    not_applicable: usize,
}

impl ExecutionCounts {
    fn parse(execution: &Map<String, Value>) -> Result<Self, String> {
        Ok(Self {
            eligible: required_usize(execution, "eligible_files")?,
            analyzed: required_usize(execution, "analyzed_files")?,
            unsupported: required_usize(execution, "unsupported_files")?,
            not_applicable: required_usize(execution, "not_applicable_files")?,
        })
    }

    fn balances(self) -> bool {
        self.analyzed + self.unsupported + self.not_applicable == self.eligible
    }
}

fn validate_capability_execution(
    value: &Value,
    detector_ids: &BTreeSet<String>,
    detector_layers: &BTreeMap<String, String>,
    execution_ids: &mut BTreeSet<String>,
) -> Result<Option<String>, String> {
    let execution = required_object(value, "capability execution")?;
    let detector_id = required_string(execution, "detector_id")?;
    if !detector_ids.contains(&detector_id) || !execution_ids.insert(detector_id.clone()) {
        return Err(format!(
            "capability execution must identify one unique registered detector: {detector_id}"
        ));
    }
    if detector_layers.get(&detector_id) != Some(&required_string(execution, "layer")?) {
        return Err(format!(
            "capability execution layer does not match detector contract for {detector_id}"
        ));
    }
    let counts = ExecutionCounts::parse(execution)?;
    if !counts.balances() {
        return Err(format!(
            "capability execution file counts do not balance for {detector_id}"
        ));
    }
    let required_inputs = string_array(execution, "required_inputs")?;
    let missing_inputs = string_array(execution, "missing_inputs")?;
    string_array(execution, "limitations")?;
    if missing_inputs
        .iter()
        .any(|input| !required_inputs.contains(input))
    {
        return Err(format!(
            "capability execution has an undeclared missing input for {detector_id}"
        ));
    }
    let status = required_string(execution, "status")?;
    let failure_reason = optional_string(execution, "failure_reason")?;
    validate_execution_status(
        &detector_id,
        &status,
        counts,
        &missing_inputs,
        &failure_reason,
    )
    .map(|executed| executed.then_some(detector_id))
}

fn validate_execution_status(
    detector_id: &str,
    status: &str,
    counts: ExecutionCounts,
    missing_inputs: &[String],
    failure_reason: &Option<String>,
) -> Result<bool, String> {
    match status {
        "executed"
            if counts.analyzed > 0 && missing_inputs.is_empty() && failure_reason.is_none() =>
        {
            Ok(true)
        }
        // Repository-metadata detectors emit findings by walking the
        // repository-wide file set, not by per-language semantics. Their
        // language policy is `NotApplicable` for every catalog language, so
        // `analyzed` is always zero while `not_applicable` carries the
        // eligible file count. Treat such a detector as `executed` when it
        // observed any eligible file, so its findings survive the
        // report-integrity check. The detector is verified to be a
        // repository-metadata owner by the upstream `capability_executions`
        // builder, which is the only path that produces this shape.
        "executed"
            if counts.analyzed == 0
                && counts.not_applicable > 0
                && counts.unsupported == 0
                && missing_inputs.is_empty()
                && failure_reason.is_none() =>
        {
            Ok(true)
        }
        "not_applicable"
            if counts.analyzed == 0
                && counts.unsupported == 0
                && missing_inputs.is_empty()
                && failure_reason.is_none() =>
        {
            Ok(false)
        }
        "unsupported"
            if counts.analyzed == 0
                && counts.unsupported > 0
                && missing_inputs.is_empty()
                && failure_reason.is_none() =>
        {
            Ok(false)
        }
        "missing_input" if !missing_inputs.is_empty() && failure_reason.is_none() => Ok(false),
        "unavailable" if failure_reason.is_some() => Ok(false),
        _ => Err(format!(
            "capability execution status is inconsistent for {detector_id}: {status}"
        )),
    }
}

pub(super) fn validate_artifact_envelopes(report: &Map<String, Value>) -> Result<(), String> {
    let map = required_object_field(report, "repository_map")?;
    if required_u32(map, "schema_version")? != REPOSITORY_MAP_SCHEMA_VERSION {
        return Err("unsupported repository map reference schema".to_string());
    }
    required_string(map, "completeness")?;
    let map_state = ArtifactEnvelopeState::parse(
        map,
        &["path", "digest", "source_snapshot_digest"],
        &["file_count", "symbol_count", "edge_count"],
    )?;
    match map_state.status.as_str() {
        "absent" if map_state.absent() => {}
        "present" if map_state.identities_present() => {}
        _ => return Err("repository map reference has inconsistent status".to_string()),
    }

    let review = required_object_field(report, "engineering_review")?;
    let review_schema = required_u32(review, "schema_version")?;
    if !matches!(
        review_schema,
        ENGINEERING_REVIEW_SCHEMA_VERSION | crate::ENGINEERING_REVIEW_V2_SCHEMA_VERSION
    ) {
        return Err("unsupported engineering review envelope schema".to_string());
    }
    required_string(review, "completeness")?;
    let count_fields = if review_schema == crate::ENGINEERING_REVIEW_V2_SCHEMA_VERSION {
        &[
            "case_count",
            "accepted_recommendation_count",
            "blocked_case_count",
            "contested_case_count",
            "challenge_response_count",
        ][..]
    } else {
        &[
            "assessment_count",
            "material_recommendation_count",
            "contested_count",
            "revision_count",
        ][..]
    };
    let review_state = ArtifactEnvelopeState::parse(
        review,
        &["review_id", "input_report_digest", "path", "digest"],
        count_fields,
    )?;
    match review_state.status.as_str() {
        "absent" | "unavailable" if review_state.absent() => {}
        "complete" | "partial" if review_state.identities_present() => {}
        _ => return Err("engineering review envelope has inconsistent status".to_string()),
    }
    Ok(())
}

struct ArtifactEnvelopeState {
    status: String,
    identities: Vec<Option<String>>,
    counts: Vec<usize>,
}

impl ArtifactEnvelopeState {
    fn parse(
        value: &Map<String, Value>,
        identity_fields: &[&str],
        count_fields: &[&str],
    ) -> Result<Self, String> {
        Ok(Self {
            status: required_string(value, "status")?,
            identities: identity_fields
                .iter()
                .map(|field| optional_string(value, field))
                .collect::<Result<_, _>>()?,
            counts: count_fields
                .iter()
                .map(|field| required_usize(value, field))
                .collect::<Result<_, _>>()?,
        })
    }

    fn absent(&self) -> bool {
        self.identities.iter().all(Option::is_none) && self.counts.iter().all(|count| *count == 0)
    }

    fn identities_present(&self) -> bool {
        self.identities.iter().all(Option::is_some)
    }
}

pub(super) fn validate_finding_provenance(report: &Map<String, Value>) -> Result<(), String> {
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| "report requires findings array".to_string())?;
    for finding in findings {
        let finding = required_object(finding, "finding")?;
        let provenance = required_object_field(finding, "provenance")?;
        for field in [
            "language",
            "source_role",
            "analysis_method",
            "origin",
            "span_precision",
        ] {
            required_string(provenance, field)?;
        }
        required_u32(provenance, "extractor_version")?;
    }
    Ok(())
}

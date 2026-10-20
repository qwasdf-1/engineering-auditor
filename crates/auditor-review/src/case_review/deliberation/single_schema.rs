use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::reviewer::ProviderProbeDefinition;

pub fn reviewer_case_review_payload_schema() -> Value {
    reviewer_case_review_payload_schema_with_probes(None)
}

fn reviewer_case_review_payload_schema_with_probes(
    probes: Option<&[ProviderProbeDefinition]>,
) -> Value {
    let model_resolved_probes = probes.map_or_else(
        || json!({"items": probe_result_wire_schema(), "type": "array"}),
        probe_results_wire_schema,
    );
    json!({
        "additionalProperties": false,
        "properties": {
            "review": {
                "additionalProperties": false,
                "properties": {
                    "case_conclusion": {"enum": [
                        "lifecycle_mismatch_substantiated", "repetition_justified",
                        "impact_immaterial", "wider_ownership_infeasible",
                        "insufficient_evidence"
                    ]},
                    "case_position": {"enum": ["substantiated", "not_substantiated", "evidence_blocked", "contested", "not_applicable"]},
                    "claims": {"items": claim_wire_schema(), "type": "array"},
                    "model_resolved_probes": model_resolved_probes,
                    "preferred_recommendation_ref": {
                        "pattern": "^[a-z][a-z0-9_-]{0,63}$",
                        "type": "string"
                    },
                    "proposed_closure": {"enum": ["resolved", "resolved_with_uncertainty", "not_substantiated", "blocked_by_missing_evidence", "contested", "not_applicable"]},
                    "recommendations": {"items": recommendation_wire_schema(), "type": "array"},
                    "unresolved_gaps": {"items": {"minLength": 1, "type": "string"}, "type": "array"}
                },
                "required": [
                    "case_conclusion", "case_position", "model_resolved_probes", "claims", "recommendations",
                    "unresolved_gaps", "proposed_closure"
                ],
                "type": "object"
            }
        },
        "required": ["review"],
        "type": "object"
    })
}

fn probe_results_wire_schema(probes: &[ProviderProbeDefinition]) -> Value {
    if probes.is_empty() {
        return json!({
            "items": probe_result_wire_schema(),
            "maxItems": 0,
            "type": "array"
        });
    }
    let variants = probes
        .iter()
        .map(|probe| {
            let mut schema = probe_result_wire_schema();
            schema["properties"]["probe_id"] = json!({"enum": [probe.probe_id]});
            if probe.admissible_evidence_ids.is_empty() {
                schema["properties"]["evidence_ids"]["maxItems"] = Value::from(0);
            } else {
                schema["properties"]["evidence_ids"]["items"] =
                    json!({"enum": probe.admissible_evidence_ids});
            }
            schema
        })
        .collect::<Vec<_>>();
    json!({
        "items": {"oneOf": variants},
        "maxItems": probes.len(),
        "minItems": probes.len(),
        "type": "array",
        "uniqueItems": true
    })
}

pub(crate) fn probe_answer_tool_schema(probes: &[ProviderProbeDefinition]) -> Value {
    let mut schema = probe_result_wire_schema();
    let probe_ids = probes
        .iter()
        .map(|probe| probe.probe_id.to_string())
        .collect::<Vec<_>>();
    let evidence_ids = probes
        .iter()
        .flat_map(|probe| probe.admissible_evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    schema["properties"]["probe_id"] = json!({"enum": probe_ids});
    schema["properties"]["evidence_ids"]["uniqueItems"] = Value::Bool(true);
    if evidence_ids.is_empty() {
        schema["properties"]["evidence_ids"]["maxItems"] = Value::from(0);
    } else {
        schema["properties"]["evidence_ids"]["items"] = json!({"enum": evidence_ids});
    }
    schema
}

pub(crate) fn validate_probe_answer_arguments(
    arguments: &Value,
    probes: &[ProviderProbeDefinition],
) -> Result<(), String> {
    let probe_id = arguments
        .get("probe_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "field `probe_id` must be a string".to_string())?;
    let probe = probes
        .iter()
        .find(|probe| probe.probe_id.as_str() == probe_id)
        .ok_or_else(|| format!("unknown probe `{probe_id}`"))?;
    let evidence_ids = arguments
        .get("evidence_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| "field `evidence_ids` must be an array".to_string())?;
    let invalid = evidence_ids
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| {
            !probe
                .admissible_evidence_ids
                .iter()
                .any(|allowed| allowed == id)
        })
        .collect::<Vec<_>>();
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "probe `{probe_id}` cannot cite evidence IDs {invalid:?}; admissible evidence IDs are {:?}",
            probe.admissible_evidence_ids
        ))
    }
}

fn probe_result_wire_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "answer": {"enum": ["yes", "no", "unknown", "not_applicable", "conflicting_evidence"]},
            "evidence_ids": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "explanation": {"minLength": 1, "type": "string"},
            "limitations": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "probe_id": {"pattern": "^case-probe:[0-9a-f]{64}$", "type": "string"}
        },
        "required": ["probe_id", "answer", "evidence_ids", "explanation", "limitations"],
        "type": "object"
    })
}

fn claim_wire_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "assumptions": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "claim_ref": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"},
            "confidence": {"enum": ["low", "medium", "high"]},
            "counter_probe_ids": {"items": {"pattern": "^case-probe:[0-9a-f]{64}$", "type": "string"}, "type": "array"},
            "dependency_claim_refs": {"items": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"}, "type": "array"},
            "kind": {"enum": ["root_cause", "actionability", "recommendation_premise", "expected_benefit"]},
            "proposition": {"minLength": 1, "type": "string"},
            "slot": {"enum": [
                "repetition", "input_equivalence", "mutation_boundary", "material_impact",
                "consumer_requirement", "wider_owner", "invalidation_feasibility", "retention_risk"
            ]},
            "status": {"enum": ["substantiated", "partially_substantiated", "not_substantiated", "blocked", "contested"]},
            "supporting_probe_ids": {"items": {"pattern": "^case-probe:[0-9a-f]{64}$", "type": "string"}, "type": "array"}
        },
        "required": [
            "claim_ref", "slot", "kind", "proposition", "status", "supporting_probe_ids",
            "counter_probe_ids", "dependency_claim_refs", "assumptions", "confidence"
        ],
        "type": "object"
    })
}

pub(crate) fn recommendation_wire_schema() -> Value {
    json!({
        "additionalProperties": false,
        "properties": {
            "affected_finding_ids": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "alternatives": {
                "items": {
                    "additionalProperties": false,
                    "properties": {
                        "summary": {"minLength": 1, "type": "string"},
                        "tradeoffs": {"items": {"minLength": 1, "type": "string"}, "type": "array"}
                    },
                    "required": ["summary", "tradeoffs"],
                    "type": "object"
                },
                "type": "array"
            },
            "confidence": {"enum": ["low", "medium", "high"]},
            "costs": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "detector_advice_relationship": {"enum": [
                "preserves", "consolidates", "narrows", "sequences", "defers",
                "not_actionable_with_current_evidence"
            ]},
            "expected_benefits": {"items": expected_benefit_wire_schema(), "type": "array"},
            "family": {"enum": ["lifecycle_reownership", "explicit_retention_avoidance", "measurement_before_change"]},
            "materiality": {"enum": ["low", "medium", "high"]},
            "materiality_basis": {"minLength": 1, "type": "string"},
            "no_alternative_reason": {"minLength": 1, "type": "string"},
            "preconditions": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "premise_claim_refs": {"items": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"}, "minItems": 1, "type": "array"},
            "recommendation_ref": {"pattern": "^[a-z][a-z0-9_-]{0,63}$", "type": "string"},
            "risks": {"items": {"minLength": 1, "type": "string"}, "type": "array"},
            "scope_ids": {"items": {"minLength": 1, "type": "string"}, "minItems": 1, "type": "array"},
            "summary": {"minLength": 1, "type": "string"},
            "verification_steps": {
                "items": {
                    "additionalProperties": false,
                    "properties": {
                        "evidence_probe_ids": {"items": {"pattern": "^case-probe:[0-9a-f]{64}$", "type": "string"}, "type": "array"},
                        "expected_observation": {"minLength": 1, "type": "string"},
                        "method": {"minLength": 1, "type": "string"}
                    },
                    "required": ["method", "expected_observation", "evidence_probe_ids"],
                    "type": "object"
                },
                "minItems": 1,
                "type": "array"
            }
        },
        "required": [
            "recommendation_ref", "family", "summary", "scope_ids", "premise_claim_refs",
            "affected_finding_ids", "detector_advice_relationship", "preconditions",
            "expected_benefits", "risks", "costs", "alternatives", "verification_steps",
            "materiality", "materiality_basis", "confidence"
        ],
        "type": "object"
    })
}

pub(super) fn expected_benefit_wire_schema() -> Value {
    json!({
        "oneOf": [
            benefit_variant("direct_measurement", true, false),
            benefit_variant("bounded_extrapolation", true, true),
            benefit_variant("structural_inference", false, false),
            benefit_variant("qualitative_only", false, false),
            benefit_variant("unknown", false, false)
        ]
    })
}

fn benefit_variant(mode: &str, numeric: bool, extrapolated: bool) -> Value {
    let mut properties = Map::from_iter([
        (
            "description".to_string(),
            json!({"minLength": 1, "type": "string"}),
        ),
        ("evidence_mode".to_string(), json!({"enum": [mode]})),
        (
            "evidence_probe_ids".to_string(),
            json!({"items": {"pattern": "^case-probe:[0-9a-f]{64}$", "type": "string"}, "type": "array"}),
        ),
    ]);
    let mut required = vec!["description", "evidence_mode", "evidence_probe_ids"];
    if numeric {
        properties.insert("numeric_value".to_string(), json!({"type": "number"}));
        properties.insert(
            "unit".to_string(),
            json!({"minLength": 1, "type": "string"}),
        );
        required.extend(["numeric_value", "unit"]);
    }
    if extrapolated {
        properties.insert(
            "extrapolation_model".to_string(),
            json!({"minLength": 1, "type": "string"}),
        );
        required.push("extrapolation_model");
    }
    json!({
        "additionalProperties": false,
        "properties": properties,
        "required": required,
        "type": "object"
    })
}

pub(crate) fn validate_recommendation_benefit_arguments(
    recommendation: &Value,
) -> Result<(), Vec<String>> {
    let Some(benefits) = recommendation
        .get("expected_benefits")
        .and_then(Value::as_array)
    else {
        return Err(vec![
            "expected_benefits: must be an array of typed benefit objects".to_string(),
        ]);
    };
    let errors = benefits
        .iter()
        .enumerate()
        .flat_map(|(index, benefit)| validate_benefit(index, benefit))
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_benefit(index: usize, benefit: &Value) -> Vec<String> {
    let path = format!("expected_benefits[{index}]");
    let Some(object) = benefit.as_object() else {
        return vec![format!("{path}: must be an object")];
    };
    let Some(mode) = object.get("evidence_mode").and_then(Value::as_str) else {
        return vec![format!("{path}.evidence_mode: must be an allowed string")];
    };
    let mut errors = numeric_field_errors(&path, mode, object);
    errors.extend(extrapolation_field_errors(&path, mode, object));
    if !allowed_benefit_mode(mode) {
        errors.push(format!(
            "{path}.evidence_mode: `{mode}` is not allowed; use direct_measurement, bounded_extrapolation, structural_inference, qualitative_only, or unknown"
        ));
    }
    errors
}

fn numeric_field_errors(path: &str, mode: &str, object: &Map<String, Value>) -> Vec<String> {
    let required = matches!(mode, "direct_measurement" | "bounded_extrapolation");
    if !required {
        return ["numeric_value", "unit"]
            .into_iter()
            .filter(|field| object.contains_key(*field))
            .map(|field| format!("{path}.{field}: not allowed for evidence_mode `{mode}`"))
            .collect();
    }
    let mut errors = Vec::new();
    if !object.get("numeric_value").is_some_and(Value::is_number) {
        errors.push(format!(
            "{path}.numeric_value: required as a number for evidence_mode `{mode}`"
        ));
    }
    if !nonempty_string(object.get("unit")) {
        errors.push(format!(
            "{path}.unit: required as a non-empty string for evidence_mode `{mode}`"
        ));
    }
    errors
}

fn extrapolation_field_errors(path: &str, mode: &str, object: &Map<String, Value>) -> Vec<String> {
    let field = "extrapolation_model";
    if mode == "bounded_extrapolation" && !nonempty_string(object.get(field)) {
        vec![format!(
            "{path}.{field}: required as a non-empty string for evidence_mode `{mode}`"
        )]
    } else if mode != "bounded_extrapolation" && object.contains_key(field) {
        vec![format!(
            "{path}.{field}: not allowed for evidence_mode `{mode}`"
        )]
    } else {
        Vec::new()
    }
}

fn nonempty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn allowed_benefit_mode(mode: &str) -> bool {
    matches!(
        mode,
        "direct_measurement"
            | "bounded_extrapolation"
            | "structural_inference"
            | "qualitative_only"
            | "unknown"
    )
}

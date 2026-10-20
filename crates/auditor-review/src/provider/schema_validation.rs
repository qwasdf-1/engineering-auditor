use serde_json::Value;

pub(crate) fn validate_schema(value: &Value, schema: &Value) -> Result<(), String> {
    validate_schema_composition(value, schema)?;
    validate_schema_literals(value, schema)?;
    validate_schema_type(value, schema)?;
    if value.is_object()
        && (schema.get("properties").is_some()
            || schema.get("type").and_then(Value::as_str) == Some("object"))
    {
        validate_object_schema(value, schema)
    } else if value.is_array()
        && (schema.get("items").is_some()
            || schema.get("type").and_then(Value::as_str) == Some("array"))
    {
        validate_array_schema(value, schema)
    } else {
        validate_scalar_schema(value, schema)
    }
}

fn validate_schema_composition(value: &Value, schema: &Value) -> Result<(), String> {
    validate_all_of(value, schema)?;
    validate_any_of(value, schema)?;
    validate_one_of(value, schema)?;
    validate_conditional(value, schema)
}

fn validate_all_of(value: &Value, schema: &Value) -> Result<(), String> {
    for branch in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_schema(value, branch)?;
    }
    Ok(())
}

fn validate_any_of(value: &Value, schema: &Value) -> Result<(), String> {
    let Some(branches) = schema.get("anyOf").and_then(Value::as_array) else {
        return Ok(());
    };
    let errors = branches
        .iter()
        .filter_map(|branch| validate_schema(value, branch).err())
        .collect::<Vec<_>>();
    if errors.len() == branches.len() {
        Err(format!(
            "provider output violates every anyOf branch: {}",
            errors.join(" | ")
        ))
    } else {
        Ok(())
    }
}

fn validate_one_of(value: &Value, schema: &Value) -> Result<(), String> {
    let Some(branches) = schema.get("oneOf").and_then(Value::as_array) else {
        return Ok(());
    };
    let results = branches
        .iter()
        .map(|branch| validate_schema(value, branch))
        .collect::<Vec<_>>();
    let matching = results.iter().filter(|result| result.is_ok()).count();
    if matching == 1 {
        return Ok(());
    }
    if matching == 0 {
        if let Some(error) = selected_one_of_branch_error(value, branches, &results) {
            return Err(error);
        }
    }
    Err(format!(
        "provider output must match exactly one oneOf branch; matched {matching}"
    ))
}

fn validate_conditional(value: &Value, schema: &Value) -> Result<(), String> {
    let Some(condition) = schema.get("if") else {
        return Ok(());
    };
    let branch = if validate_schema(value, condition).is_ok() {
        schema.get("then")
    } else {
        schema.get("else")
    };
    branch.map_or(Ok(()), |branch| validate_schema(value, branch))
}

fn selected_one_of_branch_error(
    value: &Value,
    branches: &[Value],
    results: &[Result<(), String>],
) -> Option<String> {
    let object = value.as_object()?;
    for (field, field_value) in object {
        let selected = branches
            .iter()
            .enumerate()
            .filter_map(|(index, branch)| {
                let field_schema = branch.get("properties")?.get(field)?;
                let has_literal = field_schema.get("const").is_some()
                    || field_schema.get("enum").and_then(Value::as_array).is_some();
                (has_literal && validate_schema_literals(field_value, field_schema).is_ok())
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if selected.len() == 1 {
            let index = selected[0];
            if let Err(error) = &results[index] {
                return Some(format!(
                    "provider output violates oneOf branch selected by field `{field}`: {error}"
                ));
            }
        }
    }
    None
}

fn validate_schema_literals(value: &Value, schema: &Value) -> Result<(), String> {
    if schema
        .get("const")
        .is_some_and(|expected| value != expected)
    {
        return Err("provider output violates a schema const".to_string());
    }
    let Some(values) = schema.get("enum").and_then(Value::as_array) else {
        return Ok(());
    };
    if values.contains(value) {
        return Ok(());
    }
    let actual = serde_json::to_string(value)
        .unwrap_or_else(|_| "<unserializable provider value>".to_string());
    let allowed = serde_json::to_string(values)
        .unwrap_or_else(|_| "<unserializable schema enum>".to_string());
    Err(format!(
        "provider output value {actual} violates a schema enum; allowed values are {allowed}"
    ))
}

fn validate_object_schema(value: &Value, schema: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "provider output requires an object".to_string())?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut errors = undeclared_property_errors(object, &properties, schema);
    errors.extend(required_property_errors(object, schema));
    errors.extend(declared_property_errors(object, &properties));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn undeclared_property_errors(
    object: &serde_json::Map<String, Value>,
    properties: &serde_json::Map<String, Value>,
    schema: &Value,
) -> Vec<String> {
    if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Vec::new();
    }
    let undeclared = object
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if undeclared.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "provider output contains undeclared properties: {}",
            undeclared.join(", ")
        )]
    }
}

fn required_property_errors(
    object: &serde_json::Map<String, Value>,
    schema: &Value,
) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|field| !object.contains_key(*field))
        .map(|field| format!("provider output omits required field `{field}`"))
        .collect()
}

fn declared_property_errors(
    object: &serde_json::Map<String, Value>,
    properties: &serde_json::Map<String, Value>,
) -> Vec<String> {
    properties
        .iter()
        .filter_map(|(field, field_schema)| {
            object.get(field).and_then(|field_value| {
                validate_schema(field_value, field_schema)
                    .err()
                    .map(|error| format!("field `{field}`: {error}"))
            })
        })
        .collect()
}

fn validate_array_schema(value: &Value, schema: &Value) -> Result<(), String> {
    let items = value
        .as_array()
        .ok_or_else(|| "provider output requires an array".to_string())?;
    validate_array_bounds(items.len(), schema)?;
    let Some(item_schema) = schema.get("items") else {
        return Ok(());
    };
    let errors = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            validate_schema(item, item_schema)
                .err()
                .map(|error| format!("item {index}: {error}"))
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn validate_array_bounds(length: usize, schema: &Value) -> Result<(), String> {
    let max_items = schema_count(schema, "maxItems");
    if max_items.is_some_and(|maximum| length > maximum) {
        return Err(format!(
            "provider output array exceeds maxItems ({})",
            max_items.unwrap_or_default()
        ));
    }
    let min_items = schema_count(schema, "minItems");
    if min_items.is_some_and(|minimum| length < minimum) {
        return Err(format!(
            "provider output array is below minItems ({})",
            min_items.unwrap_or_default()
        ));
    }
    Ok(())
}

fn schema_count(schema: &Value, field: &str) -> Option<usize> {
    schema
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn validate_schema_type(value: &Value, schema: &Value) -> Result<(), String> {
    let expected = match schema.get("type") {
        Some(Value::String(expected)) => vec![expected.as_str()],
        Some(Value::Array(expected)) => expected.iter().filter_map(Value::as_str).collect(),
        _ => return Ok(()),
    };
    if expected.iter().any(|kind| value_matches_type(value, kind)) {
        Ok(())
    } else {
        Err(format!(
            "provider output requires type {}",
            expected.join("|")
        ))
    }
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn validate_scalar_schema(value: &Value, schema: &Value) -> Result<(), String> {
    if let (Some(text), Some(minimum)) = (
        value.as_str(),
        schema.get("minLength").and_then(Value::as_u64),
    ) {
        if text.chars().count() < usize::try_from(minimum).unwrap_or(usize::MAX) {
            return Err(format!(
                "provider output string is below minLength ({minimum})"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_schema;

    #[test]
    fn recursive_schema_validation_enforces_nested_enum_min_items_and_nullable_types() {
        let schema = json!({
            "additionalProperties": false,
            "properties": {
                "rows": {
                    "items": {
                        "additionalProperties": false,
                        "properties": {
                            "kind": {"enum": ["allowed"]},
                            "refs": {"items": {"type": "string"}, "minItems": 1, "type": "array"},
                            "note": {"type": ["string", "null"]}
                        },
                        "required": ["kind", "refs", "note"],
                        "type": "object"
                    },
                    "minItems": 1,
                    "type": "array"
                }
            },
            "required": ["rows"],
            "type": "object"
        });
        assert!(validate_schema(
            &json!({"rows": [{"kind": "allowed", "refs": ["case-probe:x"], "note": null}]}),
            &schema
        )
        .is_ok());
        assert!(validate_schema(
            &json!({"rows": [{"kind": "outside", "refs": ["case-probe:x"], "note": null}]}),
            &schema
        )
        .unwrap_err()
        .contains("schema enum"));
        assert!(validate_schema(
            &json!({"rows": [{"kind": "allowed", "refs": [], "note": null}]}),
            &schema
        )
        .unwrap_err()
        .contains("minItems"));
    }

    #[test]
    fn conditional_schema_composition_enforces_dependent_fields() {
        let schema = json!({
            "allOf": [{
                "if": {"properties": {"mode": {"const": "structural"}}, "required": ["mode"]},
                "then": {"properties": {"numeric": {"type": "null"}}}
            }],
            "properties": {
                "mode": {"enum": ["structural", "measured"]},
                "numeric": {"type": ["string", "null"]}
            },
            "required": ["mode", "numeric"],
            "type": "object"
        });
        assert!(validate_schema(&json!({"mode": "structural", "numeric": null}), &schema).is_ok());
        assert!(validate_schema(&json!({"mode": "structural", "numeric": "2"}), &schema).is_err());
        assert!(validate_schema(&json!({"mode": "measured", "numeric": "2"}), &schema).is_ok());
    }

    #[test]
    fn any_of_recurses_and_reports_all_nested_object_errors() {
        let schema = json!({
            "properties": {
                "payload": {
                    "anyOf": [{
                        "additionalProperties": false,
                        "properties": {
                            "mode": {"enum": ["structural"]},
                            "refs": {"items": {"type": "string"}, "type": "array"}
                        },
                        "required": ["mode", "refs"],
                        "type": "object"
                    }, {"type": "null"}]
                }
            },
            "required": ["payload"],
            "type": "object"
        });
        assert!(validate_schema(&json!({"payload": null}), &schema).is_ok());
        assert!(validate_schema(
            &json!({"payload": {"mode": "structural", "refs": []}}),
            &schema
        )
        .is_ok());
        let error = validate_schema(&json!({"payload": {"benefit_kind": "complexity"}}), &schema)
            .expect_err("nested anyOf object must be validated");
        assert!(error.contains("undeclared properties: benefit_kind"));
        assert!(error.contains("omits required field `mode`"));
        assert!(error.contains("omits required field `refs`"));
    }

    #[test]
    fn one_of_requires_exactly_one_matching_branch() {
        let schema = json!({"oneOf": [{"type": "string"}, {"enum": ["exact"]}]});
        assert!(validate_schema(&json!(1), &schema).is_err());
        assert!(validate_schema(&json!("other"), &schema).is_ok());
        assert!(validate_schema(&json!("exact"), &schema).is_err());
    }

    #[test]
    fn one_of_reports_the_uniquely_discriminated_branch_error_and_array_index() {
        let schema = json!({
            "items": {
                "oneOf": [{
                    "additionalProperties": false,
                    "properties": {
                        "probe_id": {"enum": ["probe-a"]},
                        "evidence_ids": {"items": {"enum": ["evidence-a"]}, "type": "array"}
                    },
                    "required": ["probe_id", "evidence_ids"],
                    "type": "object"
                }, {
                    "additionalProperties": false,
                    "properties": {
                        "probe_id": {"enum": ["probe-b"]},
                        "evidence_ids": {"items": {"enum": ["evidence-b"]}, "type": "array"}
                    },
                    "required": ["probe_id", "evidence_ids"],
                    "type": "object"
                }]
            },
            "type": "array"
        });
        let error = validate_schema(
            &json!([
                {"probe_id": "probe-a", "evidence_ids": ["evidence-b"]},
                {"probe_id": "probe-b", "evidence_ids": ["evidence-a"]}
            ]),
            &schema,
        )
        .expect_err("both probes must use their own evidence allowlists");
        assert!(error.contains("item 0"));
        assert!(error.contains("item 1"));
        assert!(error.contains("selected by field `probe_id`"));
        assert!(error.contains("field `evidence_ids`"));
        assert!(error.contains("schema enum"));
        assert!(error.contains("\"evidence-a\""));
        assert!(error.contains("\"evidence-b\""));
    }
}

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::parser::{required_object, required_object_field, required_string, ParsedFinding};

pub(super) fn validate_graph(
    report: &Map<String, Value>,
    findings: &[ParsedFinding],
) -> Result<(), String> {
    let graph = required_object_field(report, "evidence_graph")?;
    ensure_complete(graph)?;
    let nodes = required_array(graph, "nodes")?;
    let edges = required_array(graph, "edges")?;
    let node_ids = validate_nodes(nodes)?;
    let targets = validate_edges(edges, &node_ids)?;
    validate_finding_support(findings, &node_ids, &targets)
}

fn ensure_complete(graph: &Map<String, Value>) -> Result<(), String> {
    let completeness = required_object_field(graph, "completeness")?;
    match completeness.get("complete").and_then(Value::as_bool) {
        Some(true) => Ok(()),
        _ => Err("evidence graph is not complete".to_string()),
    }
}

fn required_array<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a [Value], String> {
    object
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("evidence graph requires {field} array"))
}

fn validate_nodes(nodes: &[Value]) -> Result<BTreeSet<String>, String> {
    let mut node_ids = BTreeSet::new();
    for node in nodes {
        validate_node(node, &mut node_ids)?;
    }
    Ok(node_ids)
}

fn validate_node(node: &Value, node_ids: &mut BTreeSet<String>) -> Result<(), String> {
    let node = required_object(node, "graph node")?;
    let id = required_string(node, "id")?;
    if !node_ids.insert(id) {
        return Err("evidence graph contains duplicate node IDs".to_string());
    }
    validate_node_kind(&required_string(node, "kind")?)?;
    validate_node_properties(node)
}

fn validate_node_kind(kind: &str) -> Result<(), String> {
    match kind {
        "file" | "symbol" | "fact" | "measurement" | "finding" | "detector" | "external_input" => {
            Ok(())
        }
        other => Err(format!("unsupported evidence node kind {other}")),
    }
}

fn validate_node_properties(node: &Map<String, Value>) -> Result<(), String> {
    let Some(properties) = node.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    for forbidden in ["raw_source", "source_text", "snippet"] {
        if properties.contains_key(forbidden) {
            return Err(format!("evidence graph embeds forbidden {forbidden}"));
        }
    }
    Ok(())
}

#[derive(Default)]
struct EdgeTargets {
    support: BTreeSet<String>,
    produced: BTreeSet<String>,
}

fn validate_edges(edges: &[Value], node_ids: &BTreeSet<String>) -> Result<EdgeTargets, String> {
    let mut targets = EdgeTargets::default();
    for edge in edges {
        validate_edge(edge, node_ids, &mut targets)?;
    }
    Ok(targets)
}

fn validate_edge(
    edge: &Value,
    node_ids: &BTreeSet<String>,
    targets: &mut EdgeTargets,
) -> Result<(), String> {
    let edge = required_object(edge, "graph edge")?;
    let from = required_string(edge, "from")?;
    let to = required_string(edge, "to")?;
    if !node_ids.contains(&from) || !node_ids.contains(&to) {
        return Err("evidence graph contains a dangling edge".to_string());
    }
    match required_string(edge, "kind")?.as_str() {
        "contains" | "references" | "derived_from" => Ok(()),
        "supports" => {
            targets.support.insert(to);
            Ok(())
        }
        "produced_by" => {
            targets.produced.insert(from);
            Ok(())
        }
        other => Err(format!("unsupported evidence edge kind {other}")),
    }
}

fn validate_finding_support(
    findings: &[ParsedFinding],
    node_ids: &BTreeSet<String>,
    targets: &EdgeTargets,
) -> Result<(), String> {
    for finding in findings {
        let id = format!("finding:{}", finding.id);
        let complete = node_ids.contains(&id)
            && targets.support.contains(&id)
            && targets.produced.contains(&id);
        if !complete {
            return Err(format!(
                "finding {} lacks complete graph support",
                finding.id
            ));
        }
    }
    Ok(())
}

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::index::{CapabilityFact, FindingFact};
use super::CandidateSelectionError;

type EdgeLinks = BTreeMap<String, Vec<(String, String)>>;

pub(super) struct ParsedReport {
    pub findings: BTreeMap<String, FindingFact>,
    pub capabilities: Vec<CapabilityFact>,
    pub node_ids: BTreeSet<String>,
    pub adjacency: BTreeMap<String, BTreeSet<String>>,
}

struct GraphFacts {
    node_kinds: BTreeMap<String, String>,
    node_ids: BTreeSet<String>,
    incoming: EdgeLinks,
    outgoing: EdgeLinks,
    adjacency: BTreeMap<String, BTreeSet<String>>,
}

pub(super) fn parse_report(report: &Value) -> Result<ParsedReport, CandidateSelectionError> {
    let graph = parse_graph(report)?;
    let layers = parse_layers(report)?;
    let findings = parse_findings(report, &graph, &layers)?;
    let capabilities = parse_capabilities(report, &graph.node_ids)?;
    Ok(ParsedReport {
        findings,
        capabilities,
        node_ids: graph.node_ids,
        adjacency: graph.adjacency,
    })
}

fn parse_graph(report: &Value) -> Result<GraphFacts, CandidateSelectionError> {
    let graph = report
        .get("evidence_graph")
        .and_then(Value::as_object)
        .ok_or_else(|| contract_error("report is missing evidence_graph"))?;
    let node_kinds = required_array_value(graph.get("nodes"), "evidence graph is missing nodes")?
        .iter()
        .map(|node| Ok((required_string(node, "id")?, required_string(node, "kind")?)))
        .collect::<Result<BTreeMap<_, _>, CandidateSelectionError>>()?;
    let edges = required_array_value(graph.get("edges"), "evidence graph is missing edges")?
        .iter()
        .map(|edge| {
            Ok((
                required_string(edge, "from")?,
                required_string(edge, "to")?,
                required_string(edge, "kind")?,
            ))
        })
        .collect::<Result<Vec<_>, CandidateSelectionError>>()?;
    let mut incoming = EdgeLinks::new();
    let mut outgoing = EdgeLinks::new();
    let mut adjacency = BTreeMap::<String, BTreeSet<String>>::new();
    for (from, to, kind) in edges {
        incoming
            .entry(to.clone())
            .or_default()
            .push((from.clone(), kind.clone()));
        outgoing
            .entry(from.clone())
            .or_default()
            .push((to.clone(), kind));
        adjacency
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        adjacency.entry(to).or_default().insert(from);
    }
    let node_ids = node_kinds.keys().cloned().collect();
    Ok(GraphFacts {
        node_kinds,
        node_ids,
        incoming,
        outgoing,
        adjacency,
    })
}

fn parse_layers(report: &Value) -> Result<BTreeMap<String, String>, CandidateSelectionError> {
    required_array(report, "detectors")?
        .iter()
        .map(|detector| {
            Ok((
                required_string(detector, "id")?,
                required_string(detector, "layer")?,
            ))
        })
        .collect()
}

fn parse_findings(
    report: &Value,
    graph: &GraphFacts,
    layers: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, FindingFact>, CandidateSelectionError> {
    required_array(report, "findings")?
        .iter()
        .map(|finding| parse_finding(finding, graph, layers))
        .collect()
}

fn parse_finding(
    finding: &Value,
    graph: &GraphFacts,
    layers: &BTreeMap<String, String>,
) -> Result<(String, FindingFact), CandidateSelectionError> {
    let id = required_string(finding, "id")?;
    let detector = required_string(finding, "detector")?;
    let graph_id = format!("finding:{id}");
    let mut evidence_refs = linked_ids(&graph.incoming, &graph_id, "supports");
    let mut derived = evidence_refs
        .iter()
        .flat_map(|support| linked_ids(&graph.outgoing, support, "derived_from"))
        .collect::<Vec<_>>();
    derived.sort();
    let subject_id = exact_subject(&graph_id, &derived, graph)
        .ok_or_else(|| contract_error(&format!("finding {id} has no exact graph subject")))?;
    evidence_refs.sort();
    evidence_refs.dedup();
    if evidence_refs.is_empty() {
        return Err(contract_error(&format!(
            "finding {id} has no selection evidence"
        )));
    }
    let layer = layers.get(&detector).cloned().ok_or_else(|| {
        contract_error(&format!("finding references unknown detector {detector}"))
    })?;
    let recommendation = required_string(finding, "recommendation")?;
    Ok((
        id.clone(),
        FindingFact {
            id,
            detector,
            recommendation_key: normalize_text(&recommendation),
            subject_id,
            evidence_refs,
            layer,
        },
    ))
}

fn exact_subject(graph_id: &str, derived: &[String], graph: &GraphFacts) -> Option<String> {
    let typed = |kind| {
        derived
            .iter()
            .find(|id| graph.node_kinds.get(*id).map(String::as_str) == Some(kind))
            .cloned()
    };
    typed("symbol").or_else(|| typed("file")).or_else(|| {
        linked_ids(&graph.incoming, graph_id, "contains")
            .into_iter()
            .filter(|id| graph.node_kinds.get(id).map(String::as_str) == Some("file"))
            .min()
    })
}

fn linked_ids(links: &EdgeLinks, id: &str, kind: &str) -> Vec<String> {
    links
        .get(id)
        .into_iter()
        .flatten()
        .filter(|(_, edge_kind)| edge_kind == kind)
        .map(|(linked, _)| linked.clone())
        .collect()
}

fn parse_capabilities(
    report: &Value,
    node_ids: &BTreeSet<String>,
) -> Result<Vec<CapabilityFact>, CandidateSelectionError> {
    required_array(report, "capability_executions")?
        .iter()
        .map(|capability| {
            let detector_id = required_string(capability, "detector_id")?;
            let subject_id = format!("detector:{detector_id}");
            if !node_ids.contains(&subject_id) {
                return Err(contract_error(&format!(
                    "capability references missing detector node {subject_id}"
                )));
            }
            Ok(CapabilityFact {
                subject_id: subject_id.clone(),
                evidence_refs: vec![subject_id],
                layer: required_string(capability, "layer")?,
                status: required_string(capability, "status")?,
                eligible_files: required_usize(capability, "eligible_files")?,
            })
        })
        .collect()
}

fn required_array<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a Vec<Value>, CandidateSelectionError> {
    required_array_value(value.get(field), &format!("report is missing {field}"))
}

fn required_array_value<'a>(
    value: Option<&'a Value>,
    detail: &str,
) -> Result<&'a Vec<Value>, CandidateSelectionError> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| contract_error(detail))
}

fn required_string(value: &Value, field: &str) -> Result<String, CandidateSelectionError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| contract_error(&format!("missing string field {field}")))
}

fn required_usize(value: &Value, field: &str) -> Result<usize, CandidateSelectionError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| contract_error(&format!("missing integer field {field}")))
}

fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn contract_error(detail: &str) -> CandidateSelectionError {
    CandidateSelectionError::InvalidInput {
        detail: detail.to_string(),
    }
}

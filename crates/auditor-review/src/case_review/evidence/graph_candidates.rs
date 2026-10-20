use std::collections::BTreeSet;

use serde_json::Value;

use crate::{EvidenceClass, ReviewInput, TrustLabel};

use super::compile::{candidate, CandidateItem, RequirementTier};
use super::identity::generated_item_id;
use super::{AllowedEvidenceNeighborhood, EvidenceError, EvidenceItem, EvidenceItemKind};

pub(super) fn collect_graph_candidates(
    input: &ReviewInput,
    neighborhood: &AllowedEvidenceNeighborhood,
    candidates: &mut Vec<CandidateItem>,
) -> Result<(), EvidenceError> {
    let allowed = neighborhood
        .graph_node_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let graph = &input.report()["evidence_graph"];
    for node in graph["nodes"].as_array().into_iter().flatten() {
        if let Some(item) = graph_node_candidate(node, &allowed)? {
            candidates.push(item);
        }
    }
    for edge in graph["edges"].as_array().into_iter().flatten() {
        if let Some(item) = graph_edge_candidate(edge, &allowed)? {
            candidates.push(item);
        }
    }
    Ok(())
}

fn graph_node_candidate(
    node: &Value,
    allowed: &BTreeSet<&str>,
) -> Result<Option<CandidateItem>, EvidenceError> {
    let Some(id) = node["id"].as_str().filter(|id| allowed.contains(id)) else {
        return Ok(None);
    };
    let Some(kind) = graph_node_kind(node, id) else {
        return Ok(None);
    };
    let typed_id = if id.starts_with(kind.prefix()) {
        id.to_string()
    } else {
        generated_item_id(kind.prefix().trim_end_matches(':'), node)?
    };
    let item = EvidenceItem::new(
        typed_id,
        kind,
        TrustLabel::DeterministicEvidence,
        format!("audit-report#evidence-graph/node/{id}"),
        node.clone(),
    )?;
    Ok(Some(candidate(
        item,
        [EvidenceClass::EvidenceGraph],
        RequirementTier::Required,
    )))
}

fn graph_node_kind(node: &Value, id: &str) -> Option<EvidenceItemKind> {
    match node["kind"].as_str()? {
        "finding" if id.starts_with("finding:") => Some(EvidenceItemKind::Finding),
        "fact" | "file" | "external_input" => Some(EvidenceItemKind::Fact),
        "measurement" => Some(EvidenceItemKind::Measurement),
        "symbol" => Some(EvidenceItemKind::Symbol),
        "detector" => Some(EvidenceItemKind::Capability),
        _ => None,
    }
}

fn graph_edge_candidate(
    edge: &Value,
    allowed: &BTreeSet<&str>,
) -> Result<Option<CandidateItem>, EvidenceError> {
    let Some((from, to)) = edge_endpoints(edge) else {
        return Ok(None);
    };
    if !allowed.contains(from) || !allowed.contains(to) {
        return Ok(None);
    }
    let item = EvidenceItem::new(
        generated_item_id("edge", edge)?,
        EvidenceItemKind::Edge,
        TrustLabel::DeterministicEvidence,
        format!("audit-report#evidence-graph/edge/{from}/{to}"),
        edge.clone(),
    )?;
    Ok(Some(candidate(
        item,
        [EvidenceClass::EvidenceGraph],
        RequirementTier::Required,
    )))
}

fn edge_endpoints(edge: &Value) -> Option<(&str, &str)> {
    Some((edge["from"].as_str()?, edge["to"].as_str()?))
}

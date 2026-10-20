use std::collections::BTreeSet;

use serde_json::Value;

use crate::{repository_map::map_symbol_neighborhood, EngineeringCase, RepositoryMap, ReviewInput};

use super::{AllowedEvidenceNeighborhood, EvidenceError};

pub(super) fn case_neighborhood(
    case: &EngineeringCase,
    input: &ReviewInput,
    maximum_hops: usize,
) -> Result<AllowedEvidenceNeighborhood, EvidenceError> {
    let graph = input
        .report()
        .get("evidence_graph")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            EvidenceError::InvalidPacket("report evidence graph is absent".to_string())
        })?;
    let nodes = graph
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            EvidenceError::InvalidPacket("evidence graph nodes are absent".to_string())
        })?;
    let edges = graph
        .get("edges")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            EvidenceError::InvalidPacket("evidence graph edges are absent".to_string())
        })?;
    let selected_findings = case
        .finding_ids
        .iter()
        .map(|id| format!("finding:{id}"))
        .collect::<BTreeSet<_>>();
    let graph_nodes = graph_neighborhood(case, nodes, edges, &selected_findings, maximum_hops);
    let map_symbols = repository_map_neighborhood(case, input, &graph_nodes, maximum_hops);
    Ok(AllowedEvidenceNeighborhood {
        finding_ids: case.finding_ids.clone(),
        graph_node_ids: graph_nodes.into_iter().collect(),
        map_symbol_ids: map_symbols.into_iter().collect(),
        maximum_graph_hops: maximum_hops,
    })
}

fn graph_neighborhood(
    case: &EngineeringCase,
    nodes: &[Value],
    edges: &[Value],
    selected_findings: &BTreeSet<String>,
    maximum_hops: usize,
) -> BTreeSet<String> {
    let node_ids = nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut frontier = case
        .subject_ids
        .iter()
        .chain(&case.selection_provenance_refs)
        .cloned()
        .chain(selected_findings.iter().cloned())
        .filter(|id| node_ids.contains(id))
        .collect::<BTreeSet<_>>();
    let mut allowed = frontier.clone();
    for _ in 0..maximum_hops {
        frontier = edges
            .iter()
            .filter_map(|edge| adjacent_id(edge, &frontier))
            .filter(|id| !id.starts_with("finding:") || selected_findings.contains(*id))
            .filter(|id| node_ids.contains(*id))
            .map(str::to_string)
            .filter(|id| allowed.insert(id.clone()))
            .collect();
    }
    allowed
}

fn adjacent_id<'a>(edge: &'a Value, frontier: &BTreeSet<String>) -> Option<&'a str> {
    let from = edge.get("from")?.as_str()?;
    let to = edge.get("to")?.as_str()?;
    frontier
        .contains(from)
        .then_some(to)
        .or_else(|| frontier.contains(to).then_some(from))
}

fn repository_map_neighborhood(
    case: &EngineeringCase,
    input: &ReviewInput,
    graph_nodes: &BTreeSet<String>,
    maximum_hops: usize,
) -> BTreeSet<String> {
    let map = input.repository_map();
    let map_node_ids = map
        .symbols
        .iter()
        .map(|symbol| symbol.id.clone())
        .collect::<BTreeSet<_>>();
    let mut seeds = graph_nodes
        .intersection(&map_node_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    seeds.extend(finding_location_symbols(case, input, map));
    map_symbol_neighborhood(map, &seeds, maximum_hops)
        .into_iter()
        .filter(|id| map_node_ids.contains(id))
        .collect()
}

fn finding_location_symbols(
    case: &EngineeringCase,
    input: &ReviewInput,
    map: &RepositoryMap,
) -> BTreeSet<String> {
    case.finding_ids
        .iter()
        .filter_map(|id| input.finding(id))
        .filter_map(|finding| finding["locations"].as_array()?.first())
        .filter_map(|location| {
            let path = location["path"].as_str()?;
            let line = usize::try_from(location["line"].as_u64()?).ok()?;
            let file_id = map.files.iter().find(|file| file.path == path)?.id.as_str();
            Some((file_id, line))
        })
        .flat_map(|(file_id, line)| {
            map.symbols
                .iter()
                .filter(move |symbol| {
                    symbol.file_id == file_id
                        && symbol.span.start_line <= line
                        && line <= symbol.span.end_line
                })
                .map(|symbol| symbol.id.clone())
        })
        .collect()
}

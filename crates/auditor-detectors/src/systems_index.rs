//! The Layer 4 index the systems owners read: every emittable file's direct
//! and inherited effects, and the resolved local topology.

use std::collections::BTreeMap;

use crate::call_graph::CallGraph;
use crate::structure_index::StructureIndex;

use super::systems_effects::extract_effects;
use super::systems_facts::{SystemEffect, TopologyEdge, TopologyFile};
use super::systems_inherited::inherited_effects;
use super::systems_topology::build_topology;

pub(crate) struct SystemsIndex {
    effects: Vec<SystemEffect>,
    files: BTreeMap<String, TopologyFile>,
    edges: Vec<TopologyEdge>,
    cycles: Vec<Vec<String>>,
    local_edge_candidates: usize,
}

impl SystemsIndex {
    pub(crate) fn build(structure: &StructureIndex, graph: &CallGraph) -> Self {
        let topology = build_topology(structure);
        let direct: Vec<Vec<SystemEffect>> =
            structure.files().iter().map(extract_effects).collect();
        let inherited = inherited_effects(structure, graph, &direct);
        let effects = structure
            .files()
            .iter()
            .zip(direct.into_iter().zip(inherited))
            .filter(|(file, _)| file.emittable)
            .flat_map(|(_, (direct, inherited))| direct.into_iter().chain(inherited))
            .collect();
        Self {
            effects,
            files: topology.files,
            edges: topology.edges,
            cycles: topology.cycles,
            local_edge_candidates: topology.local_edge_candidates,
        }
    }

    pub(crate) fn effects(&self) -> &[SystemEffect] {
        &self.effects
    }

    pub(crate) fn files(&self) -> &BTreeMap<String, TopologyFile> {
        &self.files
    }

    pub(crate) fn edges(&self) -> &[TopologyEdge] {
        &self.edges
    }

    pub(crate) fn cycles(&self) -> &[Vec<String>] {
        &self.cycles
    }

    pub(crate) fn topology_complete_for(&self, path: &str) -> bool {
        self.files
            .get(path)
            .is_some_and(|file| file.unresolved_local_edges == 0)
    }

    pub(crate) fn topology_completeness(&self) -> f64 {
        if self.local_edge_candidates == 0 {
            1.0
        } else {
            self.edges.len() as f64 / self.local_edge_candidates as f64
        }
    }
}

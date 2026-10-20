//! Compact typed effects and resolved local topology for Layer 4 owners.

use std::collections::BTreeMap;

use crate::structure_index::StructureIndex;
use crate::Language;

use super::systems_effects::extract_effects;
use super::systems_topology::build_topology;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(usize)]
pub(crate) enum EffectKind {
    ArtifactRead,
    ArtifactWrite,
    BlockingIo,
    ExternalIo,
    Parse,
    Scan,
    Serialize,
    CacheInsert,
    CacheLatestRead,
    CacheBound,
    ExpensiveInitialization,
    RepositoryScan,
}

impl EffectKind {
    const NAMES: [&'static str; 12] = [
        "artifact_read",
        "artifact_write",
        "blocking_io",
        "external_io",
        "parse",
        "scan",
        "serialize",
        "cache_insert",
        "cache_latest_read",
        "cache_bound",
        "expensive_initialization",
        "repository_scan",
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SystemEffect {
    pub(crate) path: String,
    pub(crate) language: Language,
    pub(crate) function: String,
    pub(crate) line: usize,
    pub(crate) loop_line: Option<usize>,
    pub(crate) kind: EffectKind,
    pub(crate) subject: Option<String>,
    pub(crate) artifact: Option<String>,
    pub(crate) value_lineage: Option<String>,
    /// The artifact or constructor input depends on the enclosing loop variable.
    pub(crate) loop_variant: bool,
    pub(crate) operation: String,
    pub(crate) in_async: bool,
    pub(crate) in_hot_path: bool,
    pub(crate) in_request_path: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(usize)]
pub(crate) enum TopologyEdgeKind {
    Import,
    Reexport,
    Inheritance,
}

impl TopologyEdgeKind {
    const NAMES: [&'static str; 3] = ["import", "reexport", "inheritance"];

    pub(crate) const fn as_str(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TopologyEdge {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) source_package: String,
    pub(crate) target_package: String,
    pub(crate) line: usize,
    pub(crate) symbol: Option<String>,
    pub(crate) kind: TopologyEdgeKind,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TopologyFile {
    pub(crate) package: String,
    pub(crate) type_names: Vec<String>,
    pub(crate) abstract_types: usize,
    pub(crate) concrete_types: usize,
    pub(crate) unresolved_local_edges: usize,
}

pub(crate) struct SystemsIndex {
    effects: Vec<SystemEffect>,
    files: BTreeMap<String, TopologyFile>,
    edges: Vec<TopologyEdge>,
    cycles: Vec<Vec<String>>,
    local_edge_candidates: usize,
}

impl SystemsIndex {
    pub(crate) fn build(structure: &StructureIndex) -> Self {
        let topology = build_topology(structure);
        let effects = structure
            .files()
            .iter()
            .filter(|file| file.emittable)
            .flat_map(extract_effects)
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

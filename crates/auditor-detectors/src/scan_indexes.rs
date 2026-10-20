//! The lazily built indexes a scan's detectors share: built before the
//! first owner that uses each, released after the last.

use super::*;

#[derive(Default)]
pub(super) struct RemainingIndexUsers {
    algorithmic: usize,
    complexity: usize,
    optimization: usize,
    structure: usize,
    systems: usize,
}

impl RemainingIndexUsers {
    pub(super) fn from_specs(
        specs: &[&registry::DetectorSpec],
        reserve_repository_map: bool,
    ) -> Self {
        let mut users = Self::default();
        for spec in specs {
            let owner = spec.owner;
            users.algorithmic += usize::from(owner.uses_algorithmic_index());
            users.complexity += usize::from(owner.uses_complexity_index());
            users.optimization += usize::from(owner.uses_optimization_index());
            users.structure += usize::from(owner.uses_structure_index());
            users.systems += usize::from(owner.uses_systems_index());
        }
        users.structure += usize::from(reserve_repository_map);
        users
    }

    pub(super) fn release(
        owner: registry::DetectorOwner,
        context: &mut DetectorContext<'_>,
        users: &mut Self,
    ) {
        if owner.uses_algorithmic_index() {
            release_index(&mut users.algorithmic, &mut context.algorithmic);
        }
        if owner.uses_complexity_index() {
            release_index(&mut users.complexity, &mut context.complexity);
        }
        if owner.uses_optimization_index() {
            release_index(&mut users.optimization, &mut context.optimization);
        }
        if owner.uses_systems_index() {
            release_index(&mut users.systems, &mut context.systems);
        }
        if owner.uses_structure_index() {
            release_structure(&mut users.structure, context);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.algorithmic == 0
            && self.complexity == 0
            && self.optimization == 0
            && self.structure == 0
            && self.systems == 0
    }
}

/// Releases one use of the structure index, and the call graph built from
/// it with its last use.
fn release_structure(remaining: &mut usize, context: &mut DetectorContext<'_>) {
    release_index(remaining, &mut context.structure);
    if context.structure.is_none() {
        context.call_graph = None;
    }
}

pub(super) fn release_index<T>(remaining: &mut usize, index: &mut Option<T>) {
    *remaining = remaining
        .checked_sub(1)
        .expect("index user count must cover every executed owner");
    if *remaining == 0 {
        *index = None;
    }
}

impl ScanRun<'_> {
    /// Releases the structure index reserved for the repository map.
    pub(super) fn release_reserved_structure(&mut self) {
        release_structure(&mut self.remaining_index_users.structure, &mut self.context);
    }

    pub(super) fn prepare_indexes(&mut self, owner: registry::DetectorOwner) {
        if owner.uses_structure_index() {
            self.ensure_structure_index();
        }
        self.ensure_call_following_indexes(owner);
        if owner.uses_optimization_index() && self.context.optimization.is_none() {
            self.context.optimization = Some(optimization_facts::OptimizationIndex::build(
                self.context.structure.as_ref().expect("structure index"),
                self.context
                    .algorithmic
                    .as_ref()
                    .expect("algorithmic index"),
            ));
        }
        if owner.uses_complexity_index() && self.context.complexity.is_none() {
            self.context.complexity = Some(cognitive::ComplexityIndex::build(
                self.role_views.code,
                self.context.cache,
                &mut self.context.repo_ast,
            ));
        }
    }

    /// The algorithmic and systems indexes, which share one call graph.
    fn ensure_call_following_indexes(&mut self, owner: registry::DetectorOwner) {
        let context = &mut self.context;
        let algorithmic = owner.uses_algorithmic_index() && context.algorithmic.is_none();
        let systems = owner.uses_systems_index() && context.systems.is_none();
        if algorithmic || systems {
            let structure = context.structure.as_ref().expect("structure index");
            let graph = context
                .call_graph
                .get_or_insert_with(|| call_graph::CallGraph::build(structure));
            if algorithmic {
                context.algorithmic =
                    Some(algorithmic_facts::AlgorithmicIndex::build(structure, graph));
            }
            if systems {
                context.systems = Some(systems_facts::SystemsIndex::build(structure, graph));
            }
        }
    }

    pub(super) fn ensure_structure_index(&mut self) {
        if self.context.structure.is_none() {
            self.context.structure = Some(structure_index::StructureIndex::build(
                ast_scan::AstScan::context(
                    self.role_views.test_evolution,
                    self.context.method,
                    self.context.cache,
                    &mut self.context.repo_ast,
                ),
            ));
        }
    }
}

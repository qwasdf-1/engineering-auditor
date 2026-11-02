//! Whether a query-named call (`cache.getResourceByUriCacheKey(id)`,
//! `workspace.getProjectById(id)`) runs only repository code that reads no
//! store: the methods its receiver's type declares, and the repository
//! functions they call in turn, issue no read query. A method without a
//! body (an interface's or an abstract one) runs code elsewhere, so it is
//! never read as reading nothing.

use std::collections::{BTreeMap, BTreeSet};

use super::queries::may_be_store_query;
use super::structure::call_shape;
use crate::call_graph::CallGraph;
use crate::structure_index::{StructureCall, StructureIndex};
use crate::type_facts::{SymbolRef, TypeScope};

/// How many repository calls deep a callee's body is followed.
const MAX_CALLEE_DEPTH: usize = 3;

/// What a function's calls resolve to: the repository's declarations and
/// call graph, from one file.
#[derive(Clone, Copy)]
pub(super) struct Callees<'a> {
    pub(super) structure: &'a StructureIndex,
    pub(super) graph: &'a CallGraph,
    pub(super) types: TypeScope<'a>,
}

impl Callees<'_> {
    /// `call` in function `owner` runs only repository methods that read no
    /// store.
    pub(super) fn read_nothing(&self, owner: u32, call: &StructureCall) -> bool {
        let Some(method) = crate::util::identifiers(&call.callee)
            .into_iter()
            .next_back()
        else {
            return false;
        };
        let receiver = call.receiver.as_deref().unwrap_or_else(|| {
            call.callee
                .rfind(&method)
                .map(|index| &call.callee[..index])
                .unwrap_or_default()
        });
        let targets = self.types.methods(owner as usize, receiver, &method);
        let mut visited = BTreeSet::new();
        !targets.is_empty()
            && targets
                .into_iter()
                .all(|target| self.reads_nothing(target, 0, &mut visited))
    }

    fn reads_nothing(
        &self,
        target: SymbolRef,
        depth: usize,
        visited: &mut BTreeSet<SymbolRef>,
    ) -> bool {
        if !visited.insert(target) {
            return true;
        }
        let (file_index, symbol) = target;
        let file = &self.structure.files()[file_index];
        if depth > MAX_CALLEE_DEPTH || file.symbols[symbol].typing.bodiless {
            return false;
        }
        let no_aliases = BTreeMap::new();
        file.calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.owner == Some(symbol as u32))
            .all(|(call_index, call)| {
                !call_shape(call, &no_aliases).is_some_and(|shape| may_be_store_query(&shape))
                    && self
                        .graph
                        .target(file_index, call_index)
                        .is_none_or(|next| self.reads_nothing(next, depth + 1, visited))
            })
    }
}

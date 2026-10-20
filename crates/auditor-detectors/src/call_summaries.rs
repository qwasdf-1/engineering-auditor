//! Per-function summaries carried through resolved calls, shared by the
//! indexes that follow calls. Findings-free.
//!
//! A summary starts as what a function does itself and grows one call deeper
//! per round, through the summaries of the functions it calls, until nothing
//! changes or a depth limit is reached.

use std::collections::HashMap;

use crate::call_graph::{CallGraph, SymbolRef};
use crate::structure_index::{StructureCall, StructureIndex};

pub(crate) type Summaries<T> = HashMap<SymbolRef, Vec<T>>;

/// A resolved call to a summarized function.
pub(crate) struct SummarizedCall<'a, T> {
    pub(crate) file: usize,
    pub(crate) call: &'a StructureCall,
    /// The calling function; `None` for a call outside any function.
    pub(crate) caller: Option<SymbolRef>,
    pub(crate) target: SymbolRef,
    pub(crate) carried: &'a [T],
}

/// Every resolved call whose target has a summary.
pub(crate) fn summarized_calls<'a, T>(
    structure: &'a StructureIndex,
    graph: &'a CallGraph,
    summaries: &'a Summaries<T>,
) -> impl Iterator<Item = SummarizedCall<'a, T>> + 'a {
    structure
        .files()
        .iter()
        .enumerate()
        .flat_map(move |(file_index, file)| {
            file.calls
                .iter()
                .enumerate()
                .filter_map(move |(call_index, call)| {
                    let target = graph.target(file_index, call_index)?;
                    Some(SummarizedCall {
                        file: file_index,
                        call,
                        caller: call.owner.map(|owner| (file_index, owner as usize)),
                        target,
                        carried: summaries.get(&target)?,
                    })
                })
        })
}

/// Each function's summary through at most `max_depth` calls: `own`, grown
/// one call deeper per round by carrying each callee's summary through
/// `through` into its caller's, kept by `add`, until nothing changes.
pub(crate) fn summarize<T: Clone>(
    (structure, graph): (&StructureIndex, &CallGraph),
    own: &Summaries<T>,
    max_depth: usize,
    through: impl Fn(&SummarizedCall<'_, T>, &T) -> Option<T>,
    add: fn(&mut Vec<T>, T),
) -> Summaries<T> {
    let mut summaries = own.clone();
    for _ in 1..max_depth {
        let mut next = own.clone();
        for site in summarized_calls(structure, graph, &summaries) {
            let Some(caller) = site.caller.filter(|caller| *caller != site.target) else {
                continue;
            };
            for carried in site.carried {
                if let Some(carried) = through(&site, carried) {
                    add(next.entry(caller).or_default(), carried);
                }
            }
        }
        let settled = same_shape(&next, &summaries);
        summaries = next;
        if settled {
            break;
        }
    }
    summaries
}

/// Summaries only grow, so equal sizes mean nothing changed.
fn same_shape<T>(next: &Summaries<T>, previous: &Summaries<T>) -> bool {
    next.len() == previous.len()
        && next.iter().all(|(key, carried)| {
            previous
                .get(key)
                .is_some_and(|old| old.len() == carried.len())
        })
}

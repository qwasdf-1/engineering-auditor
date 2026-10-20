//! Effects that calls inherit from the repository functions they invoke.
//! Findings-free.
//!
//! Each function's summary is the inheritable effects it performs directly,
//! plus those of the functions it calls, through at most [`MAX_DEPTH`]
//! resolved calls. A call to a summarized function then carries the
//! summary's effects at the call site, in the caller's loop and async
//! context. Only effect kinds whose owners need no artifact, subject, or
//! lineage facts are inherited.

use std::collections::HashMap;

use crate::call_graph::{CallGraph, SymbolRef};
use crate::structure_index::StructureIndex;

use super::systems_effects::inherited_effect;
use super::systems_facts::{EffectKind, SystemEffect};

const INHERITED: [EffectKind; 2] = [EffectKind::ExternalIo, EffectKind::BlockingIo];

/// Calls followed from a call site to the function performing the effect.
const MAX_DEPTH: usize = 4;

#[derive(Clone)]
struct Carried {
    kind: EffectKind,
    operation: String,
    /// Called functions from the summarized one inward, as `name (path:line)`.
    via: Vec<String>,
}

/// The inherited effects of each file's calls, indexed like `direct`.
pub(super) fn inherited_effects(
    structure: &StructureIndex,
    graph: &CallGraph,
    direct: &[Vec<SystemEffect>],
) -> Vec<Vec<SystemEffect>> {
    let summaries = summarize(structure, graph, direct);
    structure
        .files()
        .iter()
        .enumerate()
        .map(|(file_index, file)| {
            file.calls
                .iter()
                .enumerate()
                .filter_map(|(call_index, call)| {
                    let target = graph.target(file_index, call_index)?;
                    let carried = summaries.get(&target)?;
                    let callee = describe(structure, target);
                    Some(carried.iter().filter_map(move |carried| {
                        let via = std::iter::once(callee.clone())
                            .chain(carried.via.iter().cloned())
                            .collect::<Vec<_>>()
                            .join(" -> ");
                        inherited_effect(file, call, carried.kind, &carried.operation, via)
                    }))
                })
                .flatten()
                .collect()
        })
        .collect()
}

type Summaries = HashMap<SymbolRef, Vec<Carried>>;

fn summarize(
    structure: &StructureIndex,
    graph: &CallGraph,
    direct: &[Vec<SystemEffect>],
) -> Summaries {
    let own = own_effects(direct);
    let mut summaries = own.clone();
    for _ in 1..MAX_DEPTH {
        let next = deepen(structure, graph, &own, &summaries);
        let settled = same_shape(&next, &summaries);
        summaries = next;
        if settled {
            break;
        }
    }
    summaries
}

/// The inheritable effects each function performs itself.
fn own_effects(direct: &[Vec<SystemEffect>]) -> Summaries {
    let mut own = Summaries::new();
    for (file_index, effects) in direct.iter().enumerate() {
        for effect in effects
            .iter()
            .filter(|effect| INHERITED.contains(&effect.kind))
        {
            if let Some(owner) = effect.owner {
                add(
                    own.entry((file_index, owner as usize)).or_default(),
                    Carried {
                        kind: effect.kind,
                        operation: effect.operation.clone(),
                        via: Vec::new(),
                    },
                );
            }
        }
    }
    own
}

/// Each function's own effects plus its callees' current summaries: one
/// more call deep than `summaries`.
fn deepen(
    structure: &StructureIndex,
    graph: &CallGraph,
    own: &Summaries,
    summaries: &Summaries,
) -> Summaries {
    let mut next = own.clone();
    for (file_index, file) in structure.files().iter().enumerate() {
        for (call_index, call) in file.calls.iter().enumerate() {
            let (Some(owner), Some(target)) = (call.owner, graph.target(file_index, call_index))
            else {
                continue;
            };
            let caller = (file_index, owner as usize);
            if target == caller {
                continue;
            }
            if let Some(carried) = summaries.get(&target) {
                let callee = describe(structure, target);
                let summary = next.entry(caller).or_default();
                for effect in carried {
                    add(summary, through(&callee, effect));
                }
            }
        }
    }
    next
}

fn through(callee: &str, effect: &Carried) -> Carried {
    Carried {
        kind: effect.kind,
        operation: effect.operation.clone(),
        via: std::iter::once(callee.to_string())
            .chain(effect.via.iter().cloned())
            .collect(),
    }
}

/// Summaries only grow, so equal sizes mean nothing changed.
fn same_shape(next: &Summaries, previous: &Summaries) -> bool {
    next.len() == previous.len()
        && next.iter().all(|(key, effects)| {
            previous
                .get(key)
                .is_some_and(|old| old.len() == effects.len())
        })
}

/// Keeps one carried effect per kind: the first, which is the shallowest.
fn add(summary: &mut Vec<Carried>, carried: Carried) {
    if !summary.iter().any(|existing| existing.kind == carried.kind) {
        summary.push(carried);
    }
}

fn describe(structure: &StructureIndex, (file_index, symbol_index): SymbolRef) -> String {
    let file = &structure.files()[file_index];
    let symbol = &file.symbols[symbol_index];
    format!("{} ({}:{})", symbol.name, file.path, symbol.line)
}

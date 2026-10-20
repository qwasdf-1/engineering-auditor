//! Effects that calls inherit from the repository functions they invoke.
//! Findings-free.
//!
//! Each function's summary is the inheritable effects it performs directly,
//! plus those of the functions it calls, through at most [`MAX_DEPTH`]
//! resolved calls. A call to a summarized function then carries the
//! summary's effects at the call site, in the caller's loop and async
//! context.
//!
//! Artifact reads and writes carry their artifact and value lineage
//! relative to the summarized function ([`super::systems_binding`]), so a
//! value local to a function on the call path pairs only with effects of
//! the same call.

use std::collections::HashMap;

use crate::call_graph::{CallGraph, SymbolRef};
use crate::structure_index::StructureIndex;

use super::systems_binding::{file_scopes, Artifact, FileScopes, Scope, Site, Value};
use super::systems_effects::{inherited_effect, BoundFacts};
use super::systems_facts::{EffectKind, SystemEffect};

const INHERITED: [EffectKind; 4] = [
    EffectKind::ExternalIo,
    EffectKind::BlockingIo,
    EffectKind::ArtifactRead,
    EffectKind::ArtifactWrite,
];

/// Calls followed from a call site to the function performing the effect.
const MAX_DEPTH: usize = 4;

/// Artifact effects one summary keeps; further ones are dropped.
const MAX_ARTIFACT_EFFECTS: usize = 8;

#[derive(Clone)]
struct Carried {
    kind: EffectKind,
    operation: String,
    /// Called functions from the summarized one inward, as `name (path:line)`.
    via: Vec<String>,
    artifact: Option<Artifact>,
    lineage: Option<Value>,
}

/// The inherited effects of each file's calls, indexed like `direct`.
pub(super) fn inherited_effects(
    structure: &StructureIndex,
    graph: &CallGraph,
    direct: &[Vec<SystemEffect>],
) -> Vec<Vec<SystemEffect>> {
    let scopes = file_scopes(structure);
    let summaries = summarize(structure, graph, &scopes, direct);
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
                    let site = Site::new(call, &scopes[file_index]);
                    Some(
                        carried
                            .iter()
                            .filter_map(|carried| {
                                let via = std::iter::once(callee.clone())
                                    .chain(carried.via.iter().cloned())
                                    .collect::<Vec<_>>()
                                    .join(" -> ");
                                let facts = bind_facts(carried, &site)?;
                                inherited_effect(
                                    file,
                                    call,
                                    (carried.kind, &carried.operation),
                                    facts,
                                    via,
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .flatten()
                .collect()
        })
        .collect()
}

/// A carried effect's artifact facts bound to the call site, or `None` for
/// an artifact effect whose value the call does not bind.
fn bind_facts(carried: &Carried, site: &Site) -> Option<BoundFacts> {
    if !is_artifact(carried.kind) {
        return Some(BoundFacts::default());
    }
    let mut facts = BoundFacts {
        value_lineage: Some(carried.lineage.as_ref()?.bind(site)?),
        ..BoundFacts::default()
    };
    if let Some(artifact) = &carried.artifact {
        let bound = artifact.bind(site);
        facts.artifact = Some(bound.text);
        facts.artifact_inputs = bound.inputs.into_iter().map(str::to_string).collect();
        facts.variant = bound.variant;
    }
    Some(facts)
}

type Summaries = HashMap<SymbolRef, Vec<Carried>>;

fn summarize(
    structure: &StructureIndex,
    graph: &CallGraph,
    scopes: &[FileScopes],
    direct: &[Vec<SystemEffect>],
) -> Summaries {
    let own = own_effects(structure, scopes, direct);
    let mut summaries = own.clone();
    for _ in 1..MAX_DEPTH {
        let next = deepen(structure, graph, scopes, (&own, &summaries));
        let settled = same_shape(&next, &summaries);
        summaries = next;
        if settled {
            break;
        }
    }
    summaries
}

fn scope<'a>(
    structure: &StructureIndex,
    scopes: &'a [FileScopes],
    function: SymbolRef,
) -> Scope<'a> {
    Scope::new(
        &scopes[function.0],
        function.1,
        describe(structure, function),
    )
}

/// The inheritable effects each function performs itself.
fn own_effects(
    structure: &StructureIndex,
    scopes: &[FileScopes],
    direct: &[Vec<SystemEffect>],
) -> Summaries {
    let mut own = Summaries::new();
    for (file_index, effects) in direct.iter().enumerate() {
        for effect in effects
            .iter()
            .filter(|effect| INHERITED.contains(&effect.kind))
        {
            let Some(owner) = effect.owner else {
                continue;
            };
            let function = (file_index, owner as usize);
            if let Some(carried) = own_effect(effect, &scope(structure, scopes, function)) {
                add(own.entry(function).or_default(), carried);
            }
        }
    }
    own
}

fn own_effect(effect: &SystemEffect, scope: &Scope) -> Option<Carried> {
    let mut carried = Carried {
        kind: effect.kind,
        operation: effect.operation.clone(),
        via: Vec::new(),
        artifact: None,
        lineage: None,
    };
    if is_artifact(effect.kind) {
        carried.lineage = Some(scope.value(effect.value_lineage.as_deref()?));
        carried.artifact = effect
            .artifact
            .as_ref()
            .map(|text| Artifact::new(text.clone(), scope, effect.loop_variant));
    }
    Some(carried)
}

/// Each function's own effects plus its callees' current summaries: one
/// more call deep than `summaries`.
fn deepen(
    structure: &StructureIndex,
    graph: &CallGraph,
    scopes: &[FileScopes],
    (own, summaries): (&Summaries, &Summaries),
) -> Summaries {
    let mut next = own.clone();
    for (file_index, file) in structure.files().iter().enumerate() {
        for (call_index, call) in file.calls.iter().enumerate() {
            let (Some(owner), Some(target)) = (call.owner, graph.target(file_index, call_index))
            else {
                continue;
            };
            let caller = (file_index, owner as usize);
            let Some(carried) = summaries.get(&target).filter(|_| target != caller) else {
                continue;
            };
            let callee = describe(structure, target);
            let site = Site::new(call, &scopes[file_index]);
            let caller_scope = scope(structure, scopes, caller);
            let summary = next.entry(caller).or_default();
            for effect in carried {
                if let Some(effect) = through(&callee, effect, &site, &caller_scope) {
                    add(summary, effect);
                }
            }
        }
    }
    next
}

/// A callee's carried effect as the caller carries it.
fn through(callee: &str, effect: &Carried, site: &Site, caller: &Scope) -> Option<Carried> {
    let mut carried = Carried {
        kind: effect.kind,
        operation: effect.operation.clone(),
        via: std::iter::once(callee.to_string())
            .chain(effect.via.iter().cloned())
            .collect(),
        artifact: None,
        lineage: None,
    };
    if is_artifact(effect.kind) {
        carried.lineage = Some(effect.lineage.as_ref()?.through(site, caller)?);
        carried.artifact = effect
            .artifact
            .as_ref()
            .map(|artifact| artifact.through(site, caller));
    }
    Some(carried)
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

/// Keeps one carried I/O effect per kind, the first and shallowest, and
/// distinct artifact effects up to [`MAX_ARTIFACT_EFFECTS`].
fn add(summary: &mut Vec<Carried>, carried: Carried) {
    let duplicate = summary.iter().any(|existing| {
        existing.kind == carried.kind
            && (!is_artifact(carried.kind)
                || (existing.lineage == carried.lineage && existing.artifact == carried.artifact))
    });
    let full = is_artifact(carried.kind)
        && summary
            .iter()
            .filter(|existing| is_artifact(existing.kind))
            .count()
            >= MAX_ARTIFACT_EFFECTS;
    if !duplicate && !full {
        summary.push(carried);
    }
}

fn is_artifact(kind: EffectKind) -> bool {
    matches!(kind, EffectKind::ArtifactRead | EffectKind::ArtifactWrite)
}

fn describe(structure: &StructureIndex, (file_index, symbol_index): SymbolRef) -> String {
    let file = &structure.files()[file_index];
    let symbol = &file.symbols[symbol_index];
    format!("{} ({}:{})", symbol.name, file.path, symbol.line)
}

#[cfg(test)]
#[path = "systems_inherited_tests.rs"]
mod tests;

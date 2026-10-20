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
//! relative to the summarized function: a value that is one of its
//! parameters binds to the call's argument at that position, and a value
//! local to a function on the call path keeps a name qualified by that
//! function, so it pairs only with effects of the same call.

use std::collections::{BTreeMap, HashMap};

use auditor_ast::SymbolKind;

use crate::call_graph::{CallGraph, SymbolRef};
use crate::structure_index::{StructureCall, StructureFile, StructureIndex};

use super::systems_effects::{
    assignment_aliases, call_arguments, inherited_effect, normalize_subject, simple_subject,
    BoundFacts,
};
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

/// A value relative to the summarized function.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    /// The function's parameter at this position, receivers excluded.
    Param(usize),
    /// A value local to a function on the call path, qualified by it.
    Local(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Artifact {
    text: String,
    /// Positions of the summarized function's parameters `text` names.
    params: Vec<usize>,
    /// `text` is exactly the parameter `params[0]`.
    is_param: bool,
    /// The artifact varies with a loop or argument on the call path.
    variant: bool,
}

#[derive(Clone)]
struct Carried {
    kind: EffectKind,
    operation: String,
    /// Called functions from the summarized one inward, as `name (path:line)`.
    via: Vec<String>,
    artifact: Option<Artifact>,
    lineage: Option<Value>,
}

/// Per-file facts the binding needs: assignment aliases and each
/// function's parameter names.
struct FileScopes {
    aliases: BTreeMap<String, String>,
    parameters: HashMap<u32, Vec<String>>,
}

/// The calling side of one call: its arguments and the caller's names.
struct Site<'a> {
    arguments: Vec<&'a str>,
    aliases: &'a BTreeMap<String, String>,
}

impl<'a> Site<'a> {
    fn new(call: &'a StructureCall, scopes: &'a FileScopes) -> Self {
        Self {
            arguments: call_arguments(call),
            aliases: &scopes.aliases,
        }
    }

    /// The argument bound to the parameter at `position`, unless a named
    /// argument makes the positional binding unknown.
    fn argument(&self, position: usize) -> Option<&'a str> {
        let argument = self.arguments.get(position)?.trim();
        (!is_named(argument)).then_some(argument)
    }

    /// The caller's variable passed as the parameter at `position`.
    fn subject(&self, position: usize) -> Option<String> {
        let argument = self.argument(position)?;
        let plain = argument
            .trim_start_matches(['&', '*'])
            .trim_start_matches("mut ")
            .trim();
        plain
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
            .then(|| simple_subject(plain))
            .flatten()
            .map(|subject| normalize_subject(&subject, self.aliases))
    }
}

/// The function a value is expressed relative to.
struct Scope<'a> {
    parameters: &'a [String],
    name: String,
}

impl Scope<'_> {
    fn value(&self, subject: &str) -> Value {
        self.parameters
            .iter()
            .position(|parameter| parameter == subject)
            .map_or_else(
                || Value::Local(format!("{}::{subject}", self.name)),
                Value::Param,
            )
    }
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
    let value_lineage = match carried.lineage.as_ref()? {
        Value::Local(token) => token.clone(),
        Value::Param(position) => site.subject(*position)?,
    };
    let mut facts = BoundFacts {
        value_lineage: Some(value_lineage),
        ..BoundFacts::default()
    };
    if let Some(artifact) = &carried.artifact {
        let (text, inputs, variant) = bind_artifact(artifact, site);
        facts.artifact = Some(text);
        facts.artifact_inputs = inputs.into_iter().map(str::to_string).collect();
        facts.variant = variant;
    }
    Some(facts)
}

/// The artifact's call-site text, the arguments it depends on, and whether
/// it varies regardless of the caller's loop.
fn bind_artifact<'a>(artifact: &Artifact, site: &Site<'a>) -> (String, Vec<&'a str>, bool) {
    let mut variant = artifact.variant;
    let mut inputs = Vec::new();
    for position in &artifact.params {
        match site.argument(*position) {
            Some(argument) => inputs.push(argument),
            None => variant = true,
        }
    }
    let text = match (artifact.is_param, inputs.first()) {
        (true, Some(argument)) => (*argument).to_string(),
        _ => artifact.text.clone(),
    };
    (text, inputs, variant)
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

fn file_scopes(structure: &StructureIndex) -> Vec<FileScopes> {
    structure
        .files()
        .iter()
        .map(|file| FileScopes {
            aliases: assignment_aliases(file),
            parameters: parameters(file),
        })
        .collect()
}

/// Each function's parameter names in order, without receivers.
fn parameters(file: &StructureFile) -> HashMap<u32, Vec<String>> {
    let mut parameters: HashMap<u32, Vec<String>> = HashMap::new();
    for symbol in &file.symbols {
        if symbol.kind != SymbolKind::Parameter
            || matches!(symbol.name.as_str(), "self" | "cls" | "this")
        {
            continue;
        }
        if let Some(parent) = symbol.parent {
            parameters
                .entry(parent)
                .or_default()
                .push(symbol.name.clone());
        }
    }
    parameters
}

fn scope<'a>(
    structure: &StructureIndex,
    scopes: &'a [FileScopes],
    function: SymbolRef,
) -> Scope<'a> {
    Scope {
        parameters: scopes[function.0]
            .parameters
            .get(&(function.1 as u32))
            .map_or(&[], Vec::as_slice),
        name: describe(structure, function),
    }
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
        carried.artifact = effect.artifact.as_ref().map(|text| {
            let params = parameter_positions(text, scope.parameters);
            Artifact {
                is_param: params.len() == 1 && scope.parameters[params[0]] == text.trim(),
                text: text.clone(),
                params,
                variant: effect.loop_variant,
            }
        });
    }
    Some(carried)
}

fn parameter_positions(text: &str, parameters: &[String]) -> Vec<usize> {
    let identifiers = crate::util::identifiers(text);
    parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| identifiers.contains(parameter))
        .map(|(position, _)| position)
        .collect()
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
        carried.lineage = Some(match effect.lineage.as_ref()? {
            Value::Local(token) => Value::Local(token.clone()),
            Value::Param(position) => caller.value(&site.subject(*position)?),
        });
        carried.artifact = effect.artifact.as_ref().map(|artifact| {
            let (text, inputs, variant) = bind_artifact(artifact, site);
            let params = parameter_positions(&inputs.join(", "), caller.parameters);
            Artifact {
                is_param: params.len() == 1 && caller.parameters[params[0]] == text.trim(),
                text,
                params,
                variant,
            }
        });
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

/// A keyword or named argument (`value=raw`, `value: raw`).
fn is_named(argument: &str) -> bool {
    let bytes = argument.as_bytes();
    argument
        .char_indices()
        .any(|(index, character)| match character {
            '=' => {
                bytes.get(index + 1) != Some(&b'=')
                    && index > 0
                    && !matches!(bytes[index - 1], b'=' | b'!' | b'<' | b'>')
            }
            ':' => bytes.get(index + 1) != Some(&b':') && (index == 0 || bytes[index - 1] != b':'),
            _ => false,
        })
}

fn describe(structure: &StructureIndex, (file_index, symbol_index): SymbolRef) -> String {
    let file = &structure.files()[file_index];
    let symbol = &file.symbols[symbol_index];
    format!("{} ({}:{})", symbol.name, file.path, symbol.line)
}

#[cfg(test)]
#[path = "systems_inherited_tests.rs"]
mod tests;

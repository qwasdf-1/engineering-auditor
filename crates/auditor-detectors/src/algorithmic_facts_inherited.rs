//! Sorts and read queries that calls inherit from the repository functions
//! they invoke. Findings-free.
//!
//! A function's summary lists the sorts of its parameters and the read
//! queries whose arguments name its parameters: its own, and those of the
//! functions it calls, through at most [`MAX_DEPTH`] resolved calls, each by
//! parameter position ([`crate::systems_binding`]). A call inside a loop then
//! repeats the operation when the bound argument is unchanged by the loop (a
//! sort) or derives from it (a query).

use std::collections::HashMap;
use std::iter::once;

use crate::call_graph::{describe, CallGraph};
use crate::call_summaries::{self, summarized_calls, SummarizedCall};
use crate::systems_binding::{file_scopes, FileScopes, Scope, Site, Value};

use super::*;

/// Calls followed from a call site to the function performing the operation.
const MAX_DEPTH: usize = 4;

/// Operations one summary keeps; further ones are dropped.
const MAX_OPERATIONS: usize = 8;

#[derive(Clone, PartialEq)]
enum Operation {
    Sort,
    Query { receiver: String, method: String },
}

#[derive(Clone)]
struct Carried {
    operation: Operation,
    /// Parameter positions: the sorted collection, or the query's inputs.
    params: Vec<usize>,
    /// Called functions from the summarized one inward, as `name (path:line)`.
    via: Vec<String>,
}

type Summaries = call_summaries::Summaries<Carried>;

/// The repeated sorts and queries of loop calls to summarized functions.
pub(super) fn inherited_facts(
    structure: &StructureIndex,
    graph: &CallGraph,
    facts: &mut Vec<AlgorithmicFact>,
) {
    let scopes = file_scopes(structure);
    let own = own_operations(structure, &scopes);
    let summaries = call_summaries::summarize(
        (structure, graph),
        &own,
        MAX_DEPTH,
        |site, carried| {
            let scope = Scope::new(&scopes[site.file], site.caller?.1, String::new());
            let binding = Site::new(site.call, &scopes[site.file]);
            through(&describe(structure, site.target), carried, &binding, &scope)
        },
        add,
    );
    for site in summarized_calls(structure, graph, &summaries) {
        let file = &structure.files()[site.file];
        let Some((_, owner)) = site.caller.filter(|_| file.emittable) else {
            continue;
        };
        if !file.symbols[owner].in_test {
            facts.extend(call_facts(structure, &scopes, &site, owner as u32));
        }
    }
}

/// The first repeated sort and the first repeated query of one call.
fn call_facts(
    structure: &StructureIndex,
    scopes: &[FileScopes],
    site: &SummarizedCall<'_, Carried>,
    owner: u32,
) -> Vec<AlgorithmicFact> {
    let file = &structure.files()[site.file];
    let aliases = aliases_for(file, owner);
    let binding = Site::new(site.call, &scopes[site.file]);
    let callee = describe(structure, site.target);
    let mut facts: Vec<AlgorithmicFact> = Vec::new();
    for carried in site.carried {
        let sort = carried.operation == Operation::Sort;
        if facts
            .iter()
            .any(|fact| matches!(fact.kind, AlgorithmicFactKind::RepeatedSort { .. }) == sort)
        {
            continue;
        }
        let Some(kind) = repeated(file, (owner, &aliases), site.call, &binding, carried) else {
            continue;
        };
        facts.push(AlgorithmicFact {
            path: file.path.clone(),
            function: file.symbols[owner as usize].name.clone(),
            line: site.call.line,
            kind,
            via: Some(chain(&callee, &carried.via).join(" -> ")),
        });
    }
    facts
}

/// The carried operation as the loop around `call` repeats it, if it does.
fn repeated(
    file: &StructureFile,
    (owner, aliases): (u32, &BTreeMap<String, String>),
    call: &StructureCall,
    site: &Site,
    carried: &Carried,
) -> Option<AlgorithmicFactKind> {
    match &carried.operation {
        Operation::Sort => {
            let root = canonical(&site.subject(carried.params[0])?, aliases);
            repeated_sort(file, owner, aliases, call, root)
        }
        Operation::Query { receiver, method } => {
            let arguments = bound_arguments(site, &carried.params)?;
            let (loop_fact, binding) = dependent_loop_binding(file, owner, call, &arguments)?;
            Some(AlgorithmicFactKind::QueryInLoop {
                receiver: receiver.clone(),
                method: method.clone(),
                binding,
                arguments,
                loop_span: loop_fact.span,
                call_span: call.span,
            })
        }
    }
}

/// The call's arguments at `params`, unless one is unbound.
fn bound_arguments(site: &Site, params: &[usize]) -> Option<String> {
    params
        .iter()
        .map(|position| site.argument(*position))
        .collect::<Option<Vec<_>>>()
        .map(|arguments| arguments.join(", "))
}

/// The sorts and queries on parameters each function performs itself.
fn own_operations(structure: &StructureIndex, scopes: &[FileScopes]) -> Summaries {
    let mut own = Summaries::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        let mut aliases: HashMap<u32, BTreeMap<String, String>> = HashMap::new();
        for call in &file.calls {
            let Some(owner) = call.owner else {
                continue;
            };
            // Aliases change a call's receiver, not its method.
            let candidate = call_shape(call, &BTreeMap::new()).is_some_and(|shape| {
                is_default_sort(&shape.method, &shape.arguments) || is_read_query(&shape.method)
            });
            if !candidate
                || file
                    .symbols
                    .get(owner as usize)
                    .is_none_or(|symbol| symbol.in_test)
            {
                continue;
            }
            let aliases = aliases
                .entry(owner)
                .or_insert_with(|| aliases_for(file, owner));
            let scope = Scope::new(&scopes[file_index], owner as usize, String::new());
            if let Some(carried) = own_operation(file, (owner, aliases), call, &scope) {
                add(
                    own.entry((file_index, owner as usize)).or_default(),
                    carried,
                );
            }
        }
    }
    own
}

fn own_operation(
    file: &StructureFile,
    (owner, aliases): (u32, &BTreeMap<String, String>),
    call: &StructureCall,
    scope: &Scope,
) -> Option<Carried> {
    let shape = call_shape(call, aliases)?;
    let (operation, params) = if is_default_sort(&shape.method, &shape.arguments) {
        let Value::Param(position) = scope.value(&shape.root) else {
            return None;
        };
        (Operation::Sort, vec![position])
    } else if is_read_query(&shape.method) && query_receiver_is_proven(file, owner, &shape.root) {
        let params = scope.positions(&shape.arguments);
        if params.is_empty() {
            return None;
        }
        let operation = Operation::Query {
            receiver: shape.root,
            method: shape.method,
        };
        (operation, params)
    } else {
        return None;
    };
    Some(Carried {
        operation,
        params,
        via: Vec::new(),
    })
}

/// A callee's carried operation as the caller carries it: only while its
/// inputs are still the caller's parameters.
fn through(callee: &str, carried: &Carried, site: &Site, caller: &Scope) -> Option<Carried> {
    let params = match carried.operation {
        Operation::Sort => match caller.value(&site.subject(carried.params[0])?) {
            Value::Param(position) => vec![position],
            Value::Local(_) => return None,
        },
        Operation::Query { .. } => {
            let params = caller.positions(&bound_arguments(site, &carried.params)?);
            if params.is_empty() {
                return None;
            }
            params
        }
    };
    Some(Carried {
        operation: carried.operation.clone(),
        params,
        via: chain(callee, &carried.via),
    })
}

/// Keeps distinct operations, the first and shallowest, up to
/// [`MAX_OPERATIONS`].
fn add(summary: &mut Vec<Carried>, carried: Carried) {
    let duplicate = summary.iter().any(|existing| {
        existing.operation == carried.operation && existing.params == carried.params
    });
    if !duplicate && summary.len() < MAX_OPERATIONS {
        summary.push(carried);
    }
}

/// `callee` followed by the functions it reaches the operation through.
fn chain(callee: &str, via: &[String]) -> Vec<String> {
    once(callee.to_string())
        .chain(via.iter().cloned())
        .collect()
}

#[cfg(test)]
#[path = "algorithmic_facts_inherited_tests.rs"]
mod tests;

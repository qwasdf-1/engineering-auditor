//! Random helpers judged by their callers. A function that returns a
//! predictable random value (`randomString(n)`, `uniqueID()`) is reported at
//! each resolved call whose result becomes a security value
//! (`const sessionToken = randomString(32)`), with the helper in `via`
//! evidence. A caller that returns the helper's result is a helper too,
//! through up to four calls. Calls resolve only to certain targets
//! (`call_graph.rs`).

use std::collections::{HashMap, HashSet};

use auditor_ast::AstNode;
use auditor_core::Finding;

use crate::ast_scan::{self, AstScan};
use crate::call_graph::{describe, CallGraph};
use crate::finding_via::through_calls;
use crate::security_random_context::{random_use, RandomUse};
use crate::structure_index::StructureIndex;
use crate::{util, Language};

/// Most calls followed from a random source to its security use.
const MAX_DEPTH: usize = 4;

const CALL_KINDS: &[&str] = &[
    "call_expression",
    "call",
    "method_invocation",
    "invocation_expression",
];

/// Functions returning a predictable random value, by (path, start byte),
/// with the helpers beneath each as a `via` chain.
#[derive(Default)]
pub(crate) struct RandomHelpers {
    functions: HashMap<(String, usize), Option<String>>,
}

impl RandomHelpers {
    pub(crate) fn add(&mut self, path: &str, function: AstNode<'_>) {
        self.functions
            .entry((path.to_string(), function.start_byte()))
            .or_insert(None);
    }
}

/// A resolved call to a random helper: its span in the caller, and the
/// helpers it reaches as a `via` chain.
struct HelperCall {
    start: usize,
    end: usize,
    via: String,
}

/// Findings at the calls whose random result becomes a security value.
/// `finding` makes the finding for a call at (path, line, pattern).
pub(crate) fn caller_findings(
    mut scan: AstScan<'_, '_>,
    (structure, graph): (&StructureIndex, &CallGraph),
    supports: impl Fn(&Language) -> bool,
    helpers: RandomHelpers,
    finding: impl Fn(&str, usize, &str) -> Finding,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen = HashSet::new();
    let mut frontier = helpers.functions;
    for _ in 0..MAX_DEPTH {
        if frontier.is_empty() {
            break;
        }
        seen.extend(frontier.keys().cloned());
        let calls = calls_to(structure, graph, &frontier);
        let mut next = HashMap::new();
        ast_scan::visit_ast_files(scan.reborrow(), &supports, |file, tree, _| {
            let rel = util::path_slash(&file.relative);
            for call in calls.get(&rel).into_iter().flatten() {
                let Some(node) = call_at(tree.root_node(), call.start, call.end) else {
                    continue;
                };
                match random_use(node) {
                    RandomUse::Security => findings.push(through_calls(
                        finding(&rel, node.start_position().0 + 1, node.text()),
                        Some(&call.via),
                    )),
                    RandomUse::ReturnedFrom(function) => {
                        next.entry((rel.clone(), function.start_byte()))
                            .or_insert_with(|| Some(call.via.clone()));
                    }
                    RandomUse::Other => {}
                }
            }
        });
        frontier = next
            .into_iter()
            .filter(|(key, _)| !seen.contains(key))
            .collect();
    }
    findings
}

/// The resolved calls to the functions in `frontier`, by caller path.
fn calls_to(
    structure: &StructureIndex,
    graph: &CallGraph,
    frontier: &HashMap<(String, usize), Option<String>>,
) -> HashMap<String, Vec<HelperCall>> {
    let paths = frontier
        .keys()
        .map(|(path, _)| path.as_str())
        .collect::<HashSet<_>>();
    let mut targets = HashMap::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        if !paths.contains(file.path.as_str()) {
            continue;
        }
        for (symbol_index, symbol) in file.symbols.iter().enumerate() {
            let Some(beneath) = frontier.get(&(file.path.clone(), symbol.span.start_byte)) else {
                continue;
            };
            let head = describe(structure, (file_index, symbol_index));
            let via = match beneath {
                Some(beneath) => format!("{head} -> {beneath}"),
                None => head,
            };
            targets.insert((file_index, symbol_index), via);
        }
    }
    let mut calls: HashMap<String, Vec<HelperCall>> = HashMap::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        for (call_index, call) in file.calls.iter().enumerate() {
            let Some(via) = graph
                .target(file_index, call_index)
                .and_then(|target| targets.get(&target))
            else {
                continue;
            };
            calls
                .entry(file.path.clone())
                .or_default()
                .push(HelperCall {
                    start: call.span.start_byte,
                    end: call.span.end_byte,
                    via: via.clone(),
                });
        }
    }
    calls
}

/// The call node spanning `start..end`.
fn call_at(root: AstNode<'_>, start: usize, end: usize) -> Option<AstNode<'_>> {
    let mut current = root;
    loop {
        if current.start_byte() == start
            && current.end_byte() == end
            && CALL_KINDS.contains(&current.kind())
        {
            return Some(current);
        }
        current = current
            .named_children()
            .find(|child| child.start_byte() <= start && end <= child.end_byte())?;
    }
}

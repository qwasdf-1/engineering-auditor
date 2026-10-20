//! Regular expressions compiled by the repository functions a loop calls.
//! Findings-free.
//!
//! A function compiles a fixed expression when it makes an uncached call to
//! a recognized compile API whose pattern reads only its parameters and
//! names it never assigns. A function calling one, with arguments that read
//! only its own parameters and unassigned names, compiles one too, through at
//! most [`MAX_DEPTH`] resolved calls. A memoized function compiles nothing
//! after its first call. A loop calling a compiling function with
//! loop-invariant arguments compiles the same expression on every pass.

use std::iter::once;

use auditor_ast::{SourceSpan, SymbolKind};

use super::collectors::pure_proof;
use super::flow::{invariant_loop, lazily_cached};
use super::operations::recognized_regex_call;
use super::syntax::call_name;
use super::{FunctionView, OptimizationFact, OptimizationFactKind};
use crate::call_graph::{describe, CallGraph};
use crate::call_summaries::{self, summarized_calls, Summaries};
use crate::structure_index::{StructureCall, StructureIndex};
use crate::util;

/// Calls followed from a call site to the function compiling the expression.
const MAX_DEPTH: usize = 4;

#[derive(Clone)]
struct Compile {
    api: String,
    /// Called functions from the summarized one inward, as `name (path:line)`.
    via: Vec<String>,
}

/// The repeated compilations of loop calls to compiling functions.
pub(super) fn inherited_facts(
    structure: &StructureIndex,
    graph: &CallGraph,
    facts: &mut Vec<OptimizationFact>,
) {
    let own = own_compiles(structure);
    let summaries = call_summaries::summarize(
        (structure, graph),
        &own,
        MAX_DEPTH,
        |site, compile| {
            let view = view(structure, site.caller?)?;
            fixed(&view, site.call).then(|| Compile {
                api: compile.api.clone(),
                via: once(describe(structure, site.target))
                    .chain(compile.via.iter().cloned())
                    .collect(),
            })
        },
        |summary, compile| {
            if summary.is_empty() {
                summary.push(compile);
            }
        },
    );
    for site in summarized_calls(structure, graph, &summaries) {
        let (Some(view), Some(compile)) = (
            site.caller.and_then(|caller| view(structure, caller)),
            site.carried.first(),
        ) else {
            continue;
        };
        if !view.file.emittable || invariant_loop(&view, site.call).is_none() {
            continue;
        }
        if lazily_cached(&view, site.call) {
            continue;
        }
        let via = once(describe(structure, site.target))
            .chain(compile.via.iter().cloned())
            .collect::<Vec<_>>()
            .join(" -> ");
        facts.push(OptimizationFact {
            path: view.file.path.clone(),
            function: view.function.name.clone(),
            line: site.call.line,
            kind: OptimizationFactKind::LoopInvariantRegex {
                api: compile.api.clone(),
            },
            proof: pure_proof(
                "lazy_first_use_cache",
                "preserve_zero_iteration_and_first_failure",
                "regular_expression",
            ),
            via: Some(via),
        });
    }
}

/// The fixed expressions each function compiles itself.
fn own_compiles(structure: &StructureIndex) -> Summaries<Compile> {
    let mut own = Summaries::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        for call in &file.calls {
            let Some(view) = call
                .owner
                .and_then(|owner| view(structure, (file_index, owner as usize)))
            else {
                continue;
            };
            if recognized_regex_call(&view, call)
                && !lazily_cached(&view, call)
                && fixed(&view, call)
            {
                let compile = Compile {
                    api: call_name(call),
                    via: Vec::new(),
                };
                own.entry((file_index, view.owner as usize))
                    .or_insert_with(|| vec![compile]);
            }
        }
    }
    own
}

/// A function that can carry a compilation: not a test, and not memoized.
fn view(
    structure: &StructureIndex,
    (file_index, owner): (usize, usize),
) -> Option<FunctionView<'_>> {
    let file = &structure.files()[file_index];
    let function = file.symbols.get(owner)?;
    (!function.in_test && !function.memoized).then_some(FunctionView {
        file,
        owner: owner as u32,
        function,
    })
}

/// `call` passes plain values that read only the function's parameters and
/// names it never assigns, so equal arguments give equal operands on every
/// call.
fn fixed(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    let operands = util::identifiers(&call.expression);
    plain_arguments(view, call)
        && !view.file.assignments.iter().any(|assignment| {
            assignment.owner == Some(view.owner)
                && !contains(assignment.span, call.span)
                && util::identifiers(&assignment.target)
                    .iter()
                    .any(|target| operands.contains(target))
        })
}

/// Arguments that are values rather than reads of state that may change
/// between calls: no nested call, and no member of `self`, `this`, or a
/// parameter (`cq.remainder()`, `self.prefix`).
fn plain_arguments(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    let mut quotes = util::QuoteState::default();
    let code = call
        .arguments
        .as_deref()
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if quotes.consumes(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let code = code.trim().trim_start_matches('(').trim_end_matches(')');
    let stateful = |root: &str| {
        matches!(root, "self" | "this")
            || view.file.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Parameter
                    && symbol.parent == Some(view.owner)
                    && symbol.name == root
            })
    };
    !code.contains('(')
        && !code
            .split(|character: char| !(character.is_alphanumeric() || "_.".contains(character)))
            .filter_map(|token| token.split_once('.'))
            .any(|(root, _)| stateful(root))
}

fn contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && inner.end_byte <= outer.end_byte
}

#[cfg(test)]
#[path = "optimization_fact_inherited_tests.rs"]
mod tests;

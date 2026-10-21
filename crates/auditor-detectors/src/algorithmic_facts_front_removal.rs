//! Front removal from an array-backed sequence inside a loop: every removal
//! shifts the remaining elements, so draining `n` items costs O(n^2).
//! Python `list.pop(0)`, JavaScript `array.shift()`, Java
//! `ArrayList.remove(0)`, C# `List<T>.RemoveAt(0)`, Rust `Vec::remove(0)`. A deque, linked list, or
//! queue removes from the front in O(1), and Go's `xs = xs[1:]` reslices.

use std::collections::BTreeMap;

use auditor_ast::SymbolKind;

use super::*;
use crate::Language;

pub(super) fn collect_front_removals(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, aliases) else {
            continue;
        };
        // A local sequence: a field's declaration is out of this function's
        // sight.
        if shape.collection != shape.root
            || !removes_front(&file.language, &shape.method, &shape.arguments)
        {
            continue;
        }
        let Some(loop_fact) = nearest_loop(file, owner, call.control_region) else {
            continue;
        };
        if declared_within(file, &shape.root, loop_fact.span)
            || bounded_loop(loop_fact)
            || !array_backed(file, owner, &shape.root)
        {
            continue;
        }
        push_fact(
            file,
            function,
            call.line,
            AlgorithmicFactKind::FrontRemoval {
                receiver: shape.root,
                method: shape.method,
                loop_span: loop_fact.span,
                call_span: call.span,
            },
            facts,
        );
    }
}

fn removes_front(language: &Language, method: &str, arguments: &str) -> bool {
    match language {
        Language::Python => method == "pop" && arguments == "0",
        Language::JavaScript | Language::TypeScript => method == "shift" && arguments.is_empty(),
        // C# `RemoveAt(0)` reads as `remove(0)`.
        Language::Java | Language::Rust | Language::CSharp => {
            method == "remove" && arguments == "0"
        }
        _ => false,
    }
}

/// A loop over a small literal range (`for _ in range(3)`) removes a
/// constant number of elements.
fn bounded_loop(loop_fact: &StructureLoop) -> bool {
    loop_fact.iterable.as_deref().is_some_and(|iterable| {
        let compact = iterable.replace(' ', "");
        compact
            .strip_prefix("range(")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|count| count.parse::<usize>().ok())
            .is_some_and(|count| count <= 16)
    })
}

/// Whether the function declares or assigns `name` as an array-backed
/// sequence, and never as a deque, linked list, or queue.
fn array_backed(file: &StructureFile, owner: u32, name: &str) -> bool {
    let declarations = file
        .symbols
        .iter()
        .filter(|symbol| {
            matches!(symbol.kind, SymbolKind::Parameter | SymbolKind::Variable)
                && symbol.parent == Some(owner)
                && symbol.name == name
        })
        .filter_map(|symbol| symbol.declaration.as_deref());
    let assignments = file
        .assignments
        .iter()
        .filter(|assignment| {
            assignment.owner == Some(owner) && assignment_root(assignment).as_deref() == Some(name)
        })
        .filter_map(|assignment| assignment.value.as_deref());
    let texts = declarations.chain(assignments).collect::<Vec<_>>();
    let queue_like = texts.iter().any(|text| {
        let lower = text.to_ascii_lowercase();
        // Types, not names: a declaration's text names its variable too.
        ["deque", "linkedlist", "queue<", "queue(", "queue::"]
            .iter()
            .any(|marker| lower.contains(marker))
    });
    !queue_like
        && texts
            .iter()
            .any(|text| array_text(&file.language, text.trim()))
}

fn array_text(language: &Language, text: &str) -> bool {
    match language {
        Language::Python => {
            text.starts_with('[')
                || text.starts_with("list(")
                || text.contains(": list")
                || text.contains("list[")
                || text.contains("List[")
        }
        Language::JavaScript | Language::TypeScript => {
            text.starts_with('[')
                || text.starts_with("Array.from(")
                || text.starts_with("new Array")
                || text.contains("[]")
                || text.contains("Array<")
        }
        Language::Java => text.contains("ArrayList"),
        Language::CSharp => {
            text.starts_with("List<") || text.contains(" List<") || text.contains("[]")
        }
        Language::Rust => text.contains("Vec<") || text.contains("Vec::") || text.contains("vec!"),
        _ => false,
    }
}

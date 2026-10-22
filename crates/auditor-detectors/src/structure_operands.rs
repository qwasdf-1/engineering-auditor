//! Operand facts read from the syntax tree: index and slice expressions (Go,
//! Python, C#, C, and C++) and Python membership tests (`x in xs`, which calls
//! `xs.__contains__(x)`), each owned by its innermost function, and the
//! Python locals count, which assignments introduce rather than declarations.

use auditor_ast::{SourceSpan, SymbolKind};

use std::collections::{BTreeMap, BTreeSet};

use super::{
    compact_text, StructureAssignment, StructureFunctionMetrics, StructureLoop,
    StructureMembership, StructureSubscript, StructureSymbol,
};
use crate::Language;

pub(super) fn collect_subscripts(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    symbols: &[StructureSymbol],
) -> Vec<StructureSubscript> {
    let kinds: &[&str] = match file.language {
        Language::Go => &["index_expression", "slice_expression"],
        Language::Python => &["subscript"],
        Language::CSharp => &["element_access_expression"],
        Language::C | Language::Cpp => &["subscript_expression"],
        _ => return Vec::new(),
    };
    crate::ast_scan::nodes_of_kind(tree.root_node(), kinds)
        .into_iter()
        .filter_map(|node| {
            let (operand, index, bounds) = match file.language {
                Language::Python => python_subscript(node)?,
                Language::CSharp => csharp_subscript(node)?,
                _ => go_subscript(node)?,
            };
            let span = SourceSpan::from_node(node);
            Some(StructureSubscript {
                operand,
                index,
                bounds,
                owner: innermost_function(symbols, span),
                span,
            })
        })
        .collect()
}

type SubscriptParts = (String, Option<String>, (String, String));

fn go_subscript(node: auditor_ast::AstNode<'_>) -> Option<SubscriptParts> {
    let text = |field: &str| {
        node.child_by_field_name(field)
            .map(|child| compact_text(child.text()))
            .unwrap_or_default()
    };
    // A C or C++ subscript names its operand `argument`.
    let operand = node
        .child_by_field_name("operand")
        .or_else(|| node.child_by_field_name("argument"))?;
    let operand = compact_text(operand.text());
    Some(if node.kind() == "slice_expression" {
        (operand, None, (text("start"), text("end")))
    } else {
        (operand, Some(text("index")), Default::default())
    })
}

/// `xs[i]`, `xs[^1]`, or the range `xs[a..b]` with one argument.
fn csharp_subscript(node: auditor_ast::AstNode<'_>) -> Option<SubscriptParts> {
    let operand = compact_text(node.child_by_field_name("expression")?.text());
    let arguments = node
        .child_by_field_name("subscript")?
        .named_children()
        .filter(|child| child.kind() == "argument")
        .filter_map(|argument| argument.named_child(0));
    single_subscript(operand, arguments, "range_expression", "..")
}

/// `xs[0]`, `xs[-1]`, or `xs[a:b]` (a stepped slice is not a prefix).
fn python_subscript(node: auditor_ast::AstNode<'_>) -> Option<SubscriptParts> {
    let operand = compact_text(node.child_by_field_name("value")?.text());
    let indices = node
        .named_children()
        .skip(1)
        .filter(|child| child.kind() != "comment");
    single_subscript(operand, indices, "slice", ":")
}

/// The one index of a subscript, or the start and end of its range (a
/// `range_kind` node with exactly two `separator`-split bounds).
fn single_subscript<'a>(
    operand: String,
    mut indices: impl Iterator<Item = auditor_ast::AstNode<'a>>,
    range_kind: &str,
    separator: &str,
) -> Option<SubscriptParts> {
    let index = indices.next()?;
    if indices.next().is_some() {
        return None;
    }
    if index.kind() != range_kind {
        return Some((
            operand,
            Some(compact_text(index.text())),
            Default::default(),
        ));
    }
    let bounds = compact_text(index.text());
    let parts = bounds.split(separator).collect::<Vec<_>>();
    let [start, end] = parts.as_slice() else {
        return None;
    };
    Some((operand, None, (start.to_string(), end.to_string())))
}

/// Python `needle in collection` and `needle not in collection` tests.
pub(super) fn collect_memberships(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    symbols: &[StructureSymbol],
) -> Vec<StructureMembership> {
    if file.language != Language::Python {
        return Vec::new();
    }
    crate::ast_scan::nodes_of_kind(tree.root_node(), &["comparison_operator"])
        .into_iter()
        .filter_map(|node| {
            let operands = node
                .named_children()
                .filter(|child| child.kind() != "comment")
                .collect::<Vec<_>>();
            let [needle, collection] = operands.as_slice() else {
                return None;
            };
            let operator = node
                .children()
                .filter(|child| !child.is_named())
                .map(|child| child.text())
                .collect::<Vec<_>>()
                .join(" ");
            matches!(operator.as_str(), "in" | "not in").then_some(())?;
            let span = SourceSpan::from_node(node);
            Some(StructureMembership {
                needle: compact_text(needle.text()),
                collection: compact_text(collection.text()),
                owner: innermost_function(symbols, span),
                span,
                negated: operator == "not in",
            })
        })
        .collect()
}

fn innermost_function(symbols: &[StructureSymbol], span: SourceSpan) -> Option<u32> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                && symbol.span.start_byte <= span.start_byte
                && span.end_byte <= symbol.span.end_byte
        })
        .max_by_key(|(_, symbol)| symbol.span.start_byte)
        .map(|(index, _)| index as u32)
}

/// Python declares no locals: a name assigned in a function is local to it.
/// Each function's count is its distinct assigned names and loop targets
/// (pylint `R0914` without the parameters, as the other languages count
/// declared locals).
pub(super) fn count_python_locals(
    assignments: &[StructureAssignment],
    loops: &[StructureLoop],
    metrics: &mut BTreeMap<u32, StructureFunctionMetrics>,
) {
    let mut names = BTreeMap::<u32, BTreeSet<&str>>::new();
    let targets = assignments
        .iter()
        .filter_map(|assignment| Some((assignment.owner?, assignment.target.as_str())))
        .chain(loops.iter().flat_map(|loop_fact| {
            loop_fact
                .binding
                .iter()
                .filter_map(move |binding| Some((loop_fact.owner?, binding.as_str())))
        }));
    for (owner, target) in targets {
        for target in target.trim_matches(['(', ')', '[', ']']).split(',') {
            let target = target.trim().trim_start_matches('*');
            if !target.is_empty()
                && target
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
            {
                names.entry(owner).or_default().insert(target);
            }
        }
    }
    for (owner, names) in names {
        if let Some(metric) = metrics.get_mut(&owner) {
            metric.locals = names.len();
        }
    }
}

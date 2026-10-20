//! Syntax-tree queries for recovery: error nodes, the declarations holding
//! them, and functions a quarantine cuts.

use std::ops::Range;

use tree_sitter::Node;

/// Nodes whose children are independent declarations.
const CONTAINER_KINDS: &[&str] = &[
    "class_body",
    "interface_body",
    "enum_body",
    "enum_class_body",
    "object_type",
    "declaration_list",
    "field_declaration_list",
    "preproc_if",
    "preproc_ifdef",
    "preproc_elif",
    "preproc_elifdef",
    "preproc_else",
];

/// The declarations holding a function that overlaps a quarantined range.
/// An error can split a function into top-level fragments (`#ifdef` inside
/// its body), so the quarantined fragment is part of a function the
/// reparse rejoins; the whole function goes instead.
pub(super) fn cut_functions(root: Node<'_>, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let kinds = crate::languages::function_node_kinds();
    let mut units = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let span = node.byte_range();
        if !ranges
            .iter()
            .any(|range| range.start < span.end && span.start < range.end)
        {
            continue;
        }
        if kinds.contains(&node.kind()) {
            units.push(unit(node));
            continue;
        }
        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor));
    }
    merged(units)
}

pub(super) fn error_count(root: Node<'_>) -> usize {
    error_nodes(root).len()
}

pub(super) fn has_error_within(root: Node<'_>, range: &Range<usize>) -> bool {
    error_nodes(root).iter().any(|node| {
        let span = node.byte_range();
        span.start < range.end && range.start < span.end.max(span.start + 1)
    })
}

fn error_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut errors = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_error() || node.is_missing() {
            errors.push(node);
            continue;
        }
        let mut cursor = node.walk();
        pending.extend(
            node.children(&mut cursor)
                .filter(|child| child.has_error() || child.is_missing()),
        );
    }
    errors
}

/// The declarations holding each error and missing node, merged.
pub(super) fn error_units(root: Node<'_>) -> Vec<Range<usize>> {
    merged(error_nodes(root).into_iter().map(unit).collect())
}

/// The top-level item or member declaration containing `node`.
fn unit(node: Node<'_>) -> Range<usize> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.parent().is_none() || is_container(parent) {
            break;
        }
        current = parent;
    }
    current.byte_range()
}

fn is_container(node: Node<'_>) -> bool {
    CONTAINER_KINDS.contains(&node.kind())
        // A Python class body.
        || (node.kind() == "block"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "class_definition"))
}

pub(super) fn merged(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

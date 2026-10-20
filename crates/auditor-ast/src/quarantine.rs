//! Recovery for sources a grammar rejects in places: each declaration that
//! holds a syntax error is quarantined, blanked with spaces so every byte
//! offset and line is kept, and the source parsed again.
//!
//! A quarantined declaration is a whole top-level item or a whole member of
//! a type, namespace, or preprocessor block, never part of a function body,
//! so every function left in the tree is complete: a function the reparse
//! finds overlapping a quarantined range is quarantined whole. Recovery fails, and the
//! file stays unparsed, when errors remain after [`MAX_ROUNDS`] or when the
//! quarantine would cover half the source.

use std::ops::Range;

use tree_sitter::{Node, Parser, Tree};

/// Rounds of quarantine and reparse: blanking one declaration can expose an
/// error the parser had absorbed into it, or rejoin a function it had split.
const MAX_ROUNDS: usize = 6;

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

/// A declaration removed from a recovered tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quarantine {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    /// The declaration's original text.
    pub text: Box<str>,
}

pub(crate) struct Recovered {
    pub(crate) tree: Tree,
    pub(crate) ranges: Vec<Range<usize>>,
}

/// `parsed` parsed again without its erroneous declarations, or `None` when
/// recovery fails. `tree` is `parsed` under `parser`'s current language.
pub(crate) fn recover(parser: &mut Parser, tree: &Tree, parsed: &str) -> Option<Recovered> {
    let mut text = parsed.as_bytes().to_vec();
    let mut tree = tree.clone();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let units = if tree.root_node().has_error() {
            error_units(tree.root_node())
        } else {
            cut_functions(tree.root_node(), &ranges)
        };
        if units.is_empty() {
            return (!tree.root_node().has_error()).then_some(Recovered { tree, ranges });
        }
        for range in units {
            blank(&mut text, range.clone());
            ranges.push(range);
        }
        ranges = merged(ranges);
        let covered = ranges.iter().map(|range| range.len()).sum::<usize>();
        if covered * 2 >= parsed.len() {
            return None;
        }
        // Blanking replaces whole UTF-8 sequences with ASCII spaces.
        tree = parser.parse(std::str::from_utf8(&text).ok()?, None)?;
    }
    None
}

/// The declarations holding a function that overlaps a quarantined range.
/// An error can split a function into top-level fragments (`#ifdef` inside
/// its body), so the quarantined fragment is part of a function the
/// reparse rejoins; the whole function goes instead.
fn cut_functions(root: Node<'_>, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
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

/// The quarantine records for `ranges` of `source`.
pub(crate) fn records(source: &str, ranges: &[Range<usize>]) -> Vec<Quarantine> {
    ranges
        .iter()
        .map(|range| Quarantine {
            start_byte: range.start,
            end_byte: range.end,
            start_line: line_of(source, range.start),
            end_line: line_of(source, range.end),
            text: source[range.clone()].into(),
        })
        .collect()
}

/// `source` with `ranges` blanked.
pub(crate) fn blanked(source: &str, ranges: &[Range<usize>]) -> String {
    let mut text = source.as_bytes().to_vec();
    for range in ranges {
        blank(&mut text, range.clone());
    }
    String::from_utf8(text).unwrap_or_else(|_| source.to_string())
}

fn line_of(source: &str, byte: usize) -> usize {
    source.as_bytes()[..byte.min(source.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn blank(text: &mut [u8], range: Range<usize>) {
    for byte in &mut text[range] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

/// The declarations holding each error and missing node, merged.
fn error_units(root: Node<'_>) -> Vec<Range<usize>> {
    let mut units = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_error() || node.is_missing() {
            units.push(unit(node));
            continue;
        }
        let mut cursor = node.walk();
        pending.extend(
            node.children(&mut cursor)
                .filter(|child| child.has_error() || child.is_missing()),
        );
    }
    merged(units)
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

fn merged(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
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

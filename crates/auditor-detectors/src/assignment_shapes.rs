//! Per-language assignment shapes for def-use rules: the plain store of a
//! value into one local, and the names local to the enclosing function.

use std::collections::BTreeSet;

use auditor_ast::{AstNode, Binding, BindingKind, FlowCache};

use crate::ast_scan;
use crate::{util, Language};

#[path = "assignment_stores.rs"]
mod stores;
#[path = "assignment_shapes_tier_two.rs"]
mod tier_two;

pub(crate) use stores::simple_assignment;

/// Function and closure node kinds, across the grammars.
const FUNCTIONS: &[&str] = &[
    "function_definition",
    "lambda",
    "function_item",
    "closure_expression",
    "function_declaration",
    "method_declaration",
    "constructor_declaration",
    "func_literal",
    "arrow_function",
    "function_expression",
    "function",
    "method_definition",
    "generator_function_declaration",
    "lambda_expression",
    "local_function_statement",
    "lambda_literal",
    "anonymous_function",
];

/// The names local to the function enclosing a block. A field, a package or
/// module variable, or a closure's captured variable may be read by a call
/// that never names it, so only locals are judged; top-level code has none.
pub(crate) enum LocalNames {
    /// Every assigned name except those a `global`/`nonlocal` statement
    /// binds outward (Python), or every name (Rust binds only locals).
    AllExcept(BTreeSet<String>),
    Declared(BTreeSet<String>),
}

impl LocalNames {
    pub(crate) fn of<'a>(
        language: &Language,
        block: AstNode<'a>,
        flows: &mut FlowCache<'a>,
    ) -> Option<Self> {
        let function = std::iter::successors(Some(block), |node| node.parent())
            .find(|node| FUNCTIONS.contains(&node.kind()))?;
        Some(match language {
            Language::Rust => Self::AllExcept(BTreeSet::new()),
            Language::Python => Self::AllExcept(
                ast_scan::nodes_of_kind(function, &["global_statement", "nonlocal_statement"])
                    .into_iter()
                    .flat_map(mentions)
                    .collect(),
            ),
            _ => Self::Declared(declared_names(language, function, flows.enclosing(block))),
        })
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            Self::AllExcept(outward) => !outward.contains(name),
            Self::Declared(declared) => declared.contains(name),
        }
    }
}

/// Names `function` declares, as the data-flow layer binds them: locals,
/// parameters, loop and catch variables, less those another name may alias
/// (C# by-reference parameters; C and C++ `volatile`, address-taken, and
/// reference-bound locals).
fn declared_names(
    language: &Language,
    function: AstNode<'_>,
    flow: &auditor_ast::FunctionFlow<'_>,
) -> BTreeSet<String> {
    let mut names = flow
        .bindings()
        .iter()
        .filter(|binding| {
            !matches!(
                binding.kind,
                BindingKind::Assignment | BindingKind::Augmented
            ) && function.start_byte() <= binding.node.start_byte()
                && binding.node.end_byte() <= function.end_byte()
                && !may_be_aliased(language, binding)
        })
        .map(|binding| binding.name.to_string())
        .collect::<BTreeSet<_>>();
    if matches!(language, Language::C | Language::Cpp) {
        for name in tier_two::aliased_names(function) {
            names.remove(&name);
        }
    }
    names
}

fn may_be_aliased(language: &Language, binding: &Binding<'_>) -> bool {
    match language {
        Language::CSharp => tier_two::is_by_reference_parameter(binding),
        Language::C | Language::Cpp => tier_two::is_volatile(binding),
        _ => false,
    }
}

/// The one local a store target names: an identifier, or a wrapper that
/// holds only one (Go's single-element `expression_list`, Rust's `mut x`,
/// Kotlin's assignable expression). A write through a pointer (`*p = 0`) or
/// a field is not a store to the local.
fn single_identifier(node: AstNode<'_>) -> Option<&str> {
    let is_name = |node: AstNode<'_>| matches!(node.kind(), "identifier" | "simple_identifier");
    if is_name(node) {
        return Some(node.text());
    }
    if !matches!(
        node.kind(),
        "expression_list" | "mut_pattern" | "directly_assignable_expression"
    ) {
        return None;
    }
    let mut children = node.named_children();
    let child = children.next()?;
    (children.next().is_none() && is_name(child)).then_some(child.text())
}

/// Every name a statement mentions: its identifiers (Kotlin's
/// `simple_identifier`, and the names of `"$x"` templates), and the `{name}`
/// captures of Rust format strings (`println!("{total}")`).
pub(crate) fn mentions(node: AstNode<'_>) -> BTreeSet<String> {
    const NAMES: &[&str] = &["identifier", "simple_identifier", "interpolated_identifier"];
    let mut names = BTreeSet::new();
    if NAMES.contains(&node.kind()) {
        names.insert(node.text().trim_start_matches('$').to_string());
    }
    names.extend(
        ast_scan::nodes_of_kind(node, NAMES)
            .into_iter()
            .map(|identifier| identifier.text().trim_start_matches('$').to_string()),
    );
    for literal in ast_scan::nodes_of_kind(node, &["string_literal", "raw_string_literal"]) {
        for capture in literal.text().split('{').skip(1) {
            let name = capture.split(['}', ':']).next().unwrap_or_default().trim();
            if !name.is_empty() && util::identifiers(name).len() == 1 {
                names.insert(name.to_string());
            }
        }
    }
    names
}

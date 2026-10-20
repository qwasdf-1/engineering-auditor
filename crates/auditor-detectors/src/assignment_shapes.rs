//! Per-language assignment shapes for def-use rules: the plain store of a
//! value into one local, and the names local to the enclosing function.

use std::collections::BTreeSet;

use auditor_ast::AstNode;

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
    pub(crate) fn of(language: &Language, block: AstNode<'_>) -> Option<Self> {
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
            _ => Self::Declared(declared_names(language, function)),
        })
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            Self::AllExcept(outward) => !outward.contains(name),
            Self::Declared(declared) => declared.contains(name),
        }
    }
}

/// Names a function declares: locals, parameters, loop and catch
/// variables.
fn declared_names(language: &Language, function: AstNode<'_>) -> BTreeSet<String> {
    let (named, listed): (&[&str], &[&str]) = match language {
        Language::C | Language::Cpp => return tier_two::c_declared_names(function),
        Language::Kotlin => return tier_two::kotlin_declared_names(function),
        Language::CSharp => return tier_two::csharp_declared_names(function),
        Language::Go => (
            &["var_spec", "parameter_declaration"],
            &["short_var_declaration", "range_clause"],
        ),
        Language::Java => (
            &[
                "variable_declarator",
                "formal_parameter",
                "catch_formal_parameter",
                "enhanced_for_statement",
            ],
            &["inferred_parameters"],
        ),
        _ => (
            &["variable_declarator"],
            &["formal_parameters", "catch_clause", "for_in_statement"],
        ),
    };
    declared_in(function, named, listed)
}

/// The `name` fields of the `named` declaration kinds, and every name the
/// `listed` kinds declare (a Go `a, b :=`, a loop variable, a catch
/// parameter).
fn declared_in(function: AstNode<'_>, named: &[&str], listed: &[&str]) -> BTreeSet<String> {
    let mut names = ast_scan::nodes_of_kind(function, named)
        .into_iter()
        .filter_map(|node| node.child_by_field_name("name"))
        .map(|name| name.text().to_string())
        .collect::<BTreeSet<_>>();
    for node in ast_scan::nodes_of_kind(function, listed) {
        let declared = match node.kind() {
            "short_var_declaration" | "range_clause" | "for_in_statement" | "foreach_statement" => {
                node.child_by_field_name("left")
            }
            "catch_clause" => node.child_by_field_name("parameter"),
            _ => Some(node),
        };
        if let Some(declared) = declared {
            names.extend(mentions(declared));
        }
    }
    names
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

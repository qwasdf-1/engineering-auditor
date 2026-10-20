//! Assignment shapes for the Tier-2 grammars (Kotlin, C#, C, C++): the
//! names a function declares, and the declarations that store one value.

use std::collections::BTreeSet;

use auditor_ast::AstNode;

use super::{declared_in, mentions};
use crate::{ast_scan, Language};

/// C# locals, catch and loop variables, and parameters other than `out`,
/// `ref`, and `in`, which are the caller's variables.
pub(super) fn csharp_declared_names(function: AstNode<'_>) -> BTreeSet<String> {
    let mut names = declared_in(
        function,
        &["variable_declarator", "catch_declaration"],
        &["foreach_statement"],
    );
    for parameter in ast_scan::nodes_of_kind(function, &["parameter"]) {
        let by_reference = parameter.named_children().any(|child| {
            child.kind() == "modifier" && matches!(child.text(), "out" | "ref" | "in")
        });
        if let Some(name) = parameter
            .child_by_field_name("name")
            .filter(|_| !by_reference)
        {
            names.insert(name.text().to_string());
        }
    }
    names
}

/// C and C++ locals and parameters. A local whose address is taken, that a
/// reference binds, or that is `volatile` may be read or written behind the
/// name's back, so it is not judged.
pub(super) fn c_declared_names(function: AstNode<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for declaration in ast_scan::nodes_of_kind(function, &["declaration", "parameter_declaration"])
    {
        if declaration
            .named_children()
            .any(|child| child.kind() == "type_qualifier" && child.text() == "volatile")
        {
            continue;
        }
        names.extend(
            declaration
                .named_children()
                .filter(|child| {
                    child.kind().ends_with("declarator") || child.kind() == "identifier"
                })
                .filter_map(c_declarator_name),
        );
    }
    for name in aliased_names(function) {
        names.remove(&name);
    }
    names
}

/// The local a declarator binds, through `*`, `&`, array, and initializer
/// wrappers.
fn c_declarator_name(declarator: AstNode<'_>) -> Option<String> {
    let declarator = declarator
        .child_by_field_name("declarator")
        .filter(|_| declarator.kind() == "init_declarator")
        .unwrap_or(declarator);
    std::iter::successors(Some(declarator), |node| {
        node.child_by_field_name("declarator")
    })
    .find(|node| node.kind() == "identifier")
    .map(|name| name.text().to_string())
}

/// Names whose address is taken (`&x`) or that a reference binds
/// (`int &r = x`).
fn aliased_names(function: AstNode<'_>) -> BTreeSet<String> {
    ast_scan::nodes_of_kind(function, &["pointer_expression", "reference_declarator"])
        .into_iter()
        .filter_map(|node| match node.kind() {
            "reference_declarator" => node
                .parent()
                .and_then(|declarator| declarator.child_by_field_name("value")),
            _ if node.text().starts_with('&') => node.child_by_field_name("argument"),
            _ => None,
        })
        .flat_map(mentions)
        .collect()
}

/// Kotlin `val`/`var` locals, parameters, and catch variables.
pub(super) fn kotlin_declared_names(function: AstNode<'_>) -> BTreeSet<String> {
    ast_scan::nodes_of_kind(
        function,
        &["variable_declaration", "parameter", "catch_block"],
    )
    .into_iter()
    .filter_map(|node| {
        node.named_children()
            .find(|child| child.kind() == "simple_identifier")
    })
    .map(|name| name.text().to_string())
    .collect()
}

/// `(target, value)` of a Tier-2 declaration or Kotlin assignment that
/// stores one value; plain C-family expression statements are shared with
/// the Tier-1 grammars.
pub(super) fn declared_store<'a>(
    language: &Language,
    node: AstNode<'a>,
) -> Option<(AstNode<'a>, AstNode<'a>)> {
    match (language, node.kind()) {
        (Language::CSharp, "local_declaration_statement") => csharp_store(node),
        (Language::C | Language::Cpp, "declaration") => c_store(node),
        (Language::Kotlin, "property_declaration") => {
            crate::kotlin_api_identity::kotlin_property_store(node)
        }
        (Language::Kotlin, "assignment") => {
            let operator = node.children().find(|child| !child.is_named())?;
            (operator.text() == "=").then_some(())?;
            Some((node.named_child(0)?, node.named_child(1)?))
        }
        _ => None,
    }
}

fn csharp_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    let declaration = node
        .named_children()
        .find(|child| child.kind() == "variable_declaration")?;
    let declarator = ast_scan::sole_child_of_kind(declaration, "variable_declarator")?;
    let name = declarator.child_by_field_name("name")?;
    let value = declarator
        .named_children()
        .filter(|child| child.start_byte() != name.start_byte())
        .last()?;
    Some((name, value))
}

fn c_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    let declarator = ast_scan::sole_child_of_kind(node, "init_declarator")?;
    let value = declarator.child_by_field_name("value")?;
    // A dead initialization to a constant (`int rc = 0;`, `p = NULL`) is
    // defensive programming (clang `deadcode.DeadStores`).
    if matches!(
        value.kind(),
        "number_literal" | "null" | "nullptr" | "true" | "false" | "char_literal"
    ) || value.text() == "NULL"
    {
        return None;
    }
    Some((declarator.child_by_field_name("declarator")?, value))
}

//! Assignment shapes for the Tier-2 grammars (Kotlin, C#, C, C++): the
//! locals another name may alias, and the declarations that store one value.

use std::collections::BTreeSet;

use auditor_ast::{AstNode, Binding, BindingKind};

use super::mentions;
use crate::{ast_scan, Language};

/// A C# `out`, `ref`, or `in` parameter, which is the caller's variable.
pub(super) fn is_by_reference_parameter(binding: &Binding<'_>) -> bool {
    binding.kind == BindingKind::Parameter
        && binding
            .node
            .named_children()
            .any(|child| child.kind() == "modifier" && matches!(child.text(), "out" | "ref" | "in"))
}

/// A C or C++ local or parameter declared `volatile`.
pub(super) fn is_volatile(binding: &Binding<'_>) -> bool {
    std::iter::successors(Some(binding.node), AstNode::parent)
        .take(3)
        .find(|node| matches!(node.kind(), "declaration" | "parameter_declaration"))
        .is_some_and(|declaration| {
            declaration
                .named_children()
                .any(|child| child.kind() == "type_qualifier" && child.text() == "volatile")
        })
}

/// Names whose address is taken (`&x`) or that a reference binds
/// (`int &r = x`).
pub(super) fn aliased_names(function: AstNode<'_>) -> BTreeSet<String> {
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

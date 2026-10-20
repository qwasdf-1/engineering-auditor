//! Plain stores of one value into one local, per grammar: the shapes the
//! def-use rules read as a write that a later read must justify.

use auditor_ast::AstNode;

use super::{single_identifier, tier_two};
use crate::{ast_scan, Language};

/// `(target, value)` of a plain assignment of a value to one local.
pub(crate) fn simple_assignment<'a>(
    language: &Language,
    node: AstNode<'a>,
) -> Option<(&'a str, AstNode<'a>)> {
    let (left, right) = match language {
        Language::Go => go_store(node),
        Language::Python => python_store(node),
        Language::Rust => rust_store(node),
        Language::JavaScript | Language::TypeScript | Language::Java => {
            declarator_store(node).or_else(|| expression_store(node))
        }
        Language::CSharp | Language::C | Language::Cpp | Language::Kotlin => {
            tier_two::declared_store(language, node).or_else(|| expression_store(node))
        }
        _ => None,
    }?;
    let target = single_identifier(left)?;
    (target != "_").then_some((target, right))
}

fn go_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    const COMPOUND: &[&str] = &["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="];
    if !matches!(
        node.kind(),
        "short_var_declaration" | "assignment_statement"
    ) || COMPOUND
        .iter()
        .any(|operator| node.text().contains(operator))
    {
        return None;
    }
    Some((
        node.child_by_field_name("left")?,
        node.child_by_field_name("right")?,
    ))
}

fn python_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    let assignment = node
        .named_child(0)
        .filter(|child| node.kind() == "expression_statement" && child.kind() == "assignment")?;
    Some((
        assignment.child_by_field_name("left")?,
        assignment.child_by_field_name("right")?,
    ))
}

fn rust_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    // A `#[cfg(..)]` binding is one of several alternatives, not a store.
    if ast_scan::preceding_attributes(node).next().is_some() {
        return None;
    }
    match node.kind() {
        "let_declaration" => Some((
            node.child_by_field_name("pattern")?,
            node.child_by_field_name("value")?,
        )),
        "expression_statement" => {
            let assignment = node
                .named_child(0)
                .filter(|child| child.kind() == "assignment_expression")?;
            Some((
                assignment.child_by_field_name("left")?,
                assignment.child_by_field_name("right")?,
            ))
        }
        _ => None,
    }
}

/// A JavaScript, TypeScript, or Java declaration of exactly one variable
/// with an initializer.
fn declarator_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    if !matches!(
        node.kind(),
        "lexical_declaration" | "variable_declaration" | "local_variable_declaration"
    ) {
        return None;
    }
    let declarators = node
        .named_children()
        .filter(|child| child.kind() == "variable_declarator")
        .collect::<Vec<_>>();
    let [declarator] = declarators.as_slice() else {
        return None;
    };
    Some((
        declarator.child_by_field_name("name")?,
        declarator.child_by_field_name("value")?,
    ))
}

/// A plain `target = value;` expression statement in a C-family grammar.
fn expression_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    let assignment = node.named_child(0).filter(|child| {
        node.kind() == "expression_statement" && child.kind() == "assignment_expression"
    })?;
    // Java spells `+=` as an assignment expression too; it reads the target.
    let operator = assignment.children().find(|child| !child.is_named())?;
    (operator.text() == "=").then_some(())?;
    Some((
        assignment.child_by_field_name("left")?,
        assignment.child_by_field_name("right")?,
    ))
}

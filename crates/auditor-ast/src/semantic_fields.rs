use super::{NodeKind, SourceSpan, IDENTIFIER_KINDS};
use crate::{AstLang, AstNode};

pub(super) fn first_field(node: AstNode<'_>, fields: &[&str]) -> Option<SourceSpan> {
    fields
        .iter()
        .find_map(|field| node.child_by_field_name(field).map(SourceSpan::from_node))
}

pub(super) fn assignment_fields(node: AstNode<'_>) -> (Option<SourceSpan>, Option<SourceSpan>) {
    if node.language() == AstLang::Kotlin && node.kind() == "property_declaration" {
        let Some(offset) = node.text().find('=') else {
            return (None, None);
        };
        let operator = node.start_byte() + offset;
        return (
            first_identifier_before(node, operator),
            first_named_node_after(node, operator + 1),
        );
    }
    let target = first_field(
        node,
        &["left", "target", "pattern", "variable", "argument", "name"],
    );
    let value = first_field(node, &["right", "value", "initializer", "expression"]);
    if target.is_some() || value.is_some() || !node.text().contains('=') {
        return (target, value);
    }
    let count = node.named_child_count();
    if count < 2 {
        return (None, None);
    }
    let target = node.named_child(0).map(SourceSpan::from_node);
    (
        target,
        node.named_child(count - 1).map(SourceSpan::from_node),
    )
}

fn first_named_node_after(node: AstNode<'_>, byte: usize) -> Option<SourceSpan> {
    node.named_children().find_map(|child| {
        if child.start_byte() >= byte {
            Some(SourceSpan::from_node(child))
        } else if child.end_byte() > byte {
            first_named_node_after(child, byte)
        } else {
            None
        }
    })
}

fn first_identifier_before(node: AstNode<'_>, byte: usize) -> Option<SourceSpan> {
    node.named_children().find_map(|child| {
        if child.start_byte() >= byte {
            None
        } else if NodeKind::of(child).is(IDENTIFIER_KINDS) {
            Some(SourceSpan::from_node(child))
        } else {
            first_identifier_before(child, byte)
        }
    })
}

pub(super) fn loop_field(node: AstNode<'_>, fields: &[&str]) -> Option<SourceSpan> {
    first_field(node, fields)
        .or_else(|| {
            node.named_children()
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "range_clause" | "for_in_clause" | "foreach_clause"
                    )
                })
                .find_map(|clause| first_field(clause, fields))
        })
        .or_else(|| {
            if node.language() != AstLang::Kotlin || node.kind() != "for_statement" {
                return None;
            }
            let parts = node
                .named_children()
                .filter(|child| child.kind() != "control_structure_body")
                .collect::<Vec<_>>();
            if fields.contains(&"left") || fields.contains(&"pattern") {
                parts.first().copied().map(SourceSpan::from_node)
            } else {
                parts.last().copied().map(SourceSpan::from_node)
            }
        })
}

pub(super) fn symbol_name(node: AstNode<'_>) -> Option<SourceSpan> {
    if NodeKind::of(node).is(IDENTIFIER_KINDS) {
        return Some(SourceSpan::from_node(node));
    }
    if node.kind() == "function_definition" && matches!(node.language(), AstLang::C | AstLang::Cpp)
    {
        return c_function_name(node).map(SourceSpan::from_node);
    }
    // A Java declaration names its variable inside the declarator
    // (`List<String> result = new ArrayList<>()`), not across its span.
    if let Some(name) = node
        .child_by_field_name("declarator")
        .and_then(|declarator| declarator.child_by_field_name("name"))
    {
        return Some(SourceSpan::from_node(name));
    }
    first_field(node, &["name", "declarator", "pattern", "left"]).or_else(|| {
        node.named_children()
            .find(|child| NodeKind::of(*child).is(IDENTIFIER_KINDS))
            .map(SourceSpan::from_node)
    })
}

/// The name a C or C++ function definition declares, through the declarator
/// chain (`*name(..)`, `(name)(..)`, `Type::name(..)`), not its last
/// parameter.
fn c_function_name(node: AstNode<'_>) -> Option<AstNode<'_>> {
    let mut current = node.child_by_field_name("declarator")?;
    loop {
        current = match current.kind() {
            "identifier" | "field_identifier" | "operator_name" | "destructor_name" => {
                return Some(current)
            }
            "qualified_identifier" => current.child_by_field_name("name")?,
            "parenthesized_declarator" => current.named_child(0)?,
            _ => current.child_by_field_name("declarator")?,
        };
    }
}

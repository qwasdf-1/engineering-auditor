//! The string a declaration or assignment binds to a name, per grammar,
//! for the literal security rules: the credential-named target and its
//! literal value, and the declaration names that mark a platform's own
//! temp-directory abstraction.

use auditor_ast::AstNode;

use super::credential_literal;
use crate::ast_scan;
use crate::rust_api_identity::rust_string;

/// A Rust `let`, `const`, `static`, assignment, or struct field initializer
/// binding a credential-named target to a string literal.
pub(super) fn rust_assigned_string(node: AstNode<'_>) -> Option<(String, String)> {
    let (target, value) = match node.kind() {
        "let_declaration" => (
            node.child_by_field_name("pattern")?,
            node.child_by_field_name("value")?,
        ),
        "const_item" | "static_item" => (
            node.child_by_field_name("name")?,
            node.child_by_field_name("value")?,
        ),
        "assignment_expression" => {
            let left = node.child_by_field_name("left")?;
            (
                left.child_by_field_name("field").unwrap_or(left),
                node.child_by_field_name("right")?,
            )
        }
        "field_initializer" => (
            node.child_by_field_name("field")?,
            node.child_by_field_name("value")?,
        ),
        _ => return None,
    };
    if !matches!(target.kind(), "identifier" | "field_identifier") {
        return None;
    }
    credential_literal(target.text().to_string(), rust_string(value)?)
}

/// A binding of a credential-named target (a variable, field, object key,
/// or struct key) to a string literal in the brace languages and Kotlin.
pub(super) fn bound_string(node: AstNode<'_>) -> Option<(String, String)> {
    let field = |name: &str| node.child_by_field_name(name);
    let (target, value) = match node.kind() {
        // C# declarators carry the initializer as an unnamed field.
        "variable_declarator" if field("value").is_none() => {
            let name = field("name")?;
            let value = node
                .named_children()
                .filter(|child| child.start_byte() != name.start_byte())
                .last()?;
            (name, value)
        }
        "variable_declarator" | "public_field_definition" | "const_spec" | "var_spec" => {
            (field("name")?, field("value")?)
        }
        "init_declarator" => (field("declarator")?, field("value")?),
        "property_declaration" => crate::kotlin_api_identity::kotlin_property_store(node)?,
        "assignment_expression" | "assignment" => {
            // `x += ".."` reads `x`; only a plain store binds a literal.
            let operator = node.children().find(|child| !child.is_named())?;
            (operator.text() == "=").then_some(())?;
            match node.kind() {
                "assignment" => (node.named_child(0)?, node.named_child(1)?),
                _ => (field("left")?, field("right")?),
            }
        }
        "pair" | "keyed_element" => (field("key")?, field("value")?),
        "short_var_declaration" | "assignment_statement" => (field("left")?, field("right")?),
        _ => return None,
    };
    credential_literal(target_name(target)?, string_value(value)?)
}

/// The name a target expression stores to: an identifier, a member or field
/// access's last name, an object key, or a single-element Go list.
fn target_name(target: AstNode<'_>) -> Option<String> {
    match target.kind() {
        "identifier" | "property_identifier" | "field_identifier" | "simple_identifier" => {
            Some(target.text().to_string())
        }
        "member_expression" => Some(target.child_by_field_name("property")?.text().to_string()),
        "field_access" | "field_expression" => {
            Some(target.child_by_field_name("field")?.text().to_string())
        }
        "member_access_expression" => Some(target.child_by_field_name("name")?.text().to_string()),
        // C declarators (`*password`, `key[]`) and Kotlin assignment targets.
        "pointer_declarator" | "array_declarator" => {
            target_name(target.child_by_field_name("declarator")?)
        }
        "directly_assignable_expression" | "navigation_expression" => {
            target_name(ast_scan::nodes_of_kind(target, &["simple_identifier"]).pop()?)
        }
        "string" => string_value(target),
        "expression_list" | "literal_element" => target_name(only_child(target)?),
        _ => None,
    }
}

fn only_child(node: AstNode<'_>) -> Option<AstNode<'_>> {
    let mut children = node.named_children();
    let only = children.next()?;
    children.next().is_none().then_some(only)
}

/// The value of a string literal without interpolation, in any language.
pub(super) fn string_value(node: AstNode<'_>) -> Option<String> {
    match node.kind() {
        "expression_list" | "literal_element" => string_value(only_child(node)?),
        // A C# verbatim string has no content children.
        "verbatim_string_literal" => {
            let text = node.text().strip_prefix('@')?;
            Some(
                text.get(1..text.len().checked_sub(1)?)?
                    .replace("\"\"", "\""),
            )
        }
        "string"
        | "template_string"
        | "string_literal"
        | "raw_string_literal"
        | "interpreted_string_literal" => {
            let mut value = String::new();
            for part in node.named_children() {
                let kind = part.kind();
                if matches!(
                    kind,
                    "template_substitution"
                        | "interpolation"
                        | "interpolated_identifier"
                        | "interpolated_expression"
                ) {
                    return None;
                }
                if kind.ends_with("content")
                    || kind.ends_with("fragment")
                    || kind == "escape_sequence"
                {
                    value.push_str(part.text());
                }
            }
            Some(value)
        }
        _ => None,
    }
}

/// Whether a declaration enclosing `node` (a function, property, or
/// variable) is named for the temp directory (`tmpdir`,
/// `SYSTEM_TEMPORARY_DIRECTORY`, `temporaryDirectory()`): a platform
/// abstraction that resolves it.
pub(super) fn defines_temp_directory(node: AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .filter_map(declaration_name)
        .any(|name| {
            let lower = name.to_ascii_lowercase();
            lower.contains("tmp") || lower.contains("temp")
        })
}

/// The name a function, property, or variable declaration binds.
fn declaration_name(node: AstNode<'_>) -> Option<&str> {
    fn named(node: AstNode<'_>) -> Option<&str> {
        node.named_children()
            .find(|child| matches!(child.kind(), "simple_identifier" | "identifier"))
            .map(|name| name.text())
    }
    match node.kind() {
        "property_declaration" => node
            .named_children()
            .find(|child| child.kind() == "variable_declaration")
            .and_then(named),
        // Kotlin parses a top-level accessor as the property's next sibling.
        "getter" | "setter" => node
            .parent()?
            .named_children()
            .filter(|sibling| {
                sibling.kind() == "property_declaration" && sibling.end_byte() <= node.start_byte()
            })
            .last()
            .and_then(declaration_name),
        "function_declaration"
        | "function_definition"
        | "function_item"
        | "method_declaration"
        | "variable_declarator"
        | "const_item"
        | "static_item"
        | "const_spec"
        | "var_spec" => node
            .child_by_field_name("name")
            .map(|name| name.text())
            .or_else(|| named(node)),
        _ => None,
    }
}

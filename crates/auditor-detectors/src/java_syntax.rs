//! Java syntax facts shared by the API resolver and the rule tables: the
//! declaration a variable name refers to, literal and argument values, and
//! the enclosing method of a node.

use auditor_ast::AstNode;

use crate::ast_scan;

/// Kinds that declare a named variable, with the field holding its type.
const DECLARATIONS: &[&str] = &[
    "local_variable_declaration",
    "field_declaration",
    "constant_declaration",
];

/// A declared variable's type node and initializer.
pub(crate) type Declaration<'a> = (AstNode<'a>, Option<AstNode<'a>>);

/// The type node and initializer of the declaration `identifier` refers to:
/// the nearest enclosing local, parameter, loop variable, resource, catch
/// parameter, or field of that name declared before the use.
pub(crate) fn variable_declaration(identifier: AstNode<'_>) -> Option<Declaration<'_>> {
    let mut node = identifier;
    while let Some(scope) = node.parent() {
        if let Some(found) = declared_in(scope, identifier) {
            return found;
        }
        node = scope;
    }
    None
}

/// What `scope` declares under `identifier`'s name: `Some(declaration)` ends
/// the search (an implicitly typed lambda parameter shadows outer names with
/// no declared type), `None` continues outward.
fn declared_in<'a>(scope: AstNode<'a>, identifier: AstNode<'a>) -> Option<Option<Declaration<'a>>> {
    let name = identifier.text();
    let found = match scope.kind() {
        "block" | "switch_block_statement_group" | "constructor_body" => scope
            .named_children()
            .filter(|child| child.end_byte() <= identifier.start_byte())
            .filter(|child| child.kind() == "local_variable_declaration")
            .find_map(|declaration| declarator_named(declaration, name)),
        "method_declaration" | "constructor_declaration" => scope
            .child_by_field_name("parameters")
            .and_then(|parameters| parameter_named(parameters, name)),
        "enhanced_for_statement" => scope
            .child_by_field_name("name")
            .filter(|declared| declared.text() == name)
            .and_then(|_| Some((scope.child_by_field_name("type")?, None))),
        "for_statement" => scope
            .child_by_field_name("init")
            .filter(|init| init.kind() == "local_variable_declaration")
            .and_then(|init| declarator_named(init, name)),
        "try_with_resources_statement" => resource_named(scope, name),
        "catch_clause" => catch_parameter_named(scope, name),
        "class_body" | "enum_body_declarations" | "interface_body" => scope
            .named_children()
            .filter(|child| DECLARATIONS.contains(&child.kind()))
            .find_map(|declaration| declarator_named(declaration, name)),
        "lambda_expression" => return lambda_parameter_named(scope, name),
        _ => None,
    };
    found.map(Some)
}

fn resource_named<'a>(statement: AstNode<'a>, name: &str) -> Option<Declaration<'a>> {
    let resource = statement
        .child_by_field_name("resources")?
        .named_children()
        .find(|resource| {
            resource
                .child_by_field_name("name")
                .is_some_and(|declared| declared.text() == name)
        })?;
    Some((
        resource.child_by_field_name("type")?,
        resource.child_by_field_name("value"),
    ))
}

fn catch_parameter_named<'a>(clause: AstNode<'a>, name: &str) -> Option<Declaration<'a>> {
    let parameter = clause
        .named_children()
        .find(|child| child.kind() == "catch_formal_parameter")?;
    parameter
        .child_by_field_name("name")
        .filter(|declared| declared.text() == name)?;
    let catch_type = parameter
        .named_children()
        .find(|child| child.kind() == "catch_type")?;
    Some((catch_type, None))
}

/// `Some` when the lambda declares `name`: its declared type, if any.
fn lambda_parameter_named<'a>(lambda: AstNode<'a>, name: &str) -> Option<Option<Declaration<'a>>> {
    let parameters = lambda.child_by_field_name("parameters")?;
    let declares = parameters.text() == name
        || ast_scan::nodes_of_kind(parameters, &["identifier"])
            .iter()
            .any(|parameter| parameter.text() == name);
    declares.then(|| parameter_named(parameters, name))
}

/// The field `name` of the class enclosing `node`.
pub(crate) fn field_declaration<'a>(mut node: AstNode<'a>, name: &str) -> Option<Declaration<'a>> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "class_body" {
            return parent
                .named_children()
                .filter(|child| child.kind() == "field_declaration")
                .find_map(|declaration| declarator_named(declaration, name));
        }
        node = parent;
    }
    None
}

fn declarator_named<'a>(declaration: AstNode<'a>, name: &str) -> Option<Declaration<'a>> {
    let type_node = declaration.child_by_field_name("type")?;
    declaration
        .named_children()
        .filter(|child| child.kind() == "variable_declarator")
        .find(|declarator| {
            declarator
                .child_by_field_name("name")
                .is_some_and(|declared| declared.text() == name)
        })
        .map(|declarator| (type_node, declarator.child_by_field_name("value")))
}

fn parameter_named<'a>(parameters: AstNode<'a>, name: &str) -> Option<Declaration<'a>> {
    parameters
        .named_children()
        .filter(|child| matches!(child.kind(), "formal_parameter" | "spread_parameter"))
        .find(|parameter| {
            parameter
                .child_by_field_name("name")
                .or_else(|| {
                    parameter
                        .named_children()
                        .find(|child| child.kind() == "variable_declarator")
                        .and_then(|declarator| declarator.child_by_field_name("name"))
                })
                .is_some_and(|declared| declared.text() == name)
        })
        .and_then(|parameter| {
            parameter
                .child_by_field_name("type")
                .or_else(|| {
                    parameter
                        .named_children()
                        .find(|child| child.kind().ends_with("type"))
                })
                .map(|type_node| (type_node, None))
        })
}

pub(crate) fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The value of a Java string literal without escapes or text blocks.
pub(crate) fn java_string(node: AstNode<'_>) -> Option<String> {
    if node.kind() != "string_literal" {
        return None;
    }
    let text = node.text();
    if text.starts_with("\"\"\"") || text.contains('\\') {
        return None;
    }
    Some(text.get(1..text.len().checked_sub(1)?)?.to_string())
}

/// The argument expressions of a Java call.
pub(crate) fn java_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| list.named_children().collect())
        .unwrap_or_default()
}

/// String values of a call's arguments, flattening one array initializer
/// (`exec(new String[] {"sh", "-c", line})`); `None` for a non-literal.
pub(crate) fn java_string_arguments(call: AstNode<'_>) -> Vec<Option<String>> {
    let arguments = java_arguments(call);
    let elements = match arguments.as_slice() {
        [array] if array.kind() == "array_creation_expression" => array
            .named_children()
            .find(|child| child.kind() == "array_initializer")
            .map(|initializer| initializer.named_children().collect())
            .unwrap_or_default(),
        [array] if array.kind() == "array_initializer" => array.named_children().collect(),
        _ => arguments,
    };
    elements.into_iter().map(java_string).collect()
}

/// The method or constructor declaration enclosing `node`, stopping at a
/// lambda or class boundary.
pub(crate) fn enclosing_java_method(mut node: AstNode<'_>) -> Option<AstNode<'_>> {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "method_declaration" | "constructor_declaration" | "static_initializer" => {
                return Some(parent)
            }
            "class_body" => return None,
            _ => node = parent,
        }
    }
    None
}

/// `public static void main(String[] args)`: a program entry point.
pub(crate) fn is_java_main_method(method: AstNode<'_>) -> bool {
    method.kind() == "method_declaration"
        && method
            .child_by_field_name("name")
            .is_some_and(|name| name.text() == "main")
        && has_java_modifier(method, "static")
}

/// Whether a Java declaration carries `modifier` (`public`, `static`).
pub(crate) fn has_java_modifier(declaration: AstNode<'_>, modifier: &str) -> bool {
    declaration
        .named_children()
        .find(|child| child.kind() == "modifiers")
        .is_some_and(|modifiers| {
            modifiers
                .text()
                .split_whitespace()
                .any(|word| word == modifier)
        })
}

/// Whether the file declares a `main` entry point anywhere.
pub(crate) fn declares_java_main(root: AstNode<'_>) -> bool {
    ast_scan::nodes_of_kind(root, &["method_declaration"])
        .into_iter()
        .any(is_java_main_method)
}

//! Where each grammar puts the parts of a binding: a function's name and
//! parameter list, a loop's variable and iterable, and a declaration's or
//! assignment's target and value. Findings-free.

use crate::AstNode;

/// Parameter lists, by kind, for functions without a `parameters` field.
pub(super) const PARAMETER_LIST_KINDS: &[&str] = &[
    "formal_parameters",
    "parameters",
    "parameter_list",
    "lambda_parameters",
    "function_value_parameters",
    "closure_parameters",
    "bracketed_parameter_list",
];

/// A function's name: its `name` field, a C declarator's identifier, a
/// Kotlin function's first name, or the callee of a Julia signature.
pub(super) fn function_name(function: AstNode<'_>) -> Option<&str> {
    if let Some(name) = function.child_by_field_name("name") {
        return Some(name.text());
    }
    if let Some(declarator) = function_declarator(function) {
        return declarator
            .child_by_field_name("declarator")
            .map(|name| name.text());
    }
    if let Some(call) = julia_signature(function) {
        return call.named_child(0).map(|name| name.text());
    }
    function
        .named_children()
        .find(|child| child.kind() == "simple_identifier")
        .map(|name| name.text())
}

/// A C or C++ function's `function_declarator`, under any pointer stars,
/// or a C++ lambda's `abstract_function_declarator`.
fn function_declarator(function: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(function.child_by_field_name("declarator"), |declarator| {
        declarator.child_by_field_name("declarator")
    })
    .find(|declarator| {
        matches!(
            declarator.kind(),
            "function_declarator" | "abstract_function_declarator"
        )
    })
}

/// The call a Julia `function f(a, b)` signature is written as.
fn julia_signature(function: AstNode<'_>) -> Option<AstNode<'_>> {
    function
        .named_children()
        .find(|child| child.kind() == "signature")?
        .named_children()
        .find(|child| child.kind() == "call_expression")
}

/// A function's parameter list, or its one parameter.
pub(super) fn parameter_list(function: AstNode<'_>) -> Option<AstNode<'_>> {
    function
        .child_by_field_name("parameters")
        .or_else(|| function.child_by_field_name("parameter"))
        .or_else(|| {
            function
                .named_children()
                .find(|child| PARAMETER_LIST_KINDS.contains(&child.kind()))
        })
        .or_else(|| function_declarator(function)?.child_by_field_name("parameters"))
        .or_else(|| {
            julia_signature(function)?
                .named_children()
                .find(|child| child.kind() == "argument_list")
        })
        .or_else(|| {
            // Julia's `x -> ..` and `(x, y) -> ..`.
            (function.kind() == "arrow_function_expression")
                .then(|| function.named_child(0))
                .flatten()
        })
}

/// A loop's variable and what it iterates: by field (`for x in xs`, C++
/// `for (T x : xs)`), in a Go, Python, or Julia clause, or by position for
/// Kotlin (`for (x in xs)`) and Julia (`for x in xs`).
pub(super) fn loop_target_and_iterable(
    node: AstNode<'_>,
) -> (Option<AstNode<'_>>, Option<AstNode<'_>>) {
    let clause = node
        .named_children()
        .find(|child| {
            matches!(
                child.kind(),
                "range_clause" | "for_in_clause" | "for_binding"
            )
        })
        .unwrap_or(node);
    let target = ["left", "name", "pattern", "variable", "declarator"]
        .iter()
        .find_map(|field| clause.child_by_field_name(field));
    let iterable = ["right", "value", "iterable"]
        .iter()
        .find_map(|field| clause.child_by_field_name(field))
        // Ruby's `for x in xs` holds `in xs`.
        .map(|value| match value.kind() {
            "in" => value.named_child(0).unwrap_or(value),
            _ => value,
        });
    if target.is_some() || iterable.is_some() {
        return (target, iterable);
    }
    let mut parts = clause.named_children();
    match clause.kind() {
        "for_binding" => (parts.next(), clause.named_children().last()),
        "for_statement" => {
            let target = parts.find(|part| {
                matches!(
                    part.kind(),
                    "variable_declaration" | "multi_variable_declaration"
                )
            });
            (target, target.and_then(|_| parts.next()))
        }
        _ => (None, None),
    }
}

/// A declaration's target and value. `with open(p) as f` binds `f` to the
/// expression before `as`; `x++` has a target only.
pub(super) fn target_and_value(node: AstNode<'_>) -> (Option<AstNode<'_>>, Option<AstNode<'_>>) {
    if node.kind() == "as_pattern" {
        return (node.child_by_field_name("alias"), node.named_child(0));
    }
    if matches!(node.kind(), "var_spec" | "const_spec") {
        return (Some(node), node.child_by_field_name("value"));
    }
    if node.kind() == "property_declaration" {
        let target = node.named_children().find(|child| {
            matches!(
                child.kind(),
                "variable_declaration" | "multi_variable_declaration"
            )
        });
        let value = node.text().find('=').and_then(|offset| {
            node.named_children()
                .find(|child| child.start_byte() > node.start_byte() + offset)
        });
        return (target, value);
    }
    let target = ["left", "name", "pattern", "declarator", "argument"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| node.named_child(0));
    let value = ["right", "value", "initializer"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| {
            let last = node.named_children().last()?;
            (node.named_child_count() > 1 && node.text().contains('=') && Some(last) != target)
                .then_some(last)
        });
    (target, value)
}

pub(super) fn operator(node: AstNode<'_>) -> Option<&str> {
    node.child_by_field_name("operator")
        .map(|operator| operator.text())
        .or_else(|| {
            node.children()
                .find(|child| !child.is_named() && child.text().ends_with('='))
                .map(|operator| operator.text())
        })
}

/// The items of a list on either side of a multiple assignment.
pub(super) fn list_items(node: AstNode<'_>) -> Vec<AstNode<'_>> {
    if matches!(
        node.kind(),
        "expression_list" | "pattern_list" | "tuple" | "tuple_expression"
    ) {
        node.named_children().collect()
    } else {
        vec![node]
    }
}

//! Where each grammar puts the parts of a binding: a function's name and
//! parameter list, a loop's variable and iterable, and a declaration's or
//! assignment's target and value. Findings-free.

use super::BindingKind;
use crate::{AstLang, AstNode};

/// Declarations and assignments: (kind, binding kind).
pub(super) const BINDING_KINDS: &[(&str, BindingKind)] = &[
    ("variable_declarator", BindingKind::Declaration),
    ("init_declarator", BindingKind::Declaration),
    ("let_declaration", BindingKind::Declaration),
    ("short_var_declaration", BindingKind::Declaration),
    ("var_spec", BindingKind::Declaration),
    ("const_spec", BindingKind::Declaration),
    ("property_declaration", BindingKind::Declaration),
    ("named_expression", BindingKind::Declaration),
    ("resource", BindingKind::Declaration),
    ("as_pattern", BindingKind::Declaration),
    ("assignment", BindingKind::Assignment),
    ("assignment_expression", BindingKind::Assignment),
    ("assignment_statement", BindingKind::Assignment),
    ("variable_assignment", BindingKind::Assignment),
    ("augmented_assignment", BindingKind::Augmented),
    ("augmented_assignment_expression", BindingKind::Augmented),
    ("compound_assignment_expr", BindingKind::Augmented),
    ("update_expression", BindingKind::Augmented),
    ("inc_statement", BindingKind::Augmented),
    ("dec_statement", BindingKind::Augmented),
];

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
pub(crate) fn loop_target_and_iterable(
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
        // Kotlin's `for (x in xs)` has no fields; a JavaScript
        // `for (var i = 0; ..)` initializer is not a loop variable.
        "for_statement" if clause.language() == AstLang::Kotlin => {
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
        // `except E as e` binds the exception raised, not the types listed.
        let value = node.named_child(0).filter(|_| {
            node.parent()
                .is_none_or(|parent| parent.kind() != "except_clause")
        });
        return (node.child_by_field_name("alias"), value);
    }
    if matches!(node.kind(), "var_spec" | "const_spec") {
        return (Some(node), node.child_by_field_name("value"));
    }
    if node.kind() == "property_declaration" && node.language() == AstLang::Kotlin {
        let target = node.named_children().find(|child| {
            matches!(
                child.kind(),
                "variable_declaration" | "multi_variable_declaration"
            )
        });
        return (target, after_assignment_operator(node));
    }
    let target = ["left", "name", "pattern", "declarator", "argument"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| node.named_child(0));
    let value = ["right", "value", "initializer"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| after_assignment_operator(node).filter(|value| Some(*value) != target));
    (target, value)
}

/// The expression after a node's `=` (or `+=`, `:=`) token: a C#
/// declarator's or Julia assignment's value, which no field holds. A `=`
/// inside a type (`(v: T) => void`) is not an operator token.
fn after_assignment_operator(node: AstNode<'_>) -> Option<AstNode<'_>> {
    let mut assigned = false;
    for child in node.children() {
        let token = !child.is_named() || child.kind() == "operator";
        if assigned && !token && !child.kind().contains("comment") {
            return Some(child);
        }
        let text = child.text();
        assigned |= token
            && text.ends_with('=')
            && !matches!(text, "==" | "!=" | "<=" | ">=" | "===" | "!==");
    }
    None
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

/// The target and value of a store, for the semantic facts: `None` when
/// `node` is not a declaration or assignment. The target is the name
/// declared (`x` of Kotlin's `val x: Int = ..`, of Go's `var x int = ..`,
/// and of C's `int *x = ..`), or the expression written (`a.b`, `m[k]`).
pub(crate) fn assignment_parts(
    node: AstNode<'_>,
) -> Option<(Option<AstNode<'_>>, Option<AstNode<'_>>)> {
    BINDING_KINDS
        .iter()
        .any(|(kind, _)| *kind == node.kind())
        .then_some(())?;
    let (target, value) = target_and_value(node);
    Some((target.map(declared_name), value))
}

/// The name a declaration target declares, when it declares one name.
fn declared_name(target: AstNode<'_>) -> AstNode<'_> {
    match target.kind() {
        "variable_declaration" => target
            .named_children()
            .find(|child| child.kind() == "simple_identifier")
            .unwrap_or(target),
        "var_spec" | "const_spec" => target.child_by_field_name("name").unwrap_or(target),
        "pointer_declarator" | "reference_declarator" | "array_declarator" => {
            std::iter::successors(Some(target), |declarator| {
                declarator.child_by_field_name("declarator").or_else(|| {
                    declarator
                        .named_children()
                        .find(|child| child.kind() == "identifier")
                })
            })
            .find(|declarator| declarator.kind() == "identifier")
            .unwrap_or(target)
        }
        _ => target,
    }
}

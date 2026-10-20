//! The bindings and reads of a flow scope, read from the node kinds and
//! fields the grammars share.

use super::{Binding, BindingKind, ANONYMOUS_FUNCTION_KINDS, LOOP_KINDS, NAMED_FUNCTION_KINDS};
use crate::AstNode;

/// Identifiers that name a local value.
const NAME_KINDS: &[&str] = &[
    "identifier",
    "simple_identifier",
    "shorthand_property_identifier_pattern",
    "shorthand_property_identifier",
];

/// Parameter lists, by kind, for functions without a `parameters` field.
const PARAMETER_LIST_KINDS: &[&str] = &[
    "formal_parameters",
    "parameters",
    "parameter_list",
    "lambda_parameters",
    "function_value_parameters",
    "closure_parameters",
    "bracketed_parameter_list",
];

/// Declarations and assignments: (kind, binding kind).
const BINDING_KINDS: &[(&str, BindingKind)] = &[
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
    ("augmented_assignment", BindingKind::Augmented),
    ("augmented_assignment_expression", BindingKind::Augmented),
    ("compound_assignment_expr", BindingKind::Augmented),
    ("update_expression", BindingKind::Augmented),
    ("inc_statement", BindingKind::Augmented),
    ("dec_statement", BindingKind::Augmented),
];

/// Every binding in `scope`, in source order.
pub(super) fn collect(scope: AstNode<'_>) -> Vec<Binding<'_>> {
    let mut bindings = Vec::new();
    if NAMED_FUNCTION_KINDS.contains(&scope.kind()) {
        parameters(scope, scope, &mut bindings);
    }
    visit(scope, &mut |node| node_bindings(node, scope, &mut bindings));
    bindings.sort_by_key(|binding| binding.target.start_byte());
    bindings
}

/// The reads of `name` in `scope`: identifiers that are neither binding
/// targets nor member, key, or label names.
pub(super) fn reads<'a>(scope: AstNode<'a>, name: &str) -> Vec<AstNode<'a>> {
    let mut reads = Vec::new();
    visit(scope, &mut |node| {
        if NAME_KINDS.contains(&node.kind()) && node.text() == name && is_read(node) {
            reads.push(node);
        }
    });
    reads
}

/// A function's name: its `name` field, or a C declarator's identifier.
pub(super) fn function_name(function: AstNode<'_>) -> Option<&str> {
    if let Some(name) = function.child_by_field_name("name") {
        return Some(name.text());
    }
    let mut declarator = function.child_by_field_name("declarator")?;
    while let Some(inner) = declarator.child_by_field_name("declarator") {
        declarator = inner;
    }
    Some(declarator.text())
}

/// Calls `f` on every node of `scope`, not entering nested named functions.
fn visit<'a>(scope: AstNode<'a>, f: &mut impl FnMut(AstNode<'a>)) {
    for child in scope.named_children() {
        if NAMED_FUNCTION_KINDS.contains(&child.kind()) {
            continue;
        }
        f(child);
        visit(child, f);
    }
}

fn node_bindings<'a>(node: AstNode<'a>, scope: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    if ANONYMOUS_FUNCTION_KINDS.contains(&node.kind()) {
        parameters(node, node, out);
    } else if LOOP_KINDS.contains(&node.kind()) {
        loop_variable(node, out);
    } else if matches!(
        node.kind(),
        "catch_clause" | "except_clause" | "catch_declaration"
    ) {
        catch_parameter(node, out);
    } else if let Some((_, kind)) = BINDING_KINDS.iter().find(|(name, _)| *name == node.kind()) {
        declaration(node, *kind, scope, out);
    } else if node.kind() == "declaration" {
        uninitialized(node, scope, out);
    }
}

/// The variables a C or C++ declaration declares without a value
/// (`size_t len;`, `char *p, buf[8];`); `init_declarator`s bind their own,
/// and a function declarator declares no variable.
fn uninitialized<'a>(declaration: AstNode<'a>, scope: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    const DECLARATORS: &[&str] = &[
        "identifier",
        "pointer_declarator",
        "array_declarator",
        "reference_declarator",
        "parenthesized_declarator",
    ];
    for declarator in declaration
        .named_children()
        .filter(|child| DECLARATORS.contains(&child.kind()))
    {
        let mut name = declarator;
        while name.kind() != "identifier" {
            let Some(inner) = name
                .child_by_field_name("declarator")
                .or_else(|| {
                    name.named_children()
                        .find(|child| DECLARATORS.contains(&child.kind()))
                })
                .filter(|inner| DECLARATORS.contains(&inner.kind()))
            else {
                break;
            };
            name = inner;
        }
        if name.kind() != "identifier" {
            continue;
        }
        out.push(Binding {
            name: name.text(),
            target: name,
            value: None,
            node: declarator,
            kind: BindingKind::Declaration,
            region: scope,
            point: declarator.end_byte(),
        });
    }
}

/// The parameters of `function`, visible in `region`.
fn parameters<'a>(function: AstNode<'a>, region: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    let list = function
        .child_by_field_name("parameters")
        .or_else(|| function.child_by_field_name("parameter"))
        .or_else(|| {
            function
                .named_children()
                .find(|child| PARAMETER_LIST_KINDS.contains(&child.kind()))
        });
    let Some(list) = list else {
        return;
    };
    let parameters = if NAME_KINDS.contains(&list.kind()) {
        vec![list]
    } else {
        list.named_children().collect()
    };
    for parameter in parameters {
        let default = ["value", "default_value", "right"]
            .iter()
            .find_map(|field| parameter.child_by_field_name(field));
        for target in declared_names(parameter) {
            out.push(Binding {
                name: target.text(),
                target,
                value: default,
                node: parameter,
                kind: BindingKind::Parameter,
                region,
                point: region.start_byte(),
            });
        }
    }
}

fn loop_variable<'a>(node: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    let clause = node
        .named_children()
        .find(|child| matches!(child.kind(), "range_clause" | "for_in_clause"))
        .unwrap_or(node);
    let target = ["left", "name", "pattern", "variable"]
        .iter()
        .find_map(|field| clause.child_by_field_name(field));
    let iterable = ["right", "value", "iterable"]
        .iter()
        .find_map(|field| clause.child_by_field_name(field));
    let (Some(target), Some(iterable)) = (target, iterable) else {
        return;
    };
    for name in declared_names(target) {
        out.push(Binding {
            name: name.text(),
            target: name,
            value: Some(iterable),
            node,
            kind: BindingKind::LoopVariable,
            region: node,
            point: iterable.end_byte(),
        });
    }
}

fn catch_parameter<'a>(node: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    let parameter = ["parameter", "name", "alias"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| {
            node.named_children()
                .find(|child| child.kind().contains("parameter"))
        });
    let Some(parameter) = parameter else {
        return;
    };
    for target in declared_names(parameter) {
        out.push(Binding {
            name: target.text(),
            target,
            value: None,
            node: parameter,
            kind: BindingKind::Parameter,
            region: node,
            point: parameter.end_byte(),
        });
    }
}

fn declaration<'a>(
    node: AstNode<'a>,
    kind: BindingKind,
    scope: AstNode<'a>,
    out: &mut Vec<Binding<'a>>,
) {
    let (Some(target), value) = target_and_value(node) else {
        return;
    };
    let kind = match (kind, operator(node)) {
        (BindingKind::Assignment, Some(operator)) if operator != "=" && operator != ":=" => {
            BindingKind::Augmented
        }
        _ => kind,
    };
    let targets = declared_names(target);
    let values = value.map(|value| list_items(value)).unwrap_or_default();
    // `a, b := x, y` binds by position; a one-item list is its item.
    let positional = values.len() == targets.len();
    for (index, name) in targets.iter().enumerate() {
        out.push(Binding {
            name: name.text(),
            target: *name,
            value: if positional {
                Some(values[index])
            } else {
                value
            },
            node,
            kind,
            region: scope,
            point: node.end_byte(),
        });
    }
}

/// A declaration's target and value. `with open(p) as f` binds `f` to the
/// expression before `as`; `x++` has a target only.
fn target_and_value(node: AstNode<'_>) -> (Option<AstNode<'_>>, Option<AstNode<'_>>) {
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

fn operator(node: AstNode<'_>) -> Option<&str> {
    node.child_by_field_name("operator")
        .map(|operator| operator.text())
        .or_else(|| {
            node.children()
                .find(|child| !child.is_named() && child.text().ends_with('='))
                .map(|operator| operator.text())
        })
}

/// The names a target or parameter declares: the identifiers in a pattern,
/// without property keys, type annotations, or default values.
pub(super) fn declared_names(node: AstNode<'_>) -> Vec<AstNode<'_>> {
    if NAME_KINDS.contains(&node.kind()) {
        return vec![node];
    }
    if is_member(node) {
        return Vec::new();
    }
    // Go names several values in one declaration (`a, b int`, `var a, b = ..`).
    if matches!(
        node.kind(),
        "parameter_declaration" | "var_spec" | "const_spec"
    ) && node.language() == crate::AstLang::Go
    {
        return node
            .named_children()
            .filter(|child| child.kind() == "identifier")
            .collect();
    }
    let mut names = Vec::new();
    let named = ["name", "pattern", "left", "declarator"]
        .iter()
        .find_map(|field| node.child_by_field_name(field));
    if let Some(named) = named {
        return declared_names(named);
    }
    for child in node.named_children() {
        if child.kind().contains("type") || child.kind().contains("annotation") {
            continue;
        }
        if node.kind() == "pair_pattern" && node.child_by_field_name("key") == Some(child) {
            continue;
        }
        names.extend(declared_names(child));
    }
    names
}

/// The items of a list on either side of a multiple assignment.
fn list_items(node: AstNode<'_>) -> Vec<AstNode<'_>> {
    if matches!(
        node.kind(),
        "expression_list" | "pattern_list" | "tuple" | "tuple_expression"
    ) {
        node.named_children().collect()
    } else {
        vec![node]
    }
}

/// A member or element access: not a local name.
pub(super) fn is_member(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "member_expression"
            | "attribute"
            | "field_access"
            | "field_expression"
            | "selector_expression"
            | "subscript_expression"
            | "subscript"
            | "index_expression"
            | "element_access_expression"
            | "member_access_expression"
            | "navigation_expression"
            | "array_access"
    )
}

/// Whether `identifier` reads a value: not the name a binding declares, a
/// member or method name, a key, a label, or a named argument's name.
fn is_read(identifier: AstNode<'_>) -> bool {
    let Some(parent) = identifier.parent() else {
        return true;
    };
    let declared_in = ["property", "field", "attribute", "key", "label", "name"]
        .iter()
        .any(|field| parent.child_by_field_name(field) == Some(identifier));
    !declared_in && !is_binding_target(identifier)
}

/// Whether `identifier` is a declaration's or assignment's target, which
/// writes the name rather than reading it (`x++` and `x += 1` do both, and
/// count as reads).
fn is_binding_target(identifier: AstNode<'_>) -> bool {
    std::iter::successors(identifier.parent(), AstNode::parent)
        .take_while(|ancestor| !NAMED_FUNCTION_KINDS.contains(&ancestor.kind()))
        .find_map(|ancestor| {
            let declares = BINDING_KINDS.iter().any(|(kind, binding)| {
                *kind == ancestor.kind() && *binding != BindingKind::Augmented
            }) || LOOP_KINDS.contains(&ancestor.kind())
                || PARAMETER_LIST_KINDS.contains(&ancestor.kind());
            if !declares {
                return None;
            }
            let target = if LOOP_KINDS.contains(&ancestor.kind()) {
                ["left", "name", "pattern", "variable"]
                    .iter()
                    .find_map(|field| ancestor.child_by_field_name(field))
            } else if PARAMETER_LIST_KINDS.contains(&ancestor.kind()) {
                Some(ancestor)
            } else {
                target_and_value(ancestor).0
            };
            Some(target.is_some_and(|target| declared_names(target).contains(&identifier)))
        })
        .unwrap_or(false)
}

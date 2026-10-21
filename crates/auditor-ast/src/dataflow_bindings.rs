//! The bindings and reads of a flow scope, read from the node kinds and
//! fields the grammars share.

use super::shapes::{
    list_items, loop_target_and_iterable, operator, parameter_list, target_and_value,
    BINDING_KINDS, PARAMETER_LIST_KINDS,
};
use super::{is_anonymous_function, is_named_function, Binding, BindingKind, LOOP_KINDS};
use crate::AstNode;

/// Identifiers that name a local value.
const NAME_KINDS: &[&str] = &[
    "identifier",
    "simple_identifier",
    "implicit_parameter",
    "variable_name",
    "shorthand_property_identifier_pattern",
    "shorthand_property_identifier",
];

/// Every binding in `scope`, in source order.
pub(super) fn collect(scope: AstNode<'_>) -> Vec<Binding<'_>> {
    let mut bindings = Vec::new();
    if is_named_function(scope) {
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

/// Calls `f` on every node of `scope`, not entering nested named functions.
fn visit<'a>(scope: AstNode<'a>, f: &mut impl FnMut(AstNode<'a>)) {
    for child in scope.named_children() {
        if is_named_function(child) {
            continue;
        }
        f(child);
        visit(child, f);
    }
}

fn node_bindings<'a>(node: AstNode<'a>, scope: AstNode<'a>, out: &mut Vec<Binding<'a>>) {
    if is_anonymous_function(node) {
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
    let Some(list) = parameter_list(function) else {
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
    let (Some(target), Some(iterable)) = loop_target_and_iterable(node) else {
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
    for ancestor in std::iter::successors(identifier.parent(), AstNode::parent) {
        if let Some(declared) = declares(ancestor, identifier) {
            return declared;
        }
        if is_named_function(ancestor) {
            return false;
        }
    }
    false
}

/// Whether `identifier` is a name `ancestor` declares, when `ancestor`
/// declares names (a binding, a loop, a parameter list, a function's
/// parameters, or a Julia signature); `None` when it declares none around
/// `identifier`.
fn declares(ancestor: AstNode<'_>, identifier: AstNode<'_>) -> Option<bool> {
    let kind = ancestor.kind();
    let target = if kind == "signature" {
        return Some(true);
    } else if is_named_function(ancestor) || is_anonymous_function(ancestor) {
        Some(parameter_list(ancestor).filter(|list| super::contains(*list, identifier))?)
    } else if LOOP_KINDS.contains(&kind) {
        loop_target_and_iterable(ancestor).0
    } else if PARAMETER_LIST_KINDS.contains(&kind) {
        Some(ancestor)
    } else if BINDING_KINDS
        .iter()
        .any(|(binding, binding_kind)| *binding == kind && *binding_kind != BindingKind::Augmented)
    {
        target_and_value(ancestor).0
    } else {
        return None;
    };
    Some(target.is_some_and(|target| declared_names(target).contains(&identifier)))
}

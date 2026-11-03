//! Backlog bounds the code enforces itself, for `unbounded_channel_or_queue`
//! in Python and Java:
//! - A queue whose current size is compared against a limit before adding
//!   (`q.qsize() >= MAX`, `len(q) > n`, `q.size() >= max`) is capped by its
//!   producers. Emptiness tests (`== 0`, `> 0`) and size reads for logging
//!   are not caps.
//! - A Java executor created in a method that also shuts it down, or in a
//!   try-with-resources header, queues only the tasks submitted during that
//!   one call.

use auditor_ast::AstNode;

use crate::ast_scan::{descendants, nodes_of_kind};

/// Whether the queue created by `call` is held in a variable whose size the
/// code compares against a limit.
pub(crate) fn size_checked(call: AstNode<'_>) -> bool {
    let Some((holder, scope)) = holder(call) else {
        return false;
    };
    nodes_of_kind(scope, &["comparison_operator", "binary_expression"])
        .into_iter()
        .any(|comparison| caps_size(comparison, &holder))
}

/// The expression text the queue is stored under (`q`, `self.q`, `this.q`
/// with the `this.` dropped) and the region that can read it: the class for
/// a field, the function for a local.
fn holder(call: AstNode<'_>) -> Option<(String, AstNode<'_>)> {
    let parent = call.parent()?;
    let (target, local_kinds): (AstNode<'_>, &[&str]) = match parent.kind() {
        "assignment" => (
            parent.child_by_field_name("left")?,
            &["function_definition"],
        ),
        "assignment_expression" => (parent.child_by_field_name("left")?, &[]),
        "variable_declarator" => (
            parent.child_by_field_name("name")?,
            java_local_scope(parent),
        ),
        _ => return None,
    };
    if parent
        .child_by_field_name("right")
        .or_else(|| parent.child_by_field_name("value"))?
        != call
    {
        return None;
    }
    let is_member = target.kind() != "identifier";
    let scope = if is_member || local_kinds.is_empty() {
        enclosing(call, &CLASS_KINDS)
    } else {
        enclosing(call, local_kinds)
    };
    Some((normalised(target.text()), scope))
}

const CLASS_KINDS: [&str; 5] = [
    "class_definition",
    "class_declaration",
    "enum_declaration",
    "record_declaration",
    "interface_declaration",
];

/// Function kinds that scope a Java local declaration; empty for a field.
fn java_local_scope(declarator: AstNode<'_>) -> &'static [&'static str] {
    match declarator.parent().map(|declaration| declaration.kind()) {
        Some("local_variable_declaration") => &["method_declaration", "constructor_declaration"],
        _ => &[],
    }
}

fn enclosing<'a>(node: AstNode<'a>, kinds: &[&str]) -> AstNode<'a> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if kinds.contains(&parent.kind()) {
            return parent;
        }
        current = parent;
    }
    current
}

fn normalised(text: &str) -> String {
    text.trim_start_matches("this.").to_string()
}

/// `size <op> limit` or `limit <op> size`, with `<op>` an ordering
/// comparison and `limit` anything but the literal `0`.
fn caps_size(comparison: AstNode<'_>, holder: &str) -> bool {
    let Some((left, operator, right)) = comparison_parts(comparison) else {
        return false;
    };
    if !matches!(operator, "<" | "<=" | ">" | ">=") {
        return false;
    }
    let other = if reads_size(left, holder) {
        right
    } else if reads_size(right, holder) {
        left
    } else {
        return false;
    };
    other.text().trim() != "0"
}

fn comparison_parts(comparison: AstNode<'_>) -> Option<(AstNode<'_>, &str, AstNode<'_>)> {
    if comparison.kind() == "binary_expression" {
        let operator = comparison.child_by_field_name("operator")?;
        return Some((
            comparison.child_by_field_name("left")?,
            operator.text(),
            comparison.child_by_field_name("right")?,
        ));
    }
    let mut operands = comparison.named_children();
    let (left, right) = (operands.next()?, operands.next()?);
    let mut operators = comparison.children().filter(|child| !child.is_named());
    let operator = operators.next()?;
    (operands.next().is_none() && operators.next().is_none())
        .then(|| (left, operator.text(), right))
}

/// `q.qsize()`, `len(q)` or `q.size()` on the held queue.
fn reads_size(node: AstNode<'_>, holder: &str) -> bool {
    match node.kind() {
        "call" => {
            let Some(function) = node.child_by_field_name("function") else {
                return false;
            };
            let object = function.child_by_field_name("object");
            let method = function.child_by_field_name("attribute");
            if let (Some(object), Some(method)) = (object, method) {
                return method.text() == "qsize" && object.text() == holder;
            }
            function.text() == "len"
                && node
                    .child_by_field_name("arguments")
                    .and_then(|arguments| arguments.named_child(0))
                    .is_some_and(|argument| argument.text() == holder)
        }
        "method_invocation" => {
            node.child_by_field_name("name")
                .is_some_and(|name| name.text() == "size")
                && node
                    .child_by_field_name("object")
                    .is_some_and(|object| normalised(object.text()) == holder)
        }
        _ => false,
    }
}

/// Whether the Java executor created by `call` lives in one method: a
/// try-with-resources resource, or a local that the method shuts down.
pub(crate) fn scoped_executor(call: AstNode<'_>) -> bool {
    let Some(parent) = call.parent() else {
        return false;
    };
    match parent.kind() {
        "resource" => true,
        "variable_declarator" => {
            !java_local_scope(parent).is_empty()
                && parent
                    .child_by_field_name("name")
                    .is_some_and(|name| shut_down_in_method(call, name.text()))
        }
        "assignment_expression" => parent
            .child_by_field_name("left")
            .filter(|left| left.kind() == "identifier")
            .is_some_and(|left| {
                declared_local(call, left.text()) && shut_down_in_method(call, left.text())
            }),
        _ => false,
    }
}

const METHOD_KINDS: [&str; 2] = ["method_declaration", "constructor_declaration"];

fn declared_local(call: AstNode<'_>, name: &str) -> bool {
    nodes_of_kind(
        enclosing(call, &METHOD_KINDS),
        &["local_variable_declaration"],
    )
    .into_iter()
    .flat_map(|declaration| nodes_of_kind(declaration, &["variable_declarator"]))
    .any(|declarator| {
        declarator
            .child_by_field_name("name")
            .is_some_and(|declared| declared.text() == name)
    })
}

fn shut_down_in_method(call: AstNode<'_>, name: &str) -> bool {
    let method = enclosing(call, &METHOD_KINDS);
    if !METHOD_KINDS.contains(&method.kind()) {
        return false;
    }
    descendants(method)
        .filter(|node| node.kind() == "method_invocation")
        .any(|invocation| {
            invocation
                .child_by_field_name("name")
                .is_some_and(|method| matches!(method.text(), "shutdown" | "shutdownNow" | "close"))
                && invocation
                    .child_by_field_name("object")
                    .is_some_and(|object| object.text() == name)
        })
}

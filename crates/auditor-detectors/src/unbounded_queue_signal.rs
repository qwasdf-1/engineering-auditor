//! Rust unit signal channels for `unbounded_channel_or_queue`: a channel
//! whose element type is `()` carries only wake-ups or cancellation, never
//! data, so it cannot build a data backlog. The element type is read from a
//! turbofish on the constructor, from a type annotation on the `let` that
//! binds the pair, or from the declared type of a struct field in the same
//! file that the sender or receiver is moved into.

use auditor_ast::AstNode;

use crate::ast_scan::{descendants, nodes_of_kind};

/// Whether the channel created by `call` provably carries `()`.
pub(crate) fn is_unit_signal(call: AstNode<'_>) -> bool {
    unit_turbofish(call) || annotated_unit(call) || stored_in_unit_field(call)
}

/// Whether `arguments` holds exactly one type argument, `()`.
fn sole_unit_argument(arguments: AstNode<'_>) -> bool {
    let mut types = arguments.named_children();
    matches!((types.next(), types.next()), (Some(only), None) if only.kind() == "unit_type")
}

/// `Sender<()>`: a generic type whose only argument is `()`.
fn unit_generic(ty: AstNode<'_>) -> bool {
    ty.kind() == "generic_type"
        && ty
            .child_by_field_name("type_arguments")
            .is_some_and(sole_unit_argument)
}

/// `channel::<()>()`.
fn unit_turbofish(call: AstNode<'_>) -> bool {
    call.child_by_field_name("function")
        .filter(|function| function.kind() == "generic_function")
        .and_then(|function| function.child_by_field_name("type_arguments"))
        .is_some_and(sole_unit_argument)
}

/// The `let` that binds the channel pair, when the call is its value.
fn binding(call: AstNode<'_>) -> Option<AstNode<'_>> {
    call.parent()
        .filter(|parent| parent.kind() == "let_declaration")
}

/// `let (tx, rx): (Sender<()>, Receiver<()>) = channel();`.
fn annotated_unit(call: AstNode<'_>) -> bool {
    binding(call)
        .and_then(|binding| binding.child_by_field_name("type"))
        .filter(|ty| ty.kind() == "tuple_type")
        .is_some_and(|ty| {
            let mut parts = ty.named_children().peekable();
            parts.peek().is_some() && parts.all(unit_generic)
        })
}

/// `let (tx, rx) = channel();` where `tx` or `rx` is moved straight into a
/// struct field declared `Sender<()>`/`Receiver<()>` in this file.
fn stored_in_unit_field(call: AstNode<'_>) -> bool {
    let Some(pattern) = binding(call).and_then(|binding| binding.child_by_field_name("pattern"))
    else {
        return false;
    };
    let scope = enclosing_function(call);
    let literals = nodes_of_kind(scope, &["struct_expression"]);
    descendants(pattern)
        .filter(|name| name.kind() == "identifier")
        .any(|name| {
            literals.iter().any(|literal| {
                stored_field(*literal, name.text())
                    .is_some_and(|field| declared_unit_field(*literal, &field))
            })
        })
}

/// The field of struct literal `literal` that is initialised from the
/// variable `name`, as `name` or `field: name`.
fn stored_field(literal: AstNode<'_>, name: &str) -> Option<String> {
    let body = literal.child_by_field_name("body")?;
    body.named_children().find_map(|init| match init.kind() {
        "shorthand_field_initializer" => (init.text() == name).then(|| name.to_string()),
        "field_initializer" => {
            let value = init.child_by_field_name("value")?;
            let field = init.child_by_field_name("field")?;
            (value.kind() == "identifier" && value.text() == name).then(|| field.text().to_string())
        }
        _ => None,
    })
}

/// Whether the struct that `literal` builds declares `field` as a unit
/// generic, such as `UnboundedSender<()>`.
fn declared_unit_field(literal: AstNode<'_>, field: &str) -> bool {
    let Some(struct_name) = literal_struct_name(literal) else {
        return false;
    };
    let root = file_root(literal);
    nodes_of_kind(root, &["struct_item"])
        .into_iter()
        .filter(|item| {
            item.child_by_field_name("name")
                .is_some_and(|name| name.text() == struct_name)
        })
        .flat_map(|item| nodes_of_kind(item, &["field_declaration"]))
        .any(|declaration| {
            declaration
                .child_by_field_name("name")
                .is_some_and(|name| name.text() == field)
                && declaration
                    .child_by_field_name("type")
                    .is_some_and(unit_generic)
        })
}

/// The struct a literal builds, with `Self` resolved through the enclosing
/// `impl` block and any path or generics dropped.
fn literal_struct_name(literal: AstNode<'_>) -> Option<String> {
    let mut name = literal.child_by_field_name("name")?;
    if name.text() == "Self" {
        name = ancestors(literal)
            .find(|node| node.kind() == "impl_item")?
            .child_by_field_name("type")?;
    }
    if name.kind() == "generic_type" {
        name = name.child_by_field_name("type")?;
    }
    name.text().rsplit("::").next().map(str::to_string)
}

fn ancestors(node: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
}

fn enclosing_function(node: AstNode<'_>) -> AstNode<'_> {
    ancestors(node)
        .find(|ancestor| ancestor.kind() == "function_item")
        .unwrap_or_else(|| file_root(node))
}

fn file_root(node: AstNode<'_>) -> AstNode<'_> {
    ancestors(node).last().unwrap_or(node)
}

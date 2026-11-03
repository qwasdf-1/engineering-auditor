//! Axios configs passed by name: `const config = { .. }; axios(config)`.
//! The name resolves to the object literal it was built from when that is
//! the only value it can hold, in the same function as the call, so the
//! call is judged as if the literal were written inline.

use auditor_ast::{function_node_kinds, Assigned, AstNode, FunctionFlow};

use crate::ast_scan;
use crate::js_api_identity::object_properties;

/// Whether the config passed as `name` to `call` is built only from object
/// literals with no `timeout` key and no spread, in the call's function, and
/// no `name.timeout = ..` follows in that function. Parameters, imports,
/// reassigned names and literals from another function resolve to nothing,
/// so they are not judged here.
pub(super) fn named_config_lacks_timeout(call: AstNode<'_>, name: AstNode<'_>) -> bool {
    let flow = FunctionFlow::enclosing(name);
    let function = enclosing_function(call);
    let values = flow.assigned(name);
    !values.is_empty()
        && values.iter().all(|part| match part {
            Assigned::Whole(value) => literal_lacks_timeout(*value, function),
            Assigned::Appended(_) | Assigned::Outside(_) => false,
        })
        && !timeout_assigned(flow.scope(), name.text(), function)
}

/// Whether `value`, looked through type casts, is an object literal in
/// `function` that may not set a timeout.
fn literal_lacks_timeout(value: AstNode<'_>, function: Option<AstNode<'_>>) -> bool {
    let object = unwrap_casts(value);
    object.kind() == "object"
        && enclosing_function(object) == function
        && !object_may_hold_timeout(object)
}

/// `value` with `as`, `satisfies`, and parentheses removed.
fn unwrap_casts(value: AstNode<'_>) -> AstNode<'_> {
    match value.kind() {
        // The value comes first; the type follows it.
        "as_expression" | "satisfies_expression" | "parenthesized_expression" => {
            value.named_child(0).map_or(value, unwrap_casts)
        }
        _ => value,
    }
}

/// Whether an object literal may carry a timeout: a `timeout` key, written
/// as `timeout: x` or shorthand `timeout`, or a spread that may hold one.
pub(super) fn object_may_hold_timeout(object: AstNode<'_>) -> bool {
    object.named_children().any(|property| {
        property.kind() == "spread_element"
            || (property.kind() == "shorthand_property_identifier" && property.text() == "timeout")
    }) || object_properties(object)
        .iter()
        .any(|(key, _)| key == "timeout")
}

/// Whether `function` (or the file's top level, when `None`) assigns
/// `name.timeout` with `=` or a logical assignment.
fn timeout_assigned(scope: AstNode<'_>, name: &str, function: Option<AstNode<'_>>) -> bool {
    ast_scan::nodes_of_kind(
        scope,
        &["assignment_expression", "augmented_assignment_expression"],
    )
    .into_iter()
    .any(|assignment| {
        enclosing_function(assignment) == function
            && assignment
                .child_by_field_name("left")
                .is_some_and(|left| is_member_of(left, name, "timeout"))
    })
}

/// Whether `target` is `name.property`.
fn is_member_of(target: AstNode<'_>, name: &str, property: &str) -> bool {
    target.kind() == "member_expression"
        && target
            .child_by_field_name("object")
            .is_some_and(|object| object.kind() == "identifier" && object.text() == name)
        && target
            .child_by_field_name("property")
            .is_some_and(|field| field.text() == property)
}

/// The function whose body holds `node`, or `None` at the file's top level.
fn enclosing_function(node: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| function_node_kinds().contains(&ancestor.kind()))
}

//! Evaluation order within a function: which binding reaches a read
//! (dominance, a shared loop), and the function scope around a node.

use super::{
    is_anonymous_function, is_named_function, Binding, BindingKind, CONDITIONAL_KINDS, LOOP_KINDS,
};
use crate::AstNode;

/// The named function enclosing `node`, or its root.
pub(super) fn enclosing_scope(node: AstNode<'_>) -> AstNode<'_> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if is_named_function(parent) {
            return parent;
        }
        current = parent;
    }
    current
}

pub(super) fn contains(outer: AstNode<'_>, inner: AstNode<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// `x = x + 1`: the read in the value is evaluated before its own binding.
pub(super) fn evaluates_before_binding(binding: &Binding<'_>, read: AstNode<'_>) -> bool {
    !matches!(
        binding.kind,
        BindingKind::Parameter | BindingKind::LoopVariable
    ) && binding.value.is_some_and(|value| contains(value, read))
}

/// Whether `binding` and `read` share a loop, so a binding after the read
/// reaches it on the next iteration.
pub(super) fn in_same_loop(binding: &Binding<'_>, read: AstNode<'_>) -> bool {
    let Some(common) = common_ancestor(binding.node, read) else {
        return false;
    };
    std::iter::successors(Some(common), AstNode::parent)
        .take_while(|node| !is_named_function(*node))
        .any(|node| LOOP_KINDS.contains(&node.kind()))
}

/// Whether `node` runs whenever `read` does, `node` coming first: no
/// conditional part lies between them and their common ancestor.
pub(super) fn dominates(node: AstNode<'_>, read: AstNode<'_>) -> bool {
    let Some(common) = common_ancestor(node, read) else {
        return false;
    };
    !is_conditional(common)
        && std::iter::successors(node.parent(), AstNode::parent)
            .take_while(|ancestor| *ancestor != common)
            .all(|ancestor| !is_conditional(ancestor))
}

fn is_conditional(node: AstNode<'_>) -> bool {
    CONDITIONAL_KINDS.contains(&node.kind())
        || LOOP_KINDS.contains(&node.kind())
        || is_anonymous_function(node)
        || node.kind() == "binary_expression"
            && node
                .child_by_field_name("operator")
                .is_some_and(|operator| matches!(operator.text(), "&&" | "||" | "??"))
}

fn common_ancestor<'a>(first: AstNode<'a>, second: AstNode<'a>) -> Option<AstNode<'a>> {
    std::iter::successors(Some(first), AstNode::parent).find(|ancestor| contains(*ancestor, second))
}

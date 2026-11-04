//! Dispatch returns: a `return` that is the whole body of one arm of a
//! switch (`case X: return y;`), so the switch maps a value to a result.

use crate::AstNode;

/// One arm of a switch, `match`, or `when`.
const ARM_KINDS: &[&str] = &[
    // C, C++
    "case_statement",
    // Java
    "switch_block_statement_group",
    "switch_rule",
    // C#
    "switch_section",
    // JavaScript, TypeScript
    "switch_case",
    "switch_default",
    // Go
    "expression_case",
    "type_case",
    "default_case",
    // Python
    "case_clause",
    // Rust
    "match_arm",
    // Kotlin
    "when_entry",
];

/// A container that only wraps its statements.
const WRAPPER_KINDS: &[&str] = &[
    "block",
    "compound_statement",
    "statement_block",
    "statement_list",
    "statements",
    "control_structure_body",
    "expression_statement",
];

/// C and C++ preprocessor conditionals, which can wrap some arms of a
/// switch (`#ifdef X case 1: return a; #endif`).
const PREPROCESSOR_KINDS: &[&str] = &[
    "preproc_if",
    "preproc_ifdef",
    "preproc_elif",
    "preproc_elifdef",
    "preproc_else",
];

/// The start byte of the switch whose arm `node`, a return, is the whole
/// body of; `None` when the arm does anything else first. Every arm of one
/// switch shares its parent, so the byte names the switch; an arm inside a
/// preprocessor conditional names the switch the conditional sits in.
pub fn dispatch_switch(node: AstNode<'_>) -> Option<usize> {
    let mut body = node;
    let mut parent = node.parent()?;
    while WRAPPER_KINDS.contains(&parent.kind()) {
        if code_children(parent).count() != 1 {
            return None;
        }
        body = parent;
        parent = parent.parent()?;
    }
    if !ARM_KINDS.contains(&parent.kind()) {
        return None;
    }
    let only_body = code_children(parent)
        .filter(|child| child.start_byte() != body.start_byte())
        .all(|child| !holds_statements(child.kind()));
    only_body.then(|| {
        let mut switch = parent.parent();
        while let Some(conditional) =
            switch.filter(|node| PREPROCESSOR_KINDS.contains(&node.kind()))
        {
            switch = conditional.parent();
        }
        switch.map_or(parent.start_byte(), |switch| switch.start_byte())
    })
}

fn code_children<'a>(node: AstNode<'a>) -> impl Iterator<Item = AstNode<'a>> {
    node.named_children()
        .filter(|child| !child.kind().contains("comment"))
}

/// A statement or statement container, as opposed to an arm's label,
/// pattern, value (`case Kind.A:` is an expression), or guard.
fn holds_statements(kind: &str) -> bool {
    kind.ends_with("statement") || kind.ends_with("declaration") || WRAPPER_KINDS.contains(&kind)
}

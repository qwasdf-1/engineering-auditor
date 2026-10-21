use super::{AstNode, ControlRegionKind};

const CONTROL_KIND_GROUPS: &[(&[&str], ControlRegionKind)] = &[
    (
        &["if", "if_statement", "if_expression", "unless"],
        ControlRegionKind::Conditional,
    ),
    (
        &[
            "for",
            "for_statement",
            "enhanced_for_statement",
            "foreach_statement",
            "for_range_loop",
            "for_in_statement",
            "for_expression",
            "while",
            "while_statement",
            "while_expression",
            "loop_expression",
            "do_statement",
        ],
        ControlRegionKind::Loop,
    ),
    (
        &[
            "match_expression",
            "switch_statement",
            "when_expression",
            "case",
        ],
        ControlRegionKind::Match,
    ),
    (&["try_statement", "try_expression"], ControlRegionKind::Try),
    (
        &["catch_clause", "except_clause", "rescue"],
        ControlRegionKind::Catch,
    ),
    (&["async_block", "async_function"], ControlRegionKind::Async),
];

pub(super) fn control_kind(node_kind: &str) -> Option<ControlRegionKind> {
    CONTROL_KIND_GROUPS
        .iter()
        .find_map(|(kinds, kind)| kinds.contains(&node_kind).then_some(*kind))
}

pub(super) fn nesting_increment(node: AstNode<'_>, kind: ControlRegionKind) -> bool {
    match kind {
        ControlRegionKind::Branch => false,
        ControlRegionKind::Conditional => !is_else_if(node),
        // A handler is a child of its `try`, whose body sits one level in:
        // the handler body sits at that same level.
        ControlRegionKind::Catch => !node
            .parent()
            .is_some_and(|parent| control_kind(parent.kind()) == Some(ControlRegionKind::Try)),
        // Rust's `?` operator parses as a `try_expression` but nests nothing.
        ControlRegionKind::Try => node.children().last().is_none_or(|last| last.kind() != "?"),
        _ => true,
    }
}

pub(super) fn is_control_branch(node: AstNode<'_>) -> bool {
    if matches!(
        node.kind(),
        "match_arm"
            | "switch_case"
            | "switch_default"
            | "switch_section"
            | "switch_rule"
            | "case_clause"
            | "case_statement"
            | "default_statement"
            | "expression_case"
            | "type_case"
            | "communication_case"
            | "when_entry"
    ) {
        return true;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    if control_kind(parent.kind()) != Some(ControlRegionKind::Conditional) {
        return false;
    }
    ["consequence", "alternative", "then", "else", "body"]
        .iter()
        .filter_map(|field| parent.child_by_field_name(field))
        .any(|branch| same_node(branch, node))
}

fn is_else_if(node: AstNode<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if let Some(parent_kind) = control_kind(parent.kind()) {
            if parent_kind != ControlRegionKind::Conditional {
                return false;
            }
            return ["alternative", "else"]
                .iter()
                .filter_map(|field| parent.child_by_field_name(field))
                .any(|branch| direct_alternative_is(branch, node));
        }
        ancestor = parent.parent();
    }
    false
}

fn direct_alternative_is(mut branch: AstNode<'_>, node: AstNode<'_>) -> bool {
    if same_node(branch, node) {
        return true;
    }
    while matches!(branch.kind(), "else" | "else_clause") && branch.named_child_count() == 1 {
        let Some(child) = branch.named_child(0) else {
            return false;
        };
        if same_node(child, node) {
            return true;
        }
        branch = child;
    }
    false
}

fn same_node(left: AstNode<'_>, right: AstNode<'_>) -> bool {
    left.start_byte() == right.start_byte() && left.end_byte() == right.end_byte()
}

//! Statement-shape sites for the brace languages (Go, Java, JavaScript,
//! TypeScript, C#, C, C++): assertions guarding public arguments, assignments used as a
//! whole condition, and empty handlers and branches. The finders return
//! nodes; the owners construct the findings.

use auditor_ast::AstNode;

use crate::ast_scan;

/// A Java `assert` on a parameter of a public method. Assertions are off
/// unless the JVM runs with `-ea`, so they cannot enforce a public method's
/// argument contract (Oracle, "Programming With Assertions"); internal
/// invariants in private code are their intended use.
pub(crate) fn java_public_argument_asserts(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["assert_statement"])
        .into_iter()
        .filter(|assertion| {
            let Some(method) = std::iter::successors(assertion.parent(), |parent| parent.parent())
                .find(|ancestor| {
                    matches!(
                        ancestor.kind(),
                        "method_declaration" | "constructor_declaration"
                    )
                })
            else {
                return false;
            };
            let public = crate::java_syntax::has_java_modifier(method, "public");
            let parameters = method
                .child_by_field_name("parameters")
                .map(|parameters| {
                    ast_scan::nodes_of_kind(parameters, &["formal_parameter"])
                        .into_iter()
                        .filter_map(|parameter| parameter.child_by_field_name("name"))
                        .map(|name| name.text())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let condition = assertion.named_child(0);
            public
                && condition.is_some_and(|condition| {
                    ast_scan::nodes_of_kind(condition, &["identifier"])
                        .into_iter()
                        .chain(
                            std::iter::once(condition).filter(|node| node.kind() == "identifier"),
                        )
                        .any(|identifier| parameters.contains(&identifier.text()))
                })
        })
        .collect()
}

/// An `if`, `while`, or `do` condition that is an assignment and nothing
/// else (`if (x = y)`), ESLint `no-cond-assign`'s default: an assignment
/// wrapped in a comparison (`(line = r.readLine()) != null`) or in extra
/// parentheses is deliberate.
pub(crate) fn assignment_conditions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut conditions =
        ast_scan::nodes_of_kind(root, &["if_statement", "while_statement", "do_statement"])
            .into_iter()
            .filter_map(|statement| statement.child_by_field_name("condition"))
            .filter(|condition| {
                let inner = match condition.kind() {
                    "parenthesized_expression" => condition.named_child(0),
                    // C++ wraps the condition (`condition_clause`).
                    "condition_clause" => condition.child_by_field_name("value"),
                    _ => Some(*condition),
                };
                inner.is_some_and(|inner| {
                    matches!(
                        inner.kind(),
                        "assignment_expression" | "augmented_assignment_expression"
                    )
                })
            })
            .collect::<Vec<_>>();
    conditions.sort_by_key(|condition| condition.start_byte());
    conditions
}

/// A Java `catch` with an empty body, unless a comment explains it or the
/// exception is named `ignored` or `expected` (PMD `EmptyCatchBlock`).
pub(crate) fn java_empty_catches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["catch_clause"])
        .into_iter()
        .filter(|clause| {
            let named_ignored = clause
                .named_children()
                .find(|child| child.kind() == "catch_formal_parameter")
                .and_then(|parameter| parameter.child_by_field_name("name"))
                .is_some_and(|name| matches!(name.text(), "ignored" | "expected"));
            !named_ignored
                && clause
                    .child_by_field_name("body")
                    .is_some_and(|body| body.named_child_count() == 0)
        })
        .collect()
}

/// Go and Java `if` and `else` blocks with nothing in them, not even a
/// comment. An `else if` is a branch of its own.
pub(crate) fn empty_if_branches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut branches = Vec::new();
    for statement in ast_scan::nodes_of_kind(root, &["if_statement"]) {
        for field in ["consequence", "alternative"] {
            if let Some(block) = statement
                .child_by_field_name(field)
                .filter(|block| block.kind() == "block" && block.named_child_count() == 0)
            {
                branches.push(if field == "consequence" {
                    statement
                } else {
                    block
                });
            }
        }
    }
    branches.sort_by_key(|branch| branch.start_byte());
    branches
}

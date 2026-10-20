//! Statement-shape sites for the Tier-2 grammars (Kotlin, C#, C, C++):
//! empty exception handlers and empty `if`/`else` branches. The finders
//! return nodes; the owners construct the findings.

use auditor_ast::AstNode;

use crate::ast_scan;

/// Exception names that state a handler is empty on purpose (PMD
/// `EmptyCatchBlock`'s `allowExceptionNameRegex`).
fn names_ignored_exception(name: &str) -> bool {
    matches!(name, "ignored" | "expected" | "_")
}

/// Kotlin `catch` blocks with nothing in them, not even a comment.
pub(crate) fn kotlin_empty_catches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["catch_block"])
        .into_iter()
        .filter(|block| {
            let mut parts = block.named_children();
            let name = parts
                .next()
                .filter(|part| part.kind() == "simple_identifier");
            !name.is_some_and(|name| names_ignored_exception(name.text()))
                && block
                    .named_children()
                    .all(|part| matches!(part.kind(), "simple_identifier" | "user_type"))
        })
        .collect()
}

/// C# and C++ `catch` clauses whose body holds nothing, not even a comment.
pub(crate) fn braced_empty_catches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["catch_clause"])
        .into_iter()
        .filter(|clause| {
            let named_ignored = ast_scan::nodes_of_kind(*clause, &["identifier"])
                .into_iter()
                .filter(|identifier| {
                    identifier.parent().is_some_and(|parent| {
                        matches!(
                            parent.kind(),
                            "catch_declaration" | "reference_declarator" | "parameter_declaration"
                        )
                    })
                })
                .any(|name| names_ignored_exception(name.text()));
            !named_ignored
                && clause
                    .child_by_field_name("body")
                    .is_some_and(|body| body.named_child_count() == 0)
        })
        .collect()
}

/// Kotlin `if` and `else` bodies written as empty braces. An `else if` is a
/// branch of its own.
pub(crate) fn kotlin_empty_branches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut branches = Vec::new();
    for expression in ast_scan::nodes_of_kind(root, &["if_expression"]) {
        for field in ["consequence", "alternative"] {
            if let Some(body) = expression.child_by_field_name(field).filter(|body| {
                body.text().trim_start().starts_with('{') && body.named_child_count() == 0
            }) {
                branches.push(if field == "consequence" {
                    expression
                } else {
                    body
                });
            }
        }
    }
    branches.sort_by_key(|branch| branch.start_byte());
    branches
}

/// C and C++ `if` and `else` blocks with nothing in them, not even a
/// comment.
pub(crate) fn c_empty_branches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let empty =
        |block: AstNode<'_>| block.kind() == "compound_statement" && block.named_child_count() == 0;
    let mut branches = Vec::new();
    for statement in ast_scan::nodes_of_kind(root, &["if_statement"]) {
        if statement
            .child_by_field_name("consequence")
            .is_some_and(empty)
        {
            branches.push(statement);
        }
        if let Some(block) = statement
            .child_by_field_name("alternative")
            .and_then(|clause| clause.named_child(0))
            .filter(|block| empty(*block))
        {
            branches.push(block);
        }
    }
    branches.sort_by_key(|branch| branch.start_byte());
    branches
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    fn count(
        language: AstLang,
        source: &str,
        finder: fn(AstNode<'_>) -> Vec<AstNode<'_>>,
    ) -> usize {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        finder(tree.root_node()).len()
    }

    #[test]
    fn empty_handlers_and_branches_in_tier_two_grammars() {
        assert_eq!(
            count(AstLang::Kotlin, "fun f() { try { g() } catch (e: Exception) { } }\nfun h() { try { g() } catch (ignored: Exception) { }\n try { g() } catch (e: Exception) { log(e) } }\n", kotlin_empty_catches),
            1
        );
        assert_eq!(
            count(AstLang::CSharp, "class C { void F() { try { G(); } catch (Exception) { } try { G(); } catch (Exception ignored) { } try { G(); } catch { /* best effort */ } } }\n", braced_empty_catches),
            1
        );
        assert_eq!(
            count(AstLang::Cpp, "void f() { try { g(); } catch (...) { } try { g(); } catch (const std::exception& e) { log(e); } }\n", braced_empty_catches),
            1
        );
        assert_eq!(
            count(
                AstLang::Kotlin,
                "fun f(x: Int) { if (x > 0) { } else { g() }\n if (x < 0) g() else { } }\n",
                kotlin_empty_branches
            ),
            2
        );
        assert_eq!(
            count(AstLang::C, "void f(int x) { if (x) { } else if (x > 1) { g(); } else { } if (x) { /* why */ } }\n", c_empty_branches),
            2
        );
    }
}

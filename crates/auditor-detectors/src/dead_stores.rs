//! Ineffectual assignments (golangci-lint `ineffassign`): in a straight-line
//! block, a value assigned to a local and overwritten before any statement
//! reads it (ESLint `no-useless-assignment`). Go, Python, Rust,
//! JavaScript/TypeScript, Java, Kotlin, C#, C, and C++; any mention of the
//! name in a statement, including a Rust format string's `{name}`, counts as
//! a read.

use std::collections::BTreeMap;

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::assignment_shapes::{mentions, simple_assignment, LocalNames};
use crate::ast_scan::{self, AstScan};
use crate::{util, Language};

pub(crate) fn unused_assignment_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let language = &file.language;
        let blocks: &[&str] = match language {
            Language::Go => &["statement_list"],
            Language::Python => &["block", "module"],
            Language::JavaScript | Language::TypeScript => &["statement_block", "program"],
            Language::Kotlin => &["statements"],
            Language::C | Language::Cpp => &["compound_statement"],
            _ => &["block"],
        };
        for block in ast_scan::nodes_of_kind(tree.root_node(), blocks) {
            if *language == Language::Rust && ast_scan::is_inside_rust_test(block) {
                continue;
            }
            for (target, overwritten) in overwritten_stores(language, block) {
                let line = overwritten.start_position().0 + 1;
                findings.push(
                        auditor_core::finding!(
                            "unused_assignment",
                            "correctness",
                            "Ineffectual assignment",
                            format!(
                                "Value assigned to `{target}` at line {line} is overwritten before it is read."
                            ),
                            Severity::Medium,
                            Confidence::High,
                            &relative,
                            Some(line),
                        )
                        .with_recommendation(
                            "Remove the overwritten assignment or consume its value before reassigning the variable.",
                        )
                        .with_evidence("variable", target.as_str())
                        .with_evidence("analysis", "ast"),
                    );
            }
        }
    });
    findings
}

/// `(name, statement)` for each assignment in `block` whose value the next
/// assignment to the same name discards unread.
fn overwritten_stores<'a>(language: &Language, block: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
    let mut stores = Vec::new();
    let Some(locals) = LocalNames::of(language, block) else {
        return stores;
    };
    // In a `try` body any call may throw, so a handler (`catch`, `except`,
    // `finally`) may read the value of every assignment before it.
    let handler_reads = block
        .parent()
        .filter(|parent| parent.kind().contains("try"))
        .map(|parent| {
            parent
                .named_children()
                .filter(|child| child.start_byte() != block.start_byte())
                .flat_map(mentions)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut pending = BTreeMap::<String, AstNode<'a>>::new();
    for statement in block.named_children() {
        // A jump or a jump target breaks the straight line: the value may be
        // read on the path through the label.
        if matches!(statement.kind(), "labeled_statement" | "goto_statement")
            || !ast_scan::nodes_of_kind(statement, &["goto_statement"]).is_empty()
        {
            pending.clear();
            continue;
        }
        let Some((target, value)) = simple_assignment(language, statement) else {
            for identifier in mentions(statement) {
                pending.remove(&identifier);
            }
            continue;
        };
        for identifier in mentions(value) {
            pending.remove(&identifier);
        }
        if let Some(previous) = pending.remove(target) {
            stores.push((target.to_string(), previous));
        }
        if locals.contains(target) && !handler_reads.contains(target) {
            pending.insert(target.to_string(), statement);
        }
    }
    stores
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overwritten(language: Language, ast: auditor_ast::AstLang, source: &str) -> Vec<String> {
        let tree = auditor_ast::AstParser::new()
            .unwrap()
            .parse(&ast, source)
            .unwrap();
        ast_scan::nodes_of_kind(tree.root_node(), &["block"])
            .into_iter()
            .flat_map(|block| overwritten_stores(&language, block))
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn a_handler_may_read_what_a_try_body_assigned() {
        let source = "class F { void open() {\n  Closeable toClose = null;\n  try {\n    toClose = raw();\n    toClose = decorate(toClose);\n    int n = 1;\n    n = 2;\n    use(n);\n  } catch (IOException e) {\n    close(toClose);\n  }\n} }\n";
        assert_eq!(
            overwritten(Language::Java, auditor_ast::AstLang::Java, source),
            ["n"]
        );
    }
}

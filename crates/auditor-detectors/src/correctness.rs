use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::braced_statement_shapes::java_public_argument_asserts;
use crate::c_api_rules::c_todo_nodes;
use crate::csharp_api_rules::{
    csharp_dynamic_evaluation, csharp_public_argument_asserts, csharp_todo_nodes,
};
use crate::go_api_rules::go_todo_nodes;
use crate::java_api_rules::{java_dynamic_evaluation, java_todo_nodes};
use crate::js_api_rules::{js_dynamic_evaluation, js_todo_nodes};
use crate::kotlin_api_rules::{
    kotlin_dynamic_evaluation, kotlin_public_argument_asserts, kotlin_todo_nodes,
};
use crate::python_api_rules::py_todo_nodes;
use crate::{util, Language};

pub(crate) fn todo_unimplemented_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        // Deferred work in build scripts and tool configuration is not
        // unfinished production code.
        if file.role == crate::SourceRole::Tooling {
            return;
        }
        let relative = util::path_slash(&file.relative);
        let root = tree.root_node();
        let todos = if file.language == Language::Go {
            go_todo_nodes(root)
        } else if file.language == Language::Kotlin {
            kotlin_todo_nodes(root)
        } else if file.language == Language::CSharp {
            csharp_todo_nodes(root)
        } else if matches!(file.language, Language::C | Language::Cpp) {
            c_todo_nodes(root)
        } else if file.language == Language::Java {
            java_todo_nodes(root)
        } else if matches!(file.language, Language::JavaScript | Language::TypeScript) {
            js_todo_nodes(root)
        } else if file.language == Language::Python {
            py_todo_nodes(root)
        } else {
            ast_scan::nodes_of_kind(root, &["macro_invocation", "line_comment", "block_comment"])
                .into_iter()
                .filter(|node| !ast_scan::is_inside_rust_test(*node) && is_todo_node(*node))
                .collect()
        };
        for node in todos {
            let line = node.start_position().0 + 1;
            findings.push(
                    auditor_core::finding!(
                        "todo_unimplemented_in_production",
                        "correctness",
                        "TODO in production code",
                        format!("Non-test source contains TODO at line {line}"),
                        Severity::High,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Resolve TODOs before shipping: track them in your issue tracker instead of leaving them in production code.",
                    )
                    .with_evidence("todo", node.text().trim())
                    .with_evidence("analysis", "ast"),
                );
        }
    });
    findings
}

pub(crate) fn assert_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        // Python strips asserts under `-O`, the JVM (Java, Kotlin) unless
        // `-ea`, and C# release builds drop `Debug.Assert`; Rust's `assert!`
        // always runs.
        |language| {
            matches!(
                language,
                Language::Python | Language::Java | Language::Kotlin | Language::CSharp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            let asserts = if file.language == Language::Java {
                java_public_argument_asserts(tree.root_node())
            } else if file.language == Language::Kotlin {
                kotlin_public_argument_asserts(tree.root_node())
            } else if file.language == Language::CSharp {
                csharp_public_argument_asserts(tree.root_node())
            } else {
                ast_scan::nodes_of_kind(tree.root_node(), &["assert_statement"])
                    .into_iter()
                    .filter(|node| !node.named_child(0).is_some_and(is_type_narrowing))
                    .collect()
            };
            for node in asserts {
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "assert_in_production",
                        "correctness",
                        "Assertion call in production code",
                        format!("Non-test source calls `assert` at line {line}"),
                        Severity::Medium,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Replace assertions with proper error handling or guard clauses.",
                    )
                    .with_evidence("source", "ast"),
                );
            }
        },
    );
    findings
}

pub(crate) fn eval_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Python
                    | Language::Java
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::Kotlin
                    | Language::CSharp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            let (calls, api_sites) = match file.language {
                Language::Java => (java_dynamic_evaluation(tree.root_node()), true),
                Language::Kotlin => (kotlin_dynamic_evaluation(tree.root_node()), true),
                Language::CSharp => (csharp_dynamic_evaluation(tree.root_node()), true),
                Language::JavaScript | Language::TypeScript => {
                    (js_dynamic_evaluation(tree.root_node()), true)
                }
                _ => (ast_scan::nodes_of_kind(tree.root_node(), &["call"]), false),
            };
            for node in calls {
                let callee = if api_sites {
                    "eval"
                } else {
                    ast_scan::callee(node)
                };
                let (title, recommendation) = match callee {
                    "eval" => (
                        "Eval call in production code",
                        "Replace eval with a parser or a domain-specific evaluation function.",
                    ),
                    "exec" => (
                        "Exec call in production code",
                        "Replace exec with a subprocess library or domain-specific execution function.",
                    ),
                    _ => continue,
                };
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "eval_in_production",
                        "correctness",
                        title,
                        format!("Non-test source calls dynamic execution at line {line}"),
                        Severity::Medium,
                        Confidence::High,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(recommendation)
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

fn is_todo_node(node: AstNode<'_>) -> bool {
    let text = node.text().trim().to_ascii_lowercase();
    match node.kind() {
        "macro_invocation" => text.starts_with("todo!") || text.starts_with("unimplemented!"),
        "line_comment" | "block_comment" => {
            text.trim_start_matches(['/', '*', ' ']).starts_with("todo")
        }
        _ => false,
    }
}

/// `isinstance(..)`, `issubclass(..)`, or `x is not None`, alone or joined by
/// `and`: an assert that tells the type checker what the code already
/// guarantees, and that can be removed without changing behavior.
fn is_type_narrowing(condition: AstNode<'_>) -> bool {
    match condition.kind() {
        "call" => matches!(ast_scan::callee(condition), "isinstance" | "issubclass"),
        "comparison_operator" => {
            let text = condition.text();
            text.trim_end().ends_with("is not None") && text.matches(" is ").count() == 1
        }
        "boolean_operator" => {
            condition
                .child_by_field_name("operator")
                .is_some_and(|operator| operator.text() == "and")
                && condition.named_children().all(is_type_narrowing)
        }
        "parenthesized_expression" => condition.named_children().all(is_type_narrowing),
        _ => false,
    }
}

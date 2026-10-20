use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::braced_statement_shapes::{
    assignment_conditions, empty_if_branches, java_empty_catches,
};
use crate::c_api_rules::{c_console_output, c_debugger_calls, c_exits, c_unclosed_files};
use crate::csharp_api_rules::{
    csharp_console_output, csharp_debugger_calls, csharp_exits, csharp_unclosed_streams,
};
use crate::go_api_rules::{go_calls_to, go_unclosed_files, is_go_main_package, GO_STDOUT_APIS};
use crate::java_api_rules::{java_console_output, java_exits, java_unclosed_streams};
use crate::js_api_rules::{is_js_program, js_exits, js_unclosed_files};
use crate::kotlin_api_rules::{kotlin_console_output, kotlin_exits, kotlin_unclosed_streams};
use crate::python_api_rules::{py_console_output, py_debugger_calls, py_exits};
use crate::rust_api_rules::{is_rust_program, rust_console_output, rust_debug_macros, rust_exits};
use crate::statement_shapes::{py_broad_empty_handlers, py_empty_branches, rust_empty_branches};
use crate::tier_two_statement_shapes::{
    braced_empty_catches, c_empty_branches, kotlin_empty_branches, kotlin_empty_catches,
};
use crate::util;
use crate::{AuditedFile, Language};

pub(crate) use crate::async_rules::{no_await_in_loop_findings, require_atomic_updates_findings};
pub(crate) use crate::errorprone_language_rules::{
    cpp_raw_ptr_ownership_findings, csharp_linq_enumeration_findings, kotlin_empty_when_findings,
    ruby_double_bang_findings,
};

pub(crate) fn assignment_in_operand_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Rust
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::Java
                    | Language::CSharp
                    | Language::C
                    | Language::Cpp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            let conditions = if file.language == Language::Rust {
                rust_assignment_conditions(tree.root_node())
            } else {
                assignment_conditions(tree.root_node())
            };
            for condition in conditions {
                let line = condition.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "assignment_in_operand",
                        "correctness",
                        "Assignment used in conditional expression",
                        format!("Assignment in if/while condition at line {line}"),
                        Severity::Medium,
                        Confidence::High,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Extract the assignment to a separate statement before the condition.",
                    )
                    .with_evidence("pattern", condition.text().trim())
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

/// Rust `if`/`while` conditions containing an assignment (not `if let`).
fn rust_assignment_conditions(root: auditor_ast::AstNode<'_>) -> Vec<auditor_ast::AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["if_expression", "while_expression"])
        .into_iter()
        .filter_map(|owner| owner.child_by_field_name("condition"))
        .filter(|condition| {
            let assignment = ["assignment_expression", "compound_assignment_expr"];
            condition.kind() != "let_condition"
                && (assignment.contains(&condition.kind())
                    || !ast_scan::nodes_of_kind(*condition, &assignment).is_empty())
        })
        .collect()
}

pub(crate) fn resource_not_closed_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Python
                    | Language::Go
                    | Language::Java
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::Kotlin
                    | Language::CSharp
                    | Language::C
            )
        },
        |file, tree, _| {
            let root = tree.root_node();
            let relative = util::path_slash(&file.relative);
            let api_sites = match file.language {
                Language::Go => Some(go_unclosed_files(root)),
                Language::Kotlin => Some(kotlin_unclosed_streams(root)),
                Language::CSharp => Some(csharp_unclosed_streams(root)),
                Language::C => Some(c_unclosed_files(root)),
                Language::Java => Some(java_unclosed_streams(root)),
                Language::JavaScript | Language::TypeScript => Some(js_unclosed_files(root)),
                _ => None,
            };
            if let Some(sites) = api_sites {
                for assignment in sites {
                    findings.push(resource_not_closed_finding(&relative, assignment));
                }
                return;
            }
            let calls = ast_scan::nodes_of_kind(root, &["call"]);
            for assignment in ast_scan::nodes_of_kind(root, &["assignment"]) {
                let Some(target) = assignment.child_by_field_name("left") else {
                    continue;
                };
                let Some(value) = assignment.child_by_field_name("right") else {
                    continue;
                };
                let target_name = target.text().trim();
                let opens_resource = value.kind() == "call" && ast_scan::callee(value) == "open"
                    || ast_scan::nodes_of_kind(value, &["call"])
                        .iter()
                        .any(|call| ast_scan::callee(*call) == "open");
                if !is_simple_identifier(target_name)
                    || !opens_resource
                    || resource_is_closed(assignment, target_name, &calls)
                {
                    continue;
                }
                findings.push(resource_not_closed_finding(&relative, assignment));
            }
        },
    );
    findings
}

fn resource_not_closed_finding(relative: &str, assignment: auditor_ast::AstNode<'_>) -> Finding {
    let line = assignment.start_position().0 + 1;
    auditor_core::finding!(
        "resource_not_closed",
        "correctness",
        "Resource may not be closed",
        format!("File or resource opened at line {line} may not be closed."),
        Severity::Medium,
        Confidence::Low,
        relative,
        Some(line),
    )
    .with_recommendation("Ensure the resource is closed (use with/using/defer or manual close).")
    .with_evidence("pattern", assignment.text().trim())
    .with_evidence("analysis", "ast")
}

fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn resource_is_closed(
    assignment: auditor_ast::AstNode<'_>,
    target: &str,
    calls: &[auditor_ast::AstNode<'_>],
) -> bool {
    let owner = enclosing_function_span(assignment);
    calls.iter().any(|call| {
        call.start_byte() > assignment.end_byte()
            && ast_scan::callee(*call) == format!("{target}.close")
            && enclosing_function_span(*call) == owner
    })
}

fn enclosing_function_span(mut node: auditor_ast::AstNode<'_>) -> Option<(usize, usize)> {
    while let Some(parent) = node.parent() {
        if matches!(parent.kind(), "function_definition" | "lambda") {
            return Some((parent.start_byte(), parent.end_byte()));
        }
        node = parent;
    }
    None
}

pub(crate) fn empty_conditional_branch_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let branches = if file.language == Language::Python {
            py_empty_branches(tree.root_node())
        } else if file.language == Language::Rust {
            rust_empty_branches(tree.root_node())
        } else if matches!(
            file.language,
            Language::Go | Language::Java | Language::CSharp
        ) {
            empty_if_branches(tree.root_node())
        } else if file.language == Language::Kotlin {
            kotlin_empty_branches(tree.root_node())
        } else if matches!(file.language, Language::C | Language::Cpp) {
            c_empty_branches(tree.root_node())
        } else {
            ast_scan::nodes_of_kind(
                tree.root_node(),
                &["if_statement", "while_statement", "for_statement"],
            )
            .into_iter()
            .filter(|node| find_body_node(node).is_some_and(|body| is_empty_body(&body)))
            .collect()
        };
        for node in branches {
            let line = node.start_position().0 + 1;
            findings.push(
                auditor_core::finding!(
                    "empty_conditional_branch",
                    "correctness",
                    "Empty conditional branch body (AST-detected)",
                    format!("Empty {} body at line {line}", node.kind()),
                    Severity::Medium,
                    Confidence::High,
                    &relative,
                    Some(line),
                )
                .with_recommendation("Remove the empty branch or add a meaningful body.")
                .with_evidence("kind", node.kind())
                .with_evidence("analysis", "ast"),
            );
        }
    });
    findings
}

fn find_body_node<'a>(node: &'a auditor_ast::AstNode<'a>) -> Option<auditor_ast::AstNode<'a>> {
    for field in ["body", "consequence"] {
        if let Some(child) = node.child_by_field_name(field) {
            return Some(child);
        }
    }
    for child in node.named_children() {
        let k = child.kind();
        if k == "body"
            || k == "block"
            || k == "statement_block"
            || k == "then"
            || k == "consequence"
        {
            return Some(child);
        }
    }
    // Fallback: last named child is often the body
    node.named_children().last()
}

fn is_empty_body(body: &auditor_ast::AstNode<'_>) -> bool {
    matches!(body.kind(), "empty_statement")
        || matches!(body.kind(), "block" | "statement_block" | "body")
            && body.named_child_count() == 0
}

/// Exit-in-production detection (absorbs go_os_exit, java_system_exit)
pub(crate) fn exit_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let root = tree.root_node();
        if is_process_boundary_path(&relative)
            || (file.language == Language::Rust && is_rust_program(file))
            || is_program_entry(file, root)
        {
            return;
        }
        let exits = if file.language == Language::Go {
            go_calls_to(root, &["os.Exit"])
        } else if file.language == Language::Java {
            java_exits(root)
        } else if matches!(file.language, Language::JavaScript | Language::TypeScript) {
            js_exits(root)
        } else if file.language == Language::Python {
            py_exits(file, root)
        } else if file.language == Language::Kotlin {
            kotlin_exits(root)
        } else if file.language == Language::CSharp {
            csharp_exits(root)
        } else if matches!(file.language, Language::C | Language::Cpp) {
            c_exits(root)
        } else {
            rust_exits(root)
        };
        for node in exits {
            let line = node.start_position().0 + 1;
            findings.push(
                auditor_core::finding!(
                    "exit_in_production",
                    "correctness",
                    "Process exit call in production code",
                    format!("Exit call at line {line}"),
                    Severity::High,
                    Confidence::High,
                    &relative,
                    Some(line),
                )
                .with_recommendation(
                    "Replace process exit with structured error propagation or graceful shutdown.",
                )
                .with_evidence("pattern", node.text().trim())
                .with_evidence("analysis", "ast"),
            );
        }
    });
    findings
}

fn is_process_boundary_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let name = std::path::Path::new(&lower)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    lower
        .split('/')
        .any(|segment| matches!(segment, "tooling" | "scripts" | "bin"))
        || lower.contains("sidecar")
        || name.starts_with("main.")
        || name.contains("_cli.")
}

/// Unhandled termination detection (absorbs panic_in_non_test, go_panic_in_production,
/// java_throw_runtime_exception, kt_throw_in_production)
pub(crate) fn unhandled_termination_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| *language == Language::Rust,
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            for node in ast_scan::nodes_of_kind(tree.root_node(), &["macro_invocation"]) {
                let text = node.text().trim_start();
                if ast_scan::is_inside_rust_test(node)
                    || !["panic!", "unreachable!"]
                        .iter()
                        .any(|name| text.starts_with(name))
                {
                    continue;
                }
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "unhandled_termination_in_production",
                        "correctness",
                        "Unhandled termination in production code",
                        format!("Unhandled termination at line {line}"),
                        Severity::High,
                        Confidence::High,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Replace the panic/throw with a Result return or structured error type.",
                    )
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

/// A Go `main` package or a JavaScript/TypeScript program: an entry point,
/// whose exits and output are its interface.
fn is_program_entry(file: &AuditedFile, root: auditor_ast::AstNode<'_>) -> bool {
    is_go_main_package(file, root)
        || (matches!(file.language, Language::JavaScript | Language::TypeScript)
            && is_js_program(file, root))
}

pub(crate) fn console_output_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let root = tree.root_node();
        if is_console_tooling_path(&relative) || is_program_entry(file, root) {
            return;
        }
        let outputs = if file.language == Language::Go {
            go_calls_to(root, GO_STDOUT_APIS)
        } else if file.language == Language::Java {
            java_console_output(root)
        } else if file.language == Language::Python {
            py_console_output(file, root)
        } else if file.language == Language::Rust {
            rust_console_output(file, root)
        } else if file.language == Language::Kotlin {
            kotlin_console_output(root)
        } else if file.language == Language::CSharp {
            csharp_console_output(root)
        } else if matches!(file.language, Language::C | Language::Cpp) {
            c_console_output(root)
        } else {
            ast_scan::nodes_of_kind(root, &["call_expression"])
                .into_iter()
                .filter(|node| {
                    matches!(
                        ast_scan::callee(*node),
                        "console.log" | "console.error" | "console.warn"
                    )
                })
                .collect()
        };
        for node in outputs
            .into_iter()
            .filter(|node| !inside_logger_class(*node))
        {
            let line = node.start_position().0 + 1;
            findings.push(
                auditor_core::finding!(
                    "console_output_in_production",
                    "correctness",
                    "Console output in production code",
                    format!("Console output in production code at line {line}"),
                    Severity::Low,
                    Confidence::High,
                    &relative,
                    Some(line),
                )
                .with_recommendation("Remove console output or replace with structured logging.")
                .with_evidence("pattern", node.text().trim())
                .with_evidence("analysis", "ast"),
            );
        }
    });
    findings
}

pub(crate) fn debugger_in_production_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    site_findings(
        scan,
        (
            "debugger_in_production",
            "Debugger statement in production code",
        ),
        Severity::High,
        "Remove debugger statements before committing.",
        |file, root| match file.language {
            Language::Python => py_debugger_calls(root),
            Language::Rust => rust_debug_macros(root),
            Language::CSharp => csharp_debugger_calls(root),
            Language::C | Language::Cpp => c_debugger_calls(root),
            _ => ast_scan::nodes_of_kind(root, &["debugger_statement"]),
        },
    )
}

pub(crate) fn empty_catch_block_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    site_findings(
        scan,
        ("empty_catch_block", "Empty catch/except block"),
        Severity::Medium,
        "Handle the exception or explain why it is intentionally swallowed.",
        |file, root| match file.language {
            Language::Python => py_broad_empty_handlers(root),
            Language::Java => java_empty_catches(root),
            Language::Kotlin => kotlin_empty_catches(root),
            Language::CSharp | Language::Cpp => braced_empty_catches(root),
            _ => ast_scan::nodes_of_kind(root, &["catch_clause"])
                .into_iter()
                .filter(|node| {
                    node.child_by_field_name("body")
                        .is_some_and(|body| is_empty_body(&body))
                })
                .collect(),
        },
    )
}

/// Whether `node` sits in a class named `*Logger`: a console logger sink,
/// whose contract is to print.
fn inside_logger_class(node: auditor_ast::AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .filter(|ancestor| {
            matches!(
                ancestor.kind(),
                "class_declaration" | "class_definition" | "class_specifier" | "object_declaration"
            )
        })
        .filter_map(|class| {
            class.child_by_field_name("name").or_else(|| {
                class
                    .named_children()
                    .find(|child| child.kind() == "type_identifier")
            })
        })
        .any(|name| name.text().ends_with("Logger"))
}

/// CLI entry points, build scripts, sandboxes, examples, and fixtures, where
/// console output is the program's interface rather than stray logging.
fn is_console_tooling_path(relative: &str) -> bool {
    const TOOLING_SEGMENTS: &[&str] = &[
        "bin",
        "build",
        "example",
        "examples",
        "fixtures",
        "sandbox",
        "script",
        "scripts",
        "test_files",
        "tooling",
        "tools",
    ];
    let mut segments = relative.split('/').collect::<Vec<_>>();
    let file_name = segments.pop().unwrap_or_default();
    segments
        .iter()
        .any(|segment| TOOLING_SEGMENTS.contains(segment))
        || file_name.starts_with("gulpfile.")
        || file_name.starts_with("Gruntfile.")
        || file_name.starts_with("Makefile.")
        || file_name.starts_with("Jakefile.")
        // Build and tool configuration (`vite.config.ts`, `tsdown.config.ts`).
        || file_name.contains(".config.")
}

/// Findings at the sites `sites` returns for each Rust, Python, JavaScript,
/// TypeScript, Java, Kotlin, C#, C, and C++ file.
fn site_findings(
    scan: AstScan<'_, '_>,
    (detector, title): (&'static str, &'static str),
    severity: Severity,
    recommendation: &'static str,
    sites: for<'a> fn(&AuditedFile, auditor_ast::AstNode<'a>) -> Vec<auditor_ast::AstNode<'a>>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Rust
                    | Language::Python
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::Java
                    | Language::Kotlin
                    | Language::CSharp
                    | Language::C
                    | Language::Cpp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            for node in sites(file, tree.root_node()) {
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        detector,
                        "correctness",
                        title,
                        format!("{title} at line {line}"),
                        severity,
                        Confidence::High,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(recommendation)
                    .with_evidence("pattern", node.text().trim())
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

#[cfg(test)]
include!("errorprone_tests.rs");

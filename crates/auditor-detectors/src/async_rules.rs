//! Async ordering rules: the grammar-backed await-in-loop rule (JavaScript,
//! TypeScript, Python, Rust, C#) and ESLint `require-atomic-updates`
//! (JavaScript/TypeScript, lexical).

use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::statement_shapes::{
    csharp_awaits_in_loops, js_awaits_in_loops, py_awaits_in_loops, rust_awaits_in_loops,
};
use crate::{util, AuditedFile, Language};

/// ESLint require-atomic-updates: detects async read-modify-write races in JS/TS
pub(crate) fn require_atomic_updates_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        if !matches!(
            file.language,
            crate::Language::JavaScript | crate::Language::TypeScript
        ) {
            continue;
        }
        if util::is_test_source_path(&file.relative) {
            continue;
        }
        let rel = util::path_slash(&file.relative);
        let text = cache.text(file);
        for (line_number, line) in text.lines().enumerate() {
            let code = util::strip_comments(line);
            let trimmed = code.trim();
            if trimmed.contains("await") {
                let has_compound = trimmed.contains("+=")
                    || trimmed.contains("-=")
                    || trimmed.contains("*=")
                    || trimmed.contains("/=");
                if has_compound {
                    findings.push(
                        auditor_core::finding!(
                            "require_atomic_updates",
                            "correctness",
                            "Possible race condition in async read-modify-write (ESLint require-atomic-updates)",
                            format!(
                                "Async read-modify-write at line {} may produce a race condition",
                                line_number + 1
                            ),
                            Severity::Medium,
                            Confidence::Low,
                            &rel,
                            Some(line_number + 1),
                        )
                        .with_recommendation("Use a lock or read the value after the await to ensure atomicity.")
                        .with_evidence("pattern", trimmed),
                    );
                }
            }
        }
    }
    findings
}

/// `await` serializing a loop, read from the grammar (ESLint
/// `no-await-in-loop`): JavaScript, TypeScript, Python, Rust, and C#.
pub(crate) fn no_await_in_loop_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Python
                    | Language::Rust
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::CSharp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            let awaits = match file.language {
                Language::Rust => rust_awaits_in_loops(tree.root_node()),
                Language::Python => py_awaits_in_loops(tree.root_node()),
                Language::CSharp => csharp_awaits_in_loops(tree.root_node()),
                _ => js_awaits_in_loops(tree.root_node()),
            };
            for node in awaits {
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "no_await_in_loop",
                        "correctness",
                        "Await inside loop (ESLint no-await-in-loop)",
                        format!("Await expression inside a loop at line {line}"),
                        Severity::Medium,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation("Consider moving the awaited operation outside the loop or gathering the awaits concurrently.")
                    .with_evidence("pattern", node.text().trim())
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

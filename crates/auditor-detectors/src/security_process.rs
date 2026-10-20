use auditor_core::{Confidence, Finding, Severity};

use auditor_ast::AstNode;

use crate::ast_scan::{self, AstScan};
use crate::c_api_rules::c_shell_commands;
use crate::csharp_api_rules::csharp_shell_commands;
use crate::go_api_rules::go_shell_commands;
use crate::java_api_rules::java_shell_commands;
use crate::js_api_rules::js_shell_commands;
use crate::kotlin_api_rules::kotlin_shell_commands;
use crate::rust_api_rules::rust_shell_commands;
use crate::{util, Language};

pub(crate) fn shell_execution_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let sites = match file.language {
            Language::Go => go_shell_commands(tree.root_node()),
            Language::Java => java_shell_commands(tree.root_node()),
            Language::JavaScript | Language::TypeScript => js_shell_commands(tree.root_node()),
            Language::Rust => rust_shell_commands(tree.root_node()),
            Language::Kotlin => kotlin_shell_commands(tree.root_node()),
            Language::CSharp => csharp_shell_commands(tree.root_node()),
            Language::C | Language::Cpp => c_shell_commands(tree.root_node()),
            _ => python_shell_calls(tree.root_node()),
        };
        for node in sites {
            let line = node.start_position().0 + 1;
            findings.push(
                auditor_core::finding!(
                    "shell_execution",
                    "correctness",
                    "Shell execution in production code",
                    format!("Shell execution at line {line}"),
                    Severity::Medium,
                    Confidence::Medium,
                    &relative,
                    Some(line),
                )
                .with_recommendation("Use a safer API or validate all shell arguments.")
                .with_evidence("pattern", node.text().trim())
                .with_evidence("analysis", "ast"),
            );
        }
    });
    findings
}

fn python_shell_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["call"])
        .into_iter()
        .filter(|node| {
            let callee = ast_scan::callee(*node);
            let compact = node
                .text()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .to_ascii_lowercase();
            callee == "os.system"
                || matches!(
                    callee,
                    "subprocess.call" | "subprocess.Popen" | "subprocess.run"
                ) && compact.contains("shell=true")
        })
        .collect()
}

use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::c_api_rules::c_weak_cryptography;
use crate::csharp_api_rules::csharp_weak_cryptography;
use crate::java_api_rules::java_weak_cryptography;
use crate::js_api_rules::js_weak_cryptography;
use crate::kotlin_api_rules::kotlin_weak_cryptography;
use crate::python_api_rules::py_weak_cryptography;
use crate::rust_api_rules::rust_weak_cryptography;
use crate::{util, Language};

pub(crate) fn insecure_cryptography_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, Language::is_tiered, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let root = tree.root_node();
        let sites = if file.language == Language::Java {
            java_weak_cryptography(root)
        } else if file.language == Language::Kotlin {
            kotlin_weak_cryptography(root)
        } else if file.language == Language::CSharp {
            csharp_weak_cryptography(root)
        } else if matches!(file.language, Language::C | Language::Cpp) {
            c_weak_cryptography(root)
        } else if matches!(file.language, Language::JavaScript | Language::TypeScript) {
            js_weak_cryptography(root)
        } else if file.language == Language::Python {
            py_weak_cryptography(root)
        } else if file.language == Language::Rust {
            rust_weak_cryptography(root)
        } else {
            ast_scan::nodes_of_kind(root, &["call_expression"])
                .into_iter()
                .filter_map(|node| Some((node, weak_go_algorithm(ast_scan::callee(node))?)))
                .collect()
        };
        for (node, algorithm) in sites {
            let line = node.start_position().0 + 1;
            findings.push(
                auditor_core::finding!(
                    "insecure_cryptography",
                    "correctness",
                    "Insecure cryptographic algorithm (AST-detected)",
                    format!("Weak crypto algorithm `{algorithm}` at line {line}"),
                    Severity::High,
                    Confidence::High,
                    &relative,
                    Some(line),
                )
                .with_recommendation("Use modern algorithms like SHA-256, AES-256-GCM, or bcrypt.")
                .with_evidence("algorithm", algorithm)
                .with_evidence("analysis", "ast"),
            );
        }
    });
    findings
}

fn weak_go_algorithm(callee: &str) -> Option<&'static str> {
    let package = callee.split('.').next()?;
    match package.to_ascii_lowercase().as_str() {
        "md5" => Some("MD5"),
        "sha1" => Some("SHA-1"),
        "des" => Some("DES"),
        "rc4" => Some("RC4"),
        "blowfish" => Some("Blowfish"),
        _ => None,
    }
}

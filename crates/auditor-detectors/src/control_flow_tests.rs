use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use auditor_ast::RepoAst;

use super::*;
use crate::{AnalysisMethod, AuditedFile, SourceRole};

fn temp_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "engineering-auditor-flow-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn audited_file(root: &Path, relative: &str, source: &str, language: Language) -> AuditedFile {
    let absolute = root.join(relative);
    fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    fs::write(&absolute, source).unwrap();
    AuditedFile {
        absolute,
        relative: relative.into(),
        language,
        bytes: source.len() as u64,
        role: SourceRole::Production,
    }
}

fn run(
    label: &str,
    relative: &str,
    source: &str,
    language: Language,
    detector: &str,
) -> Vec<Finding> {
    let root = temp_root(label);
    let file = audited_file(&root, relative, source, language);
    let cache = util::FileCache::new(std::slice::from_ref(&file));
    let mut repo_ast = RepoAst::new();
    let findings = match detector {
        "unreachable_code" => unreachable_code_findings(AstScan::new(
            std::slice::from_ref(&file),
            AnalysisMethod::Ast,
            &cache,
            &mut repo_ast,
        )),
        "constant_condition" => constant_condition_findings(AstScan::new(
            std::slice::from_ref(&file),
            AnalysisMethod::Ast,
            &cache,
            &mut repo_ast,
        )),
        other => panic!("unknown detector {other}"),
    };
    fs::remove_dir_all(root).unwrap();
    findings
}

#[test]
fn unreachable_code_reports_the_first_statement_after_return() {
    let findings = run(
        "rust-unreachable",
        "src/lib.rs",
        "fn execute() {\n    return;\n    work();\n    cleanup();\n}\n",
        Language::Rust,
        "unreachable_code",
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].locations[0].line, Some(3));
}

#[test]
fn unreachable_code_preserves_javascript_hoisted_declarations() {
    let findings = run(
        "js-hoist",
        "src/main.js",
        "function execute() { return; var deferred; function helper() {} }\n",
        Language::JavaScript,
        "unreachable_code",
    );
    assert!(findings.is_empty());
}

#[test]
fn unreachable_code_does_not_treat_a_trailing_comment_as_a_statement() {
    let findings = run(
        "rust-terminator-comment",
        "src/lib.rs",
        "fn execute() {\n    loop {\n        continue; // explain the control transfer\n    }\n}\n",
        Language::Rust,
        "unreachable_code",
    );
    assert!(findings.is_empty());
}

#[test]
fn unreachable_code_treats_switch_labels_as_entry_points() {
    let cpp = run(
        "cpp-switch-labels",
        "src/impl.cpp",
        "node* get(int type) {\n  switch (type) {\n    case 1:\n      break;\n    case 2:\n    case 3:\n      return nullptr;\n    default:\n      work();\n      break;\n      cleanup();\n  }\n  return nullptr;\n}\n",
        Language::Cpp,
        "unreachable_code",
    );
    assert_eq!(
        cpp.iter().map(|f| f.locations[0].line).collect::<Vec<_>>(),
        [Some(11)]
    );
    let c = run(
        "c-goto-label",
        "src/a.c",
        "int f(int x) {\n  if (x) goto done;\n  return 1;\ndone:\n  return 0;\n}\n",
        Language::C,
        "unreachable_code",
    );
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn constant_condition_allows_the_do_while_true_idiom() {
    let findings = run(
        "java-do-while-true",
        "src/Walk.java",
        "class W { int f(int x) { do { x = next(x); if (x > 9) break; } while (true); return x; } }\n",
        Language::Java,
        "constant_condition",
    );
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn unreachable_code_skips_csharp_local_functions_after_a_return() {
    let findings = run(
        "csharp-local-function",
        "src/Map.cs",
        "class M {\n    int F() {\n        return G(1);\n        [Pure]\n        static int G(int x) => x;\n    }\n}\n",
        Language::CSharp,
        "unreachable_code",
    );
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn unreachable_code_skips_preprocessor_directives_after_a_return() {
    let findings = run(
        "csharp-pragma",
        "src/Retry.cs",
        "class R {\n    int F() {\n#pragma warning disable 1998\n        return 1;\n#pragma warning restore 1998\n    }\n}\n",
        Language::CSharp,
        "unreachable_code",
    );
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn constant_condition_reports_literal_if_but_not_runtime_condition() {
    let positive = run(
        "python-constant",
        "src/main.py",
        "def execute(value):\n    if True:\n        consume(value)\n",
        Language::Python,
        "constant_condition",
    );
    assert_eq!(positive.len(), 1);

    let negative = run(
        "python-runtime",
        "src/main.py",
        "def execute(value):\n    if value:\n        consume(value)\n",
        Language::Python,
        "constant_condition",
    );
    assert!(negative.is_empty());
}

#[test]
fn constant_condition_allows_intentional_while_true_loop() {
    let findings = run(
        "rust-sentinel",
        "src/lib.rs",
        "fn execute() { while true { work(); } }\n",
        Language::Rust,
        "constant_condition",
    );
    assert!(findings.is_empty());
}

#[test]
fn flow_detectors_fail_closed_for_malformed_syntax_and_token_mode() {
    let root = temp_root("malformed");
    let file = audited_file(
        &root,
        "src/lib.rs",
        "fn broken( { return; work();",
        Language::Rust,
    );
    let cache = util::FileCache::new(std::slice::from_ref(&file));
    let mut repo_ast = RepoAst::new();
    assert!(unreachable_code_findings(AstScan::new(
        std::slice::from_ref(&file),
        AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ),)
    .is_empty());
    assert!(constant_condition_findings(AstScan::new(
        std::slice::from_ref(&file),
        AnalysisMethod::Token,
        &cache,
        &mut repo_ast,
    ),)
    .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreachable_code_needs_an_else_to_close_a_python_elif_chain() {
    let open_chain = "def f(v):\n    if v == 1:\n        return 'a'\n    elif v == 2:\n        return 'b'\n    return 'c'\n";
    assert!(run(
        "py-elif-open",
        "pkg/m.py",
        open_chain,
        Language::Python,
        "unreachable_code"
    )
    .is_empty());
    let closed_chain = "def f(v):\n    if v == 1:\n        return 'a'\n    elif v == 2:\n        return 'b'\n    else:\n        return 'c'\n    return 'd'\n";
    let findings = run(
        "py-elif-closed",
        "pkg/m.py",
        closed_chain,
        Language::Python,
        "unreachable_code",
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].locations[0].line, Some(8));
}

#[test]
fn unreachable_code_keeps_generator_marker_yields_and_cfg_gated_returns() {
    let generator = "def stream():\n    raise Closed()\n    yield b''\n";
    assert!(run(
        "py-yield",
        "pkg/m.py",
        generator,
        Language::Python,
        "unreachable_code"
    )
    .is_empty());
    let cfg = "fn f(x: u8) -> u8 {\n    #[cfg(not(debug_assertions))]\n    return x;\n    #[cfg(debug_assertions)]\n    return x + 1;\n}\n";
    assert!(run(
        "rust-cfg",
        "src/lib.rs",
        cfg,
        Language::Rust,
        "unreachable_code"
    )
    .is_empty());
}

#[test]
fn constant_condition_allows_the_for_true_infinite_loop_idiom() {
    let source = "class A {\n  int f(int[] b) {\n    int i = 0;\n    for (; true; i++) {\n      if (b[i] == 0) break;\n    }\n    return i;\n  }\n}\n";
    assert!(run(
        "java-for-true",
        "src/A.java",
        source,
        Language::Java,
        "constant_condition"
    )
    .is_empty());
}

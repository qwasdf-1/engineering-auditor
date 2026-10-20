use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::CallGraph;
use crate::ast_scan::AstScan;
use crate::structure_index::StructureIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-call-graph-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn file(root: &Path, relative: &str, language: Language, source: &str) -> crate::AuditedFile {
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    crate::AuditedFile {
        absolute,
        relative: relative.into(),
        language,
        bytes: source.len() as u64,
        role: SourceRole::Production,
    }
}

/// `caller_path:callee -> target_path:target_name` for every resolved call.
fn resolved(files: &[crate::AuditedFile]) -> Vec<String> {
    let cache = util::FileCache::new(files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let index = StructureIndex::build(AstScan::context(
        files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let graph = CallGraph::build(&index);
    let mut edges = Vec::new();
    for (file_index, file) in index.files().iter().enumerate() {
        for (call_index, call) in file.calls.iter().enumerate() {
            if let Some((target_file, target_symbol)) = graph.target(file_index, call_index) {
                let target = &index.files()[target_file];
                edges.push(format!(
                    "{}:{} -> {}:{}",
                    file.path, call.callee, target.path, target.symbols[target_symbol].name
                ));
            }
        }
    }
    edges.sort();
    edges
}

#[test]
fn rust_calls_resolve_to_local_and_imported_functions_but_not_methods_on_values() {
    let root = root("rust");
    let files = vec![
        file(
            &root,
            "src/lib.rs",
            Language::Rust,
            "mod remote;\nuse remote::details;\n\nfn get(id: u64) -> u64 { id }\n\nfn local(id: u64) -> u64 { get(id) }\n\nfn run(client: Client, ids: Vec<u64>) {\n    for id in ids {\n        consume(details(&client, id));\n        consume(local(id));\n        consume(client.get(id));\n    }\n}\n",
        ),
        file(
            &root,
            "src/remote.rs",
            Language::Rust,
            "pub fn details(client: &Client, id: u64) -> Response { client.get(id) }\n",
        ),
    ];
    assert_eq!(
        resolved(&files),
        [
            "src/lib.rs:details -> src/remote.rs:details",
            "src/lib.rs:get -> src/lib.rs:get",
            "src/lib.rs:local -> src/lib.rs:local",
        ]
    );
}

#[test]
fn ambiguous_unimported_names_stay_unresolved() {
    let root = root("ambiguous");
    let files = vec![
        file(
            &root,
            "app/a.py",
            Language::Python,
            "def helper():\n    return 1\n",
        ),
        file(
            &root,
            "app/b.py",
            Language::Python,
            "def helper():\n    return 2\n",
        ),
        file(
            &root,
            "app/c.py",
            Language::Python,
            "def run():\n    return helper()\n",
        ),
        file(
            &root,
            "app/d.py",
            Language::Python,
            "from app.a import helper\n\n\ndef run():\n    return helper()\n",
        ),
    ];
    assert_eq!(resolved(&files), ["app/d.py:helper -> app/a.py:helper"]);
}

#[test]
fn java_static_and_implicit_this_calls_resolve_but_instance_calls_do_not() {
    let root = root("java");
    let files = vec![
        file(
            &root,
            "src/main/java/app/Service.java",
            Language::Java,
            "package app;\n\nclass Service {\n    int local(int id) { return id; }\n\n    void run(Remote remote, int id) {\n        consume(Remote.details(id));\n        consume(local(id));\n        consume(remote.details(id));\n    }\n}\n",
        ),
        file(
            &root,
            "src/main/java/app/Remote.java",
            Language::Java,
            "package app;\n\nclass Remote {\n    static int details(int id) { return id; }\n}\n",
        ),
    ];
    let edges = resolved(&files);
    assert!(edges.contains(
        &"src/main/java/app/Service.java:details -> src/main/java/app/Remote.java:details"
            .to_string()
    ));
    assert!(edges.contains(
        &"src/main/java/app/Service.java:local -> src/main/java/app/Service.java:local".to_string()
    ));
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.contains(":details -> "))
            .count(),
        1,
        "`remote.details` on an instance must stay unresolved: {edges:?}"
    );
}

#[test]
fn go_calls_resolve_within_the_package_directory_only() {
    let root = root("go");
    let files = vec![
        file(
            &root,
            "service/service.go",
            Language::Go,
            "package service\n\nfunc run(id int) int {\n\treturn details(id)\n}\n",
        ),
        file(
            &root,
            "service/remote.go",
            Language::Go,
            "package service\n\nfunc details(id int) int {\n\treturn id\n}\n",
        ),
        file(
            &root,
            "other/other.go",
            Language::Go,
            "package other\n\nfunc run(id int) int {\n\treturn details(id)\n}\n",
        ),
    ];
    assert_eq!(
        resolved(&files),
        ["service/service.go:details -> service/remote.go:details"]
    );
}

use super::*;
use crate::ast_scan::AstScan;
use crate::call_graph::CallGraph;
use crate::{util, AnalysisMethod, AuditedFile, Language, SourceRole};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn go_preallocation_uses_range_and_append_facts() {
    let source = "package parity\nfunc copyValues(items []int) []int {\n result := make([]int, 0)\n for _, item := range items { result = append(result, item) }\n return result\n}\n";
    let root = temp_root();
    std::fs::create_dir_all(&root).unwrap();
    let absolute = root.join("source.go");
    std::fs::write(&absolute, source).unwrap();
    let files = vec![AuditedFile {
        absolute,
        relative: PathBuf::from("source.go"),
        language: Language::Go,
        bytes: source.len() as u64,
        role: SourceRole::Production,
    }];
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = StructureIndex::build(AstScan::context(
        &files,
        AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let algorithmic = AlgorithmicIndex::build(&structure, &CallGraph::build(&structure));
    let optimization = OptimizationIndex::build(&structure, &algorithmic);
    assert!(
        optimization.facts.iter().any(|fact| matches!(
            fact.kind,
            OptimizationFactKind::KnownSizePreallocation { .. }
        )),
        "loops={:?} calls={:?} assignments={:?}",
        structure.files()[0].loops,
        structure.files()[0].calls,
        structure.files()[0].assignments,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn typed_string_accumulation_frontends_build_facts() {
    let cases = [
        (
            "source.ts",
            Language::TypeScript,
            "function render(items: string[]) { let output: string = ''; for (const item of items) output += item; return output }\n",
        ),
        (
            "Source.java",
            Language::Java,
            "import java.util.List; class Source { String render(List<String> items) { String output = \"\"; for (String item : items) output += item; return output; } }\n",
        ),
        (
            "source.kt",
            Language::Kotlin,
            "fun render(items: List<String>): String { var output = \"\"; for (item in items) output += item; return output }\n",
        ),
    ];

    for (path, language, source) in cases {
        let (structure, optimization, root) = build_optimization(path, language, source);
        assert!(
            optimization
                .facts
                .iter()
                .any(|fact| matches!(fact.kind, OptimizationFactKind::StringAccumulation { .. })),
            "path={path} structure={:#?}",
            structure.files(),
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn java_imported_pattern_compilation_builds_regex_fact() {
    let source = "import java.util.regex.Pattern; class Source { void inspect(String[] items) { for (String item : items) { Pattern pattern = Pattern.compile(\"^fixed$\"); consume(pattern, item); } } }\n";
    let (structure, optimization, root) = build_optimization("Source.java", Language::Java, source);
    assert!(
        optimization
            .facts
            .iter()
            .any(|fact| matches!(fact.kind, OptimizationFactKind::LoopInvariantRegex { .. })),
        "structure={:#?}",
        structure.files(),
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn build_optimization(
    path: &str,
    language: Language,
    source: &str,
) -> (StructureIndex, OptimizationIndex, PathBuf) {
    let root = temp_root();
    std::fs::create_dir_all(&root).unwrap();
    let absolute = root.join(path);
    std::fs::write(&absolute, source).unwrap();
    let files = vec![AuditedFile {
        absolute,
        relative: PathBuf::from(path),
        language,
        bytes: source.len() as u64,
        role: SourceRole::Production,
    }];
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = StructureIndex::build(AstScan::context(
        &files,
        AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let algorithmic = AlgorithmicIndex::build(&structure, &CallGraph::build(&structure));
    let optimization = OptimizationIndex::build(&structure, &algorithmic);
    (structure, optimization, root)
}

fn temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "engineering-auditor-optimization-{}-{nanos}",
        std::process::id()
    ))
}

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn class_bases_resolve_to_the_simple_name_of_a_qualified_type() {
    assert_eq!(
        class_base(
            &Language::CSharp,
            "internal sealed class DataTableHandler : SqlMapper.ITypeHandler {"
        ),
        Some("ITypeHandler".into())
    );
    assert_eq!(
        class_base(&Language::Python, "class Model(models.Base):"),
        Some("Base".into())
    );
    assert_eq!(
        class_base(
            &Language::Java,
            "class Reader extends java.io.FilterReader {"
        ),
        Some("FilterReader".into())
    );
    assert_eq!(
        class_base(&Language::Kotlin, "class A(val x: Int) : B(), C {"),
        Some("B".into())
    );
}

#[test]
fn class_bases_follow_attributes_and_multi_line_headers() {
    assert_eq!(
        class_base(
            &Language::CSharp,
            "[JsonConverter(typeof(X))]\npublic class DataTableConverter : JsonConverter\n{"
        ),
        Some("JsonConverter".into())
    );
    assert_eq!(
        class_base(
            &Language::Kotlin,
            "/** Scope. */\n@OptIn(KoinInternalApi::class)\nclass Scope(\n  val id: String,\n) : Lockable() {"
        ),
        Some("Lockable".into())
    );
    assert_eq!(
        class_base(
            &Language::TypeScript,
            "export class ZodObject<\n  T extends ZodRawShape,\n> extends ZodType<T> {"
        ),
        Some("ZodType".into())
    );
    assert_eq!(
        class_base(&Language::Python, "@dataclass\nclass A(Base):\n    x: int"),
        Some("Base".into())
    );
}

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "engineering-auditor-structure-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn file(
    root: &Path,
    relative: &str,
    language: Language,
    role: SourceRole,
    source: &str,
) -> crate::AuditedFile {
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    crate::AuditedFile {
        absolute,
        relative: relative.into(),
        language,
        bytes: source.len() as u64,
        role,
    }
}

fn index(files: &[crate::AuditedFile]) -> StructureIndex {
    let cache = util::FileCache::new(files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    StructureIndex::build(AstScan::context(
        files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ))
}

fn resolution_for(index: &StructureIndex, path: &str, name: &str) -> ReferenceResolution {
    let (file_index, file) = index
        .files()
        .iter()
        .enumerate()
        .find(|(_, file)| file.path == path)
        .unwrap();
    let symbol_index = file
        .symbols
        .iter()
        .position(|symbol| symbol.name == name)
        .unwrap();
    index.reference_resolution(file_index, symbol_index)
}

#[test]
fn import_parsing_preserves_alias_and_canonical_target() {
    let parsed = parse_import(
        &Language::Rust,
        "pub use crate::model::Config as RuntimeConfig;",
    );
    assert_eq!(parsed.canonical.as_deref(), Some("crate::model::Config"));
    assert_eq!(parsed.imported.as_deref(), Some("Config"));
    assert_eq!(parsed.alias.as_deref(), Some("RuntimeConfig"));
    assert_eq!(parsed.module.as_deref(), Some("crate::model"));
}

#[test]
fn lexical_path_normalization_resolves_parent_segments() {
    assert_eq!(
        normalize_path(Path::new("src/feature/../model.rs")),
        "src/model.rs"
    );
}

#[test]
fn import_resolution_disambiguates_same_name_symbols_and_aliases() {
    let root = temp_root("same-name");
    let files = vec![
        file(
            &root,
            "src/alpha.rs",
            Language::Rust,
            SourceRole::Production,
            "const SETTING: usize = 1;\n",
        ),
        file(
            &root,
            "src/beta.rs",
            Language::Rust,
            SourceRole::Production,
            "const SETTING: usize = 2;\n",
        ),
        file(
            &root,
            "src/lib.rs",
            Language::Rust,
            SourceRole::Production,
            "mod alpha;\nmod beta;\npub use crate::alpha::SETTING as ACTIVE_SETTING;\nfn value() -> usize { ACTIVE_SETTING }\n",
        ),
    ];
    let index = index(&files);
    assert_eq!(
        resolution_for(&index, "src/alpha.rs", "SETTING"),
        ReferenceResolution::Referenced
    );
    assert_eq!(
        resolution_for(&index, "src/beta.rs", "SETTING"),
        ReferenceResolution::Unreferenced
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn test_files_resolve_production_symbols_without_becoming_emission_files() {
    let root = temp_root("test-context");
    let files = vec![
        file(
            &root,
            "src/lib.rs",
            Language::Rust,
            SourceRole::Production,
            "const INTERNAL_VALUE: usize = 1;\n",
        ),
        file(
            &root,
            "tests/usage.rs",
            Language::Rust,
            SourceRole::Test,
            "#[test]\nfn uses_value() { let _ = INTERNAL_VALUE; }\n",
        ),
    ];
    let index = index(&files);
    assert_eq!(
        resolution_for(&index, "src/lib.rs", "INTERNAL_VALUE"),
        ReferenceResolution::Referenced
    );
    assert!(
        !index
            .files()
            .iter()
            .find(|file| file.path == "tests/usage.rs")
            .unwrap()
            .emittable
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dynamic_imports_remain_explicit_unresolved_coverage() {
    let root = temp_root("dynamic-import");
    let files = vec![file(
        &root,
        "src/load.js",
        Language::JavaScript,
        SourceRole::Production,
        "async function load(name) { return import(name); }\n",
    )];
    let index = index(&files);
    assert_eq!(index.dynamic_import_count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn function_metrics_use_nearest_semantic_function_owner() {
    let root = temp_root("function-metrics");
    let files = vec![file(
        &root,
        "src/lib.rs",
        Language::Rust,
        SourceRole::Production,
        "fn outer() { let outer_value = 1; fn inner() { let inner_value = 2; return; } consume(outer_value); }\n",
    )];
    let index = index(&files);
    let file = &index.files()[0];
    let metric = |name: &str| {
        let owner = file
            .symbols
            .iter()
            .position(|symbol| symbol.name == name)
            .unwrap() as u32;
        file.function_metrics[&owner]
    };

    assert_eq!(metric("outer").locals, 1);
    assert_eq!(metric("outer").returns, 0);
    assert_eq!(metric("inner").locals, 1);
    assert_eq!(metric("inner").returns, 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn control_depth_compensates_for_else_if_chains() {
    let root = temp_root("else-if-depth");
    let files = vec![file(
        &root,
        "src/lib.rs",
        Language::Rust,
        SourceRole::Production,
        "fn classify(value: usize) { if value == 1 {} else if value == 2 {} else if value == 3 {} }\n",
    )];
    let index = index(&files);
    assert_eq!(
        index.files()[0]
            .controls
            .iter()
            .map(|control| control.depth)
            .max(),
        Some(1)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_nested_function_counts_its_own_nesting() {
    let root = temp_root("nested-def-depth");
    let files = vec![file(
        &root,
        "src/walk.py",
        Language::Python,
        SourceRole::Production,
        "def outer(rows):\n    for row in rows:\n        if row:\n            def inner(n):\n                if n:\n                    return 1\n                return 0\n            inner(row)\n",
    )];
    let index = index(&files);
    let file = &index.files()[0];
    let inner = file
        .symbols
        .iter()
        .position(|symbol| symbol.name == "inner")
        .unwrap() as u32;
    let deepest = file
        .controls
        .iter()
        .filter(|control| control.owner == Some(inner))
        .map(|control| control.depth)
        .max();
    assert_eq!(deepest, Some(1));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn loops_over_batches_pages_and_windows_walk_batches() {
    let walks = |binding: &str, iterable: &str| {
        StructureLoop {
            owner: None,
            binding: vec![binding.to_string()],
            iterable: Some(iterable.to_string()),
            iterable_span: None,
            span: SourceSpan {
                start_byte: 0,
                end_byte: 0,
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            control_region: 0,
        }
        .walks_batches()
    };
    assert!(walks("purlBatch", "purls"));
    assert!(walks("page", "range(10)"));
    assert!(walks("group", "ids.chunks(100)"));
    assert!(walks("part", "Lists.partition(ids, 100)"));
    assert!(walks("part", "itertools.batched (ids, 50)"));
    assert!(!walks("item", "items"));
    assert!(!walks("pages_seen", "counts"));
    assert!(!walks("part", "self.batches"));
    assert!(!walks("part", "rechunks(ids)"));
}

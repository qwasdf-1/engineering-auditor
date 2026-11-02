use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{chain_segments, Resolution, Segment, TypeFacts};
use crate::ast_scan::AstScan;
use crate::structure_index::StructureIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-type-facts-{name}-{}-{nanos}",
        std::process::id()
    ))
}

/// What each expression is at its `at` marker text, across `sources`.
fn resolve(
    name: &str,
    sources: &[(&str, Language, &str)],
    uses: &[(&str, &str, &str)],
) -> Vec<Resolution> {
    let root = root(name);
    let files = sources
        .iter()
        .map(|(relative, language, source)| file(&root, relative, language.clone(), source))
        .collect::<Vec<_>>();
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let index = StructureIndex::build(AstScan::context(
        &files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let facts = TypeFacts::new(&index);
    let resolved = uses
        .iter()
        .map(|(path, at, expression)| {
            let file = facts
                .file_index(path)
                .unwrap_or_else(|| panic!("{path} not indexed"));
            let source = sources
                .iter()
                .find(|(relative, ..)| relative == path)
                .unwrap()
                .2;
            let owner = facts.owner_at(file, source.find(at).unwrap());
            facts.resolve(file, owner, expression)
        })
        .collect();
    let _ = std::fs::remove_dir_all(root);
    resolved
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

fn typed(name: &str) -> Resolution {
    Resolution {
        type_name: Some(name.to_string()),
        ..Resolution::default()
    }
}

#[test]
fn chains_split_into_named_and_called_links() {
    let segment = |name, call| Segment { name, call };
    assert_eq!(
        chain_segments("table.getObjectId().x"),
        [
            segment("table", false),
            segment("getObjectId", true),
            segment("x", false)
        ]
    );
    assert_eq!(
        chain_segments("a.b(c(d), \"e\")"),
        [segment("a", false), segment("b", true)]
    );
    assert!(chain_segments("a + b").is_empty());
    assert!(chain_segments("items[0]").is_empty());
    assert!(chain_segments("\"x\"").is_empty());
}

#[test]
fn java_values_resolve_through_locals_fields_bases_and_return_types() {
    let table = "package m;\npublic class Table extends Base {\n    private long id;\n    public long getObjectId() { return id; }\n    public Kind kind() { return Kind.VIEW; }\n}\n";
    let base = "package m;\npublic class Base {\n    protected String name;\n    public String getName() { return name; }\n}\n";
    let kind = "package m;\npublic enum Kind { TABLE, VIEW }\n";
    let constants = "package m;\npublic final class Constants {\n    public static final String ALIAS = \"Umb.Alias\";\n    public static final java.util.List<String> NAMES = java.util.List.of(\"a\", \"b\");\n}\n";
    let user = "package m;\nclass User {\n    private Table table;\n    void load(long sessionId, String raw) {\n        int count = 2;\n        run(sessionId);\n    }\n}\n";
    let sources = [
        ("src/Table.java", Language::Java, table),
        ("src/Base.java", Language::Java, base),
        ("src/Kind.java", Language::Java, kind),
        ("src/Constants.java", Language::Java, constants),
        ("src/User.java", Language::Java, user),
    ];
    let at = |expression| ("src/User.java", "run(sessionId)", expression);
    let resolved = resolve(
        "java",
        &sources,
        &[
            at("sessionId"),
            at("count"),
            at("raw"),
            at("table.getObjectId()"),
            at("this.table.getObjectId()"),
            at("table.getName()"),
            at("table.kind().name()"),
            at("Kind.VIEW"),
            at("raw.length()"),
            at("Constants.ALIAS"),
            at("unknown.value"),
        ],
    );
    assert_eq!(resolved[0], typed("long"));
    assert_eq!(resolved[1], typed("int"));
    assert_eq!(resolved[2], typed("String"));
    assert_eq!(resolved[3], typed("long"));
    assert_eq!(resolved[4], typed("long"));
    assert_eq!(resolved[5], typed("String"));
    assert!(resolved[6].enum_value);
    assert!(resolved[7].enum_value);
    assert_eq!(resolved[8], typed("int"));
    assert_eq!(resolved[9].constant.as_deref(), Some("\"Umb.Alias\""));
    assert_eq!(resolved[10], Resolution::default());
    assert!(resolved[0].is_numeric_or_enum() && !resolved[2].is_numeric_or_enum());
}

#[test]
fn csharp_constants_resolve_through_nested_static_classes() {
    let constants = "namespace U;\npublic static class Constants {\n    public static class PropertyEditors {\n        public static class Aliases {\n            public const string CheckBoxList = \"Umbraco.CheckBoxList\";\n        }\n    }\n}\n";
    let user = "namespace U;\npublic class Migration {\n    private static readonly int Timeout = 30;\n    public void Run(int id, Guid key) {\n        Execute(id);\n    }\n}\n";
    let sources = [
        ("src/Constants.cs", Language::CSharp, constants),
        ("src/Migration.cs", Language::CSharp, user),
    ];
    let at = |expression| ("src/Migration.cs", "Execute(id)", expression);
    let resolved = resolve(
        "csharp",
        &sources,
        &[
            at("Constants.PropertyEditors.Aliases.CheckBoxList"),
            at("id"),
            at("key"),
            at("Timeout"),
        ],
    );
    assert_eq!(
        resolved[0].constant.as_deref(),
        Some("\"Umbraco.CheckBoxList\"")
    );
    assert_eq!(resolved[1], typed("int"));
    assert!(resolved[2].is_numeric_or_enum());
    assert_eq!(resolved[3].constant.as_deref(), Some("30"));
}

#[test]
fn ambiguous_type_names_resolve_only_where_every_candidate_agrees() {
    let first = "package a;\npublic class Node { public long id() { return 1; } public String label() { return \"\"; } }\n";
    let second = "package b;\npublic class Node { public long id() { return 2; } public int label() { return 0; } }\n";
    let user = "package c;\nclass User {\n    void f(Node node) {\n        use(node);\n    }\n}\n";
    let sources = [
        ("src/a/Node.java", Language::Java, first),
        ("src/b/Node.java", Language::Java, second),
        ("src/c/User.java", Language::Java, user),
    ];
    let at = |expression| ("src/c/User.java", "use(node)", expression);
    let resolved = resolve(
        "ambiguous",
        &sources,
        &[at("node.id()"), at("node.label()")],
    );
    assert_eq!(resolved[0], typed("long"));
    assert_eq!(resolved[1], Resolution::default());
}

#[test]
fn go_kotlin_and_typescript_state_parameter_types_constants_and_results() {
    let go = "package p\n\nconst Table = \"users\"\n\nfunc count() int64 { return 1 }\n\nfunc load(id int64, name string) {\n\trun(id)\n}\n";
    let kotlin = "class Repo {\n    companion object {\n        const val TABLE = \"users\"\n    }\n\n    fun size(): Long = 1L\n\n    fun load(id: Int, name: String) {\n        run(id)\n    }\n}\n";
    let typescript = "const TABLE = 'users';\nclass Repo {\n    size(): number { return 1; }\n    load(id: number, name: string) {\n        run(id);\n    }\n}\n";
    let sources = [
        ("p/load.go", Language::Go, go),
        ("src/Repo.kt", Language::Kotlin, kotlin),
        ("src/repo.ts", Language::TypeScript, typescript),
    ];
    let resolved = resolve(
        "typed",
        &sources,
        &[
            ("p/load.go", "run(id)", "id"),
            ("p/load.go", "run(id)", "Table"),
            ("p/load.go", "run(id)", "count()"),
            ("src/Repo.kt", "run(id)", "id"),
            ("src/Repo.kt", "run(id)", "TABLE"),
            ("src/Repo.kt", "run(id)", "size()"),
            ("src/repo.ts", "run(id)", "id"),
            ("src/repo.ts", "run(id)", "TABLE"),
            ("src/repo.ts", "run(id)", "name"),
        ],
    );
    assert_eq!(resolved[0], typed("int64"));
    assert_eq!(resolved[1].constant.as_deref(), Some("\"users\""));
    assert_eq!(resolved[2], typed("int64"));
    assert_eq!(resolved[3], typed("Int"));
    assert_eq!(resolved[4].constant.as_deref(), Some("\"users\""));
    assert_eq!(resolved[5], typed("Long"));
    assert_eq!(resolved[6], typed("number"));
    assert_eq!(resolved[7].constant.as_deref(), Some("'users'"));
    assert_eq!(resolved[8], typed("string"));
}

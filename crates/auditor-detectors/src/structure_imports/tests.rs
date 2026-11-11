use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast_scan::AstScan;
use crate::structure_index::StructureIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-import-resolution-{name}-{}-{nanos}",
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

/// Each import of `importer` as `stand-in -> used paths`.
fn used(files: &[crate::AuditedFile], importer: &str) -> Vec<String> {
    let cache = util::FileCache::new(files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let index = StructureIndex::build(AstScan::context(
        files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let file = index
        .files()
        .iter()
        .find(|file| file.path == importer)
        .unwrap();
    file.imports
        .iter()
        .map(|import| {
            format!(
                "{} -> [{}]",
                import.resolved_path.as_deref().unwrap_or("-"),
                import.used_paths.join(", ")
            )
        })
        .collect()
}

#[test]
fn a_go_package_import_reaches_the_files_declaring_the_names_it_uses() {
    let root = root("go");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("go.mod"), "module example.com/app\n").unwrap();
    let files = vec![
        file(
            &root,
            "cmd/main.go",
            Language::Go,
            "package main\n\nimport (\n\t\"example.com/app/store\"\n\ts \"example.com/app/store\"\n\t_ \"example.com/app/store\"\n)\n\nvar item store.Item\n\nfunc run() int {\n\treturn store.Load(store.Max) + s.Load(1)\n}\n",
        ),
        file(
            &root,
            "store/a.go",
            Language::Go,
            "package store\n\nconst (\n\tMax = 3\n\tmin = 1\n)\n\nfunc helper() int {\n\treturn min\n}\n",
        ),
        file(
            &root,
            "store/b.go",
            Language::Go,
            "package store\n\nfunc Load(id int) int {\n\treturn id\n}\n",
        ),
        file(&root, "store/c.go", Language::Go, "package store\n\ntype Item struct{}\n"),
        file(&root, "store/d.go", Language::Go, "package store\n\nfunc Unused() {}\n"),
        file(
            &root,
            "store/b_test.go",
            Language::Go,
            "package store\n\nfunc Load2() {}\n",
        ),
    ];
    assert_eq!(
        used(&files, "cmd/main.go"),
        [
            "store/a.go -> [store/a.go, store/b.go, store/c.go]",
            "store/a.go -> [store/b.go]",
            "store/a.go -> []",
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_java_wildcard_import_reaches_the_classes_it_names() {
    let root = root("java");
    let files = vec![
        file(
            &root,
            "src/main/java/com/example/app/Main.java",
            Language::Java,
            "package com.example.app;\n\nimport com.example.model.*;\n\nclass Main {\n    Foo foo = new Foo();\n}\n",
        ),
        file(
            &root,
            "src/main/java/com/example/model/Bar.java",
            Language::Java,
            "package com.example.model;\n\npublic class Bar {}\n",
        ),
        file(
            &root,
            "src/main/java/com/example/model/Foo.java",
            Language::Java,
            "package com.example.model;\n\npublic class Foo {}\n",
        ),
    ];
    assert_eq!(
        used(&files, "src/main/java/com/example/app/Main.java"),
        ["src/main/java/com/example/model/Bar.java -> [src/main/java/com/example/model/Foo.java]"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_python_relative_import_climbs_one_package_per_extra_dot() {
    let root = root("python");
    let files = vec![
        file(
            &root,
            "pkg/utils/disk.py",
            Language::Python,
            "from ..logger import logger\nfrom . import parse\nfrom .parse import parse_bool\nfrom ...outside import x\nimport pkg.utils\n",
        ),
        file(&root, "pkg/__init__.py", Language::Python, ""),
        file(&root, "pkg/logger.py", Language::Python, "logger = None\n"),
        file(&root, "pkg/utils/__init__.py", Language::Python, ""),
        file(&root, "pkg/utils/parse.py", Language::Python, "def parse_bool(v):\n    return v\n"),
    ];
    assert_eq!(
        used(&files, "pkg/utils/disk.py"),
        [
            "pkg/logger.py -> []",
            "pkg/utils/__init__.py -> []",
            "pkg/utils/parse.py -> []",
            "- -> []",
            "pkg/utils/__init__.py -> []",
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

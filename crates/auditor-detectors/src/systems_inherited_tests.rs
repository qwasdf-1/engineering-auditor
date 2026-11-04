use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast_scan::AstScan;
use crate::call_graph::CallGraph;
use crate::structure_index::StructureIndex;
use crate::systems::{blocking_io_in_async_context_findings, loop_artifact_rewrite_findings};
use crate::systems_facts::SystemsIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-systems-inherited-{name}-{}-{nanos}",
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

/// One detector's messages for one file.
fn messages(
    name: &str,
    (relative, language, source): (&str, Language, &str),
    detector: fn(&SystemsIndex) -> Vec<crate::Finding>,
) -> Vec<String> {
    let root = root(name);
    let files = vec![file(&root, relative, language, source)];
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = StructureIndex::build(AstScan::context(
        &files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let messages = detector(&SystemsIndex::build(
        &structure,
        &CallGraph::build(&structure),
    ))
    .into_iter()
    .map(|finding| finding.message)
    .collect();
    let _ = std::fs::remove_dir_all(root);
    messages
}

/// The `loop_artifact_rewrite` messages for one Python file.
fn rewrites(name: &str, source: &str) -> Vec<String> {
    messages(
        name,
        ("app/service.py", Language::Python, source),
        loop_artifact_rewrite_findings,
    )
}

#[test]
fn a_helper_writing_a_per_item_path_is_not_a_rewrite() {
    let source = "\
from pathlib import Path


def save(target, raw):
    Path(target).write_text(raw)


def rewrite(paths):
    for path in paths:
        raw = Path(path).read_text()
        save(path + '.out', raw)
";
    assert!(rewrites("per-item", source).is_empty());
}

#[test]
fn a_helper_rewriting_one_artifact_per_call_is_a_rewrite() {
    let source = "\
from pathlib import Path


def bump(entry):
    data = Path('index.json').read_text()
    Path('index.json').write_text(data + entry)


def record(entries):
    for entry in entries:
        bump(entry)
";
    assert_eq!(
        rewrites("own-pair", source),
        ["Function `record` performs whole-artifact read and write effects inside one loop, through `bump`."]
    );
}

#[test]
fn a_written_value_binds_through_two_helpers() {
    let source = "\
from pathlib import Path


def save(raw):
    Path('report.json').write_text(raw)


def publish(text):
    save(text)


def rewrite(paths):
    for path in paths:
        raw = Path(path).read_text()
        publish(raw)
";
    assert_eq!(
        rewrites("two-deep", source),
        ["Function `rewrite` performs whole-artifact read and write effects inside one loop, through `publish`."]
    );
}

#[test]
fn an_unrelated_written_value_is_not_a_rewrite() {
    let source = "\
from pathlib import Path


def save(raw):
    Path('report.json').write_text(raw)


def rewrite(paths, summary):
    for path in paths:
        raw = Path(path).read_text()
        save(summary)
";
    assert!(rewrites("unrelated", source).is_empty());
}

#[test]
fn an_async_function_literal_called_through_a_method_is_not_blocking() {
    let source = "import { promises as fs } from 'node:fs';\n\
class Renderer {\n\
  async render() {\n\
    return await this.template('a');\n\
  }\n\
  async template(name) {\n\
    return (async () => {\n\
      return await fs.readFile(name, 'utf8');\n\
    })();\n\
  }\n\
  async load(name) {\n\
    return this.read(name);\n\
  }\n\
  read(name) {\n\
    return require('fs').readFileSync(name, 'utf8');\n\
  }\n\
}\n";
    assert_eq!(
        messages(
            "iife",
            ("src/renderer.js", Language::JavaScript, source),
            blocking_io_in_async_context_findings,
        ),
        vec!["Function `load` makes a blocking call from an async function, through `read`."]
    );
}

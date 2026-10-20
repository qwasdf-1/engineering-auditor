use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::algorithmic_facts::AlgorithmicIndex;
use crate::ast_scan::AstScan;
use crate::call_graph::CallGraph;
use crate::optimization::loop_invariant_regex_compilation_findings;
use crate::optimization_facts::OptimizationIndex;
use crate::structure_index::StructureIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-optimization-inherited-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn file(root: &Path, relative: &str, source: &str) -> crate::AuditedFile {
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    crate::AuditedFile {
        absolute,
        relative: relative.into(),
        language: Language::Python,
        bytes: source.len() as u64,
        role: SourceRole::Production,
    }
}

/// The `loop_invariant_regex_compilation` messages for one Python file.
fn messages(name: &str, source: &str) -> Vec<String> {
    let root = root(name);
    let files = vec![file(&root, "app/service.py", source)];
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = StructureIndex::build(AstScan::context(
        &files,
        crate::AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let graph = CallGraph::build(&structure);
    let algorithmic = AlgorithmicIndex::build(&structure, &graph);
    let messages = loop_invariant_regex_compilation_findings(&OptimizationIndex::build(
        &structure,
        &algorithmic,
        &graph,
    ))
    .into_iter()
    .map(|finding| finding.message)
    .collect();
    let _ = std::fs::remove_dir_all(root);
    messages
}

#[test]
fn a_memoized_or_cached_helper_compiles_once() {
    let source = "\
import re
from functools import lru_cache


@lru_cache(maxsize=None)
def matcher():
    return re.compile('^fixed$')


def plain():
    return re.compile('^fixed$')


def inspect(items):
    pattern = None
    for item in items:
        consume(matcher(), item)
        if pattern is None:
            pattern = plain()
        consume(pattern, item)
";
    assert_eq!(messages("memoized", source), Vec::<String>::new());
}

#[test]
fn a_helper_pattern_binds_to_the_loop_call_arguments() {
    let source = "\
import re


def matcher(word):
    return re.compile('^' + word + '$')


def configured():
    word = load_word()
    return re.compile(word)


def inspect(items, word):
    for item in items:
        consume(matcher(item), item)
        consume(configured(), item)
        consume(matcher(word), item)
";
    assert_eq!(
        messages("bound-pattern", source),
        ["Function `inspect` calls `compile` with a loop-invariant pattern, through `matcher`."]
    );
}

#[test]
fn a_helper_pattern_read_from_state_is_not_fixed() {
    let source = "\
import re


def matcher(query):
    return re.compile(query.remainder())


class Parser:
    def pattern(self):
        return re.compile(self.prefix + '$')

    def parse(self, query, items):
        for item in items:
            consume(matcher(query), item)
            consume(self.pattern(), item)
";
    assert_eq!(messages("stateful-pattern", source), Vec::<String>::new());
}

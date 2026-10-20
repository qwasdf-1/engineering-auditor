use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::algorithmic::{n_plus_one_query_in_loop_findings, repeated_sort_in_loop_findings};
use crate::algorithmic_facts::AlgorithmicIndex;
use crate::ast_scan::AstScan;
use crate::call_graph::CallGraph;
use crate::structure_index::StructureIndex;
use crate::{util, Language, SourceRole};

fn root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ea-algorithmic-inherited-{name}-{}-{nanos}",
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

/// The repeated-sort and N+1 query messages for one Python file.
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
    let index = AlgorithmicIndex::build(&structure, &CallGraph::build(&structure));
    let messages = repeated_sort_in_loop_findings(&index)
        .into_iter()
        .chain(n_plus_one_query_in_loop_findings(&index))
        .map(|finding| finding.message)
        .collect();
    let _ = std::fs::remove_dir_all(root);
    messages
}

#[test]
fn a_helper_sorting_each_loop_item_is_not_a_repeated_sort() {
    let source = "\
def refresh(candidates: list[int]):
    candidates.sort()


def inspect(batches: list[list[int]], extra: list[int]):
    for batch in batches:
        refresh(batch)
    for batch in batches:
        extra.append(batch[0])
        refresh(extra)
";
    assert_eq!(messages("per-item-sort", source), Vec::<String>::new());
}

#[test]
fn a_sort_binds_through_two_helpers() {
    let source = "\
def order(values: list[int]):
    values.sort()


def refresh(candidates: list[int]):
    order(candidates)


def inspect(requests: list[Request], candidates: list[int]):
    for request in requests:
        refresh(candidates)
        consume(request, candidates[0])
";
    assert_eq!(
        messages("two-helper-sort", source),
        ["Function `inspect` repeatedly sorts unchanged `candidates` inside a loop, through `refresh`."]
    );
}

#[test]
fn a_helper_query_needs_a_loop_derived_argument() {
    let source = "\
def load_one(repo: Repository, item_id: int):
    return repo.find_by_id(item_id)


def fetch(repo: Repository, item_id: int):
    return load_one(repo, item_id)


def load(ids: list[int], repo: Repository, fixed: int):
    for item_id in ids:
        consume(fetch(repo, item_id))
        consume(load_one(repo, fixed))
        consume(load_one(repo, item_id=fixed))
";
    assert_eq!(
        messages("helper-query", source),
        ["Function `load` calls `repo.find_by_id` with loop-derived `item_id`, through `fetch`."]
    );
}

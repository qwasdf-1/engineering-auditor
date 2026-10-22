use std::sync::Arc;

use auditor_ast::{AstLang, AstTree, RepoAst};

use crate::{util, AnalysisMethod, AuditedFile, Language, SourceRole};

#[path = "ast_nodes.rs"]
mod nodes;
pub(crate) use nodes::*;

pub(crate) struct AstScan<'files, 'ast> {
    files: &'files [AuditedFile],
    method: AnalysisMethod,
    cache: &'files util::FileCache,
    repo_ast: &'ast mut RepoAst,
    scope: AstScanScope,
}

#[derive(Clone, Copy)]
enum AstScanScope {
    Emission,
    Context,
    /// Production, tooling, and test files: test-quality emission.
    TestScope,
}

impl<'files, 'ast> AstScan<'files, 'ast> {
    pub(crate) fn new(
        files: &'files [AuditedFile],
        method: AnalysisMethod,
        cache: &'files util::FileCache,
        repo_ast: &'ast mut RepoAst,
    ) -> Self {
        Self {
            files,
            method,
            cache,
            repo_ast,
            scope: AstScanScope::Emission,
        }
    }

    pub(crate) fn context(
        files: &'files [AuditedFile],
        method: AnalysisMethod,
        cache: &'files util::FileCache,
        repo_ast: &'ast mut RepoAst,
    ) -> Self {
        let mut scan = Self::new(files, method, cache, repo_ast);
        scan.scope = AstScanScope::Context;
        scan
    }

    /// A scan of the same files sharing this scan's parse cache, for an
    /// owner that walks its files more than once.
    pub(crate) fn reborrow(&mut self) -> AstScan<'files, '_> {
        AstScan {
            files: self.files,
            method: self.method,
            cache: self.cache,
            repo_ast: &mut *self.repo_ast,
            scope: self.scope,
        }
    }

    pub(crate) fn test_scope(
        files: &'files [AuditedFile],
        method: AnalysisMethod,
        cache: &'files util::FileCache,
        repo_ast: &'ast mut RepoAst,
    ) -> Self {
        let mut scan = Self::new(files, method, cache, repo_ast);
        scan.scope = AstScanScope::TestScope;
        scan
    }
}

pub(crate) fn visit_ast_files(
    scan: AstScan<'_, '_>,
    supports: impl Fn(&Language) -> bool,
    visit: impl FnMut(&AuditedFile, &AstTree, AstLang),
) {
    visit_trees(scan, supports, |_| false, visit);
}

/// [`visit_ast_files`], except that files of the languages `as_parsed`
/// selects are visited as tree-sitter parsed them, syntax errors and all.
/// That suits a rule that reads only declarations whose shape survives a
/// local error: a Catch2 `TEST_CASE("..") { .. }` reads as a statement
/// missing its `;` before a block, which recovery quarantines.
pub(crate) fn visit_trees(
    scan: AstScan<'_, '_>,
    supports: impl Fn(&Language) -> bool,
    as_parsed: impl Fn(&Language) -> bool,
    mut visit: impl FnMut(&AuditedFile, &AstTree, AstLang),
) {
    if scan.method == AnalysisMethod::Token {
        return;
    }
    for file in scan.files {
        let Some((language, source)) = eligible_source(&scan, file, &supports) else {
            continue;
        };
        let Ok(tree) = scan
            .repo_ast
            .get_or_parse_shared(&file.absolute, &language, source)
        else {
            continue;
        };
        // A tree with syntax errors is visited as recovered, without the
        // declarations holding them, or not at all.
        let tree = if as_parsed(&file.language) {
            Some(tree)
        } else {
            tree.error_free()
        };
        let Some(tree) = tree else {
            continue;
        };
        visit(file, tree, language);
    }
}

/// Findings on the nodes of the kinds `kinds` names for each parsed file's
/// language, where `finding` judges a node (with the file's language and
/// relative path) to be one.
pub(crate) fn kind_findings(
    scan: AstScan<'_, '_>,
    supports: impl Fn(&Language) -> bool,
    kinds: impl Fn(&Language) -> &'static [&'static str],
    finding: impl Fn(auditor_ast::AstNode<'_>, &Language, &str) -> Option<auditor_core::Finding>,
) -> Vec<auditor_core::Finding> {
    let mut findings = Vec::new();
    visit_ast_files(scan, supports, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        findings.extend(
            nodes_of_kind(tree.root_node(), kinds(&file.language))
                .into_iter()
                .filter_map(|node| finding(node, &file.language, &relative)),
        );
    });
    findings
}

/// Findings at the sites `sites` picks in each parsed file, one per
/// `(node, api)` pair, built by `finding` with the file's relative path.
pub(crate) fn site_findings(
    scan: AstScan<'_, '_>,
    supports: impl Fn(&Language) -> bool,
    sites: impl for<'a> Fn(
        &Language,
        auditor_ast::AstNode<'a>,
    ) -> Vec<(auditor_ast::AstNode<'a>, String)>,
    finding: impl Fn(auditor_ast::AstNode<'_>, &str, &str) -> auditor_core::Finding,
) -> Vec<auditor_core::Finding> {
    let mut findings = Vec::new();
    visit_ast_files(scan, supports, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        findings.extend(
            sites(&file.language, tree.root_node())
                .into_iter()
                .map(|(node, api)| finding(node, &api, &relative)),
        );
    });
    findings
}

/// `visit_ast_files` as a parallel map: parses uncached files and runs `map`
/// on all cores, returning results in file order.
pub(crate) fn map_ast_files<'files, R: Send>(
    scan: AstScan<'files, '_>,
    supports: impl Fn(&Language) -> bool,
    map: impl Fn(&'files AuditedFile, &AstTree, AstLang) -> R + Sync,
) -> Vec<R> {
    if scan.method == AnalysisMethod::Token {
        return Vec::new();
    }
    let files: &'files [AuditedFile] = scan.files;
    let eligible: Vec<_> = files
        .iter()
        .filter_map(|file| {
            eligible_source(&scan, file, &supports)
                .map(|(language, source)| (file, language, source))
        })
        .collect();
    scan.repo_ast.parse_missing_shared(
        eligible
            .iter()
            .map(|(file, language, source)| (file.absolute.clone(), *language, Arc::clone(source)))
            .collect(),
    );
    let repo_ast = &*scan.repo_ast;
    let parsed: Vec<_> = eligible
        .into_iter()
        .filter_map(|(file, language, _)| {
            let tree = repo_ast.get(&file.absolute)?.error_free()?;
            Some((file, tree, language))
        })
        .collect();
    auditor_core::parallel_map(&parsed, |(file, tree, language)| map(file, tree, *language))
}

fn eligible_source(
    scan: &AstScan<'_, '_>,
    file: &AuditedFile,
    supports: &impl Fn(&Language) -> bool,
) -> Option<(AstLang, Arc<str>)> {
    let role_allowed = match scan.scope {
        AstScanScope::Emission => {
            matches!(file.role, SourceRole::Production | SourceRole::Tooling)
                && !util::is_test_source_path(&file.relative)
        }
        AstScanScope::TestScope => matches!(
            file.role,
            SourceRole::Production | SourceRole::Tooling | SourceRole::Test
        ),
        AstScanScope::Context => matches!(
            file.role,
            SourceRole::Production
                | SourceRole::Tooling
                | SourceRole::Test
                | SourceRole::Fixture
                | SourceRole::Benchmark
                | SourceRole::Example
                | SourceRole::Reference
        ),
    };
    if !role_allowed || !supports(&file.language) {
        return None;
    }
    let language = util::map_language_to_ast(&file.language)?;
    Some((language, scan.cache.shared_text(file)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_mode_does_not_parse_or_visit() {
        let mut repo_ast = RepoAst::new();
        let cache = util::FileCache::new(&[]);
        let mut visited = false;
        visit_ast_files(
            AstScan::new(&[], AnalysisMethod::Token, &cache, &mut repo_ast),
            |_| true,
            |_, _, _| visited = true,
        );
        assert!(!visited);
    }

    #[test]
    fn context_mode_visits_test_roles() {
        let root = std::env::temp_dir().join(format!(
            "engineering-auditor-ast-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let absolute = root.join("tests/usage.rs");
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(&absolute, "fn usage() {}\n").unwrap();
        let file = AuditedFile {
            relative: "tests/usage.rs".into(),
            absolute,
            language: Language::Rust,
            bytes: 12,
            role: SourceRole::Test,
        };
        let cache = util::FileCache::new(std::slice::from_ref(&file));
        let mut repo_ast = RepoAst::new();
        let mut visited = false;
        visit_ast_files(
            AstScan::context(
                std::slice::from_ref(&file),
                AnalysisMethod::Ast,
                &cache,
                &mut repo_ast,
            ),
            |_| true,
            |_, _, _| visited = true,
        );
        assert!(visited);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rust_test_ancestry_follows_attribute_siblings() {
        let source =
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn sample() { assert!(true); }\n}\n";
        let mut parser = auditor_ast::AstParser::new().unwrap();
        let tree = parser.parse(&AstLang::Rust, source).unwrap();
        let macros = nodes_of_kind(tree.root_node(), &["macro_invocation"]);
        assert_eq!(macros.len(), 1);
        assert!(is_inside_rust_test(macros[0]));

        let functions = nodes_of_kind(tree.root_node(), &["function_item"]);
        assert_eq!(functions.len(), 1);
        assert!(is_inside_rust_test(functions[0]));
    }
}

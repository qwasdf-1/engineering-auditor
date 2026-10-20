use std::sync::Arc;

use auditor_ast::{AstLang, AstNode, AstNodeKind, AstTree, RepoAst};

use crate::{util, AnalysisMethod, AuditedFile, Language, SourceRole};

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
        if tree.root_node().has_error() {
            continue;
        }
        visit(file, tree, language);
    }
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
            let tree = repo_ast.get(&file.absolute)?;
            (!tree.root_node().has_error()).then_some((file, tree, language))
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

pub(crate) fn nodes_of_kind<'a>(root: AstNode<'a>, kinds: &[&str]) -> Vec<AstNode<'a>> {
    kinds
        .iter()
        .flat_map(|kind| root.descendants_of_kind(&AstNodeKind::from(*kind)))
        .collect()
}

/// Comments of `kinds` that open with a TODO marker.
pub(crate) fn todo_comments<'a>(root: AstNode<'a>, kinds: &[&str]) -> Vec<AstNode<'a>> {
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter(|comment| {
            comment
                .text()
                .trim_start_matches(['/', '*', '#', ' '])
                .to_ascii_lowercase()
                .starts_with("todo")
        })
        .collect()
}

/// A stub's message saying the code is unfinished.
pub(crate) fn is_unimplemented_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "todo",
        "not implemented",
        "not yet implemented",
        "unimplemented",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

/// TODO comments of `kinds` and the unfinished-code `stubs`, in source order.
pub(crate) fn todo_nodes<'a>(
    root: AstNode<'a>,
    kinds: &[&str],
    stubs: impl IntoIterator<Item = AstNode<'a>>,
) -> Vec<AstNode<'a>> {
    let mut nodes = todo_comments(root, kinds);
    nodes.extend(stubs);
    nodes.sort_by_key(|node| node.start_byte());
    nodes
}

/// The content of a `string_literal` without interpolation or escapes (C,
/// C++, Kotlin); `None` for any other node.
pub(crate) fn string_literal_content(node: AstNode<'_>) -> Option<String> {
    if node.kind() != "string_literal" {
        return None;
    }
    let mut parts = node.named_children();
    match (parts.next(), parts.next()) {
        (None, _) => Some(String::new()),
        (Some(content), None) if content.kind() == "string_content" => {
            Some(content.text().to_string())
        }
        _ => None,
    }
}

/// The only named child of `kind`, if there is exactly one.
pub(crate) fn sole_child_of_kind<'a>(node: AstNode<'a>, kind: &str) -> Option<AstNode<'a>> {
    let mut matches = node.named_children().filter(|child| child.kind() == kind);
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// The `name` field of every node of `kinds` in `root`.
pub(crate) fn declared_names<'a>(
    root: AstNode<'a>,
    kinds: &'a [&'a str],
) -> impl Iterator<Item = String> + 'a {
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter_map(|declaration| declaration.child_by_field_name("name"))
        .map(|name| name.text().to_string())
}

/// Whether `node` is, or contains, an identifier of `kind` named in `names`.
pub(crate) fn mentions_any(node: AstNode<'_>, kind: &str, names: &[&str]) -> bool {
    std::iter::once(node)
        .chain(nodes_of_kind(node, &[kind]))
        .any(|identifier| names.contains(&identifier.text()))
}

/// A block whose only statement is `return true;`.
pub(crate) fn returns_only_true(body: AstNode<'_>) -> bool {
    let mut statements = body
        .named_children()
        .filter(|child| !child.kind().contains("comment"));
    match (statements.next(), statements.next()) {
        (Some(statement), None) => {
            statement.kind() == "return_statement"
                && statement
                    .named_child(0)
                    .is_some_and(|value| value.text() == "true")
        }
        _ => false,
    }
}

pub(crate) fn callee(node: AstNode<'_>) -> &str {
    node.child_by_field_name("function")
        .or_else(|| node.child_by_field_name("callee"))
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0))
        .map(|child| child.text())
        .unwrap_or_default()
}

pub(crate) fn is_inside_rust_test(mut node: AstNode<'_>) -> bool {
    loop {
        if matches!(node.kind(), "function_item" | "mod_item") {
            let header = node
                .text()
                .split_once('{')
                .map_or(node.text(), |part| part.0);
            if header.contains("#[test]")
                || header.contains("cfg(test)")
                || preceding_test_attribute(node)
            {
                return true;
            }
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn preceding_test_attribute(node: AstNode<'_>) -> bool {
    preceding_attributes(node).any(|attribute| {
        attribute.text().contains("#[test]") || attribute.text().contains("cfg(test)")
    })
}

/// The run of Rust `attribute_item` siblings directly before `node`.
pub(crate) fn preceding_attributes(node: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    let mut run = Vec::new();
    if let Some(parent) = node.parent() {
        for sibling in parent.named_children() {
            if sibling.start_byte() == node.start_byte() && sibling.end_byte() == node.end_byte() {
                break;
            }
            if sibling.kind() == "attribute_item" {
                run.push(sibling);
            } else {
                run.clear();
            }
        }
    }
    run.into_iter()
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

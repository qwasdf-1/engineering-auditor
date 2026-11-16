//! Resolved API identity for Rust: the path a call denotes through the
//! file's `use` declarations. With `use std::process::Command as Cmd;`, the
//! call `Cmd::new(..)` is `std::process::Command::new`; with
//! `use md5::{Digest, Md5};`, `Md5::new()` is `md5::Md5::new`.
//!
//! Bindings come from plain, aliased (`as`), and grouped (`{..}`, `self`)
//! `use` trees, read by [`use_leaves`]; globs bind nothing a path can name.
//! Method calls on values have no path and are matched by method name in
//! the API tables.

use std::collections::BTreeMap;

use auditor_ast::AstNode;

use crate::ast_scan;

/// `use` bindings of one Rust file: local name -> full path.
pub(crate) struct RustScope {
    bindings: BTreeMap<String, String>,
}

impl RustScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut bindings = BTreeMap::new();
        for declaration in ast_scan::nodes_of_kind(root, &["use_declaration"]) {
            for leaf in use_leaves(declaration.text()) {
                let (path, alias) = leaf
                    .rsplit_once(" as ")
                    .map_or((leaf.as_str(), None), |(path, alias)| (path, Some(alias)));
                if path.ends_with('*') {
                    continue;
                }
                let local = alias.unwrap_or_else(|| path.rsplit("::").next().unwrap_or_default());
                bindings.insert(local.to_string(), path.to_string());
            }
        }
        Self { bindings }
    }

    /// The resolved path of an identifier or scoped path: its head through a
    /// `use` binding, or the path as written.
    pub(crate) fn resolve(&self, node: AstNode<'_>) -> Option<String> {
        match node.kind() {
            "identifier" => Some(
                self.bindings
                    .get(node.text())
                    .cloned()
                    .unwrap_or_else(|| node.text().to_string()),
            ),
            "scoped_identifier" => {
                let name = node.child_by_field_name("name")?;
                match node.child_by_field_name("path") {
                    Some(path) => Some(format!("{}::{}", self.resolve(path)?, name.text())),
                    None => Some(name.text().to_string()),
                }
            }
            "generic_function" => self.resolve(node.child_by_field_name("function")?),
            _ => None,
        }
    }

    /// Every path call in `root` with its resolved callee.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        crate::api_identity::resolved_calls(root, &["call_expression"], |call| {
            self.resolve(call.child_by_field_name("function")?)
        })
    }
}

/// The paths one `use` declaration imports, each `path` or `path as
/// alias`: braces expand, `a::{self}` is `a`, and a glob keeps its `*`.
/// Comments inside the tree are dropped.
pub(crate) fn use_leaves(declaration: &str) -> Vec<String> {
    let text = without_comments(declaration);
    let mut leaves = Vec::new();
    expand("", use_tree(&text), &mut leaves);
    leaves
}

/// `text` without `//` and `/* */` comments, its whitespace collapsed; a
/// `use` tree has no other `/`.
fn without_comments(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match (character, characters.peek()) {
            ('/', Some('/')) => {
                characters.by_ref().find(|character| *character == '\n');
                kept.push(' ');
            }
            ('/', Some('*')) => {
                characters.next();
                let mut last = ' ';
                characters.by_ref().find(|character| {
                    let closes = last == '*' && *character == '/';
                    last = *character;
                    closes
                });
                kept.push(' ');
            }
            _ => kept.push(character),
        }
    }
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The tree after `use`, without its visibility or `;`.
fn use_tree(declaration: &str) -> &str {
    let text = declaration.trim().trim_end_matches(';').trim();
    let text = match text.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')').map_or(rest, |(_, rest)| rest),
        Some(rest) if rest.starts_with(' ') => rest,
        _ => text,
    }
    .trim_start();
    text.strip_prefix("use ").unwrap_or(text).trim()
}

fn expand(prefix: &str, tree: &str, leaves: &mut Vec<String>) {
    let tree = tree.trim();
    if tree.is_empty() {
        return;
    }
    let join = |path: &str| match (prefix.is_empty(), path.is_empty()) {
        (true, _) => path.to_string(),
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}::{path}"),
    };
    let Some(open) = tree.find('{') else {
        let (path, alias) = tree
            .rsplit_once(" as ")
            .map_or((tree, None), |(path, alias)| {
                (path.trim(), Some(alias.trim()))
            });
        let path = if path == "self" {
            prefix.to_string()
        } else {
            join(path.strip_suffix("::self").unwrap_or(path))
        };
        if !path.is_empty() {
            leaves.push(alias.map_or_else(|| path.clone(), |alias| format!("{path} as {alias}")));
        }
        return;
    };
    let prefix = join(tree[..open].trim().trim_end_matches("::"));
    let body = tree[open + 1..].trim_end();
    let body = body.strip_suffix('}').unwrap_or(body);
    let mut depth = 0_i32;
    let mut start = 0;
    for (index, character) in body.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                expand(&prefix, &body[start..index], leaves);
                start = index + 1;
            }
            _ => {}
        }
    }
    expand(&prefix, &body[start..], leaves);
}

/// The argument expressions of a call.
pub(crate) fn rust_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| {
            list.named_children()
                .filter(|child| !child.kind().ends_with("comment"))
                .collect()
        })
        .unwrap_or_default()
}

/// The value of a plain or raw Rust string literal.
pub(crate) fn rust_string(node: AstNode<'_>) -> Option<String> {
    matches!(node.kind(), "string_literal" | "raw_string_literal").then(|| {
        node.named_children()
            .filter(|part| part.kind() == "string_content")
            .map(|part| part.text())
            .collect()
    })
}

/// The method name of a method call (`builder.arg(..)` -> `arg`).
pub(crate) fn method_name(call: AstNode<'_>) -> Option<&str> {
    let function = call.child_by_field_name("function")?;
    (function.kind() == "field_expression")
        .then(|| function.child_by_field_name("field"))
        .flatten()
        .map(|field| field.text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"
use std::process::{self, Command as Cmd};
use md5::{Digest, Md5};
use std::net::*;

fn run() {
    Cmd::new("sh");
    Md5::new();
    process::exit(1);
    std::fs::Permissions::from_mode(0o777);
    TcpListener::bind("0.0.0.0:80");
    builder.arg("-c");
}
"#;

    #[test]
    fn a_use_tree_imports_each_leaf() {
        assert_eq!(
            use_leaves(
                "pub(crate) use crate::{\n    a::{B as C, self}, // the module\n    d::*, /* all */ e,\n};"
            ),
            ["crate::a::B as C", "crate::a", "crate::d::*", "crate::e"]
        );
        assert_eq!(use_leaves("use super::*;"), ["super::*"]);
    }

    #[test]
    fn rust_paths_resolve_through_use_trees() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Rust, SOURCE)
            .unwrap();
        let scope = RustScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(api, _)| api)
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "std::process::Command::new",
                "md5::Md5::new",
                "std::process::exit",
                "std::fs::Permissions::from_mode",
                "TcpListener::bind",
            ]
        );
    }
}

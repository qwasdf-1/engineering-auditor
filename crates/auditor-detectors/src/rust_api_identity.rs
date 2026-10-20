//! Resolved API identity for Rust: the path a call denotes through the
//! file's `use` declarations. With `use std::process::Command as Cmd;`, the
//! call `Cmd::new(..)` is `std::process::Command::new`; with
//! `use md5::{Digest, Md5};`, `Md5::new()` is `md5::Md5::new`.
//!
//! Bindings come from plain, aliased (`as`), and grouped (`{..}`, `self`)
//! `use` trees; globs bind nothing a path can name. Method calls on values
//! have no path and are matched by method name in the API tables.

use std::collections::BTreeMap;

use auditor_ast::AstNode;

use crate::ast_scan;

/// `use` bindings of one Rust file: local name -> full path.
pub(crate) struct RustScope {
    bindings: BTreeMap<String, String>,
}

impl RustScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            bindings: BTreeMap::new(),
        };
        for declaration in ast_scan::nodes_of_kind(root, &["use_declaration"]) {
            if let Some(tree) = declaration.child_by_field_name("argument") {
                scope.bind(tree, "");
            }
        }
        scope
    }

    fn bind(&mut self, tree: AstNode<'_>, prefix: &str) {
        let join = |path: &str| {
            if prefix.is_empty() {
                path.to_string()
            } else {
                format!("{prefix}::{path}")
            }
        };
        match tree.kind() {
            "identifier" | "scoped_identifier" => {
                let full = join(tree.text());
                let local = full.rsplit("::").next().unwrap_or_default().to_string();
                self.bindings.insert(local, full);
            }
            "use_as_clause" => {
                if let (Some(path), Some(alias)) =
                    (tree.named_child(0), tree.child_by_field_name("alias"))
                {
                    self.bindings
                        .insert(alias.text().to_string(), join(path.text()));
                }
            }
            "scoped_use_list" => {
                let nested = tree
                    .named_children()
                    .find(|child| child.kind() != "use_list")
                    .map_or_else(|| prefix.to_string(), |path| join(path.text()));
                for item in tree
                    .named_children()
                    .filter(|child| child.kind() == "use_list")
                    .flat_map(|list| list.named_children())
                {
                    if item.kind() == "self" {
                        let local = nested.rsplit("::").next().unwrap_or_default().to_string();
                        self.bindings.insert(local, nested.clone());
                    } else {
                        self.bind(item, &nested);
                    }
                }
            }
            _ => {}
        }
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

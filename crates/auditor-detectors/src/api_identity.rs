//! Resolved API identity: the module-qualified name a call or type reference
//! denotes through the file's import bindings. In a Go file that imports
//! `x "os/exec"`, the callee `x.Command` is `os/exec.Command`; a bare
//! identifier (a builtin such as `panic`, or a local) resolves to itself.
//!
//! Detectors match these names against per-language API tables instead of
//! callee text, so an aliased import cannot hide a call and a local value
//! that happens to share a package's name cannot fake one.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;

/// Local import bindings of one file: binding name -> module path.
pub(crate) struct ImportScope {
    bindings: BTreeMap<String, String>,
}

impl ImportScope {
    /// The import bindings of a Go source file. A spec binds its explicit
    /// name, or the package name implied by its path; blank (`_`) and dot
    /// imports bind nothing that a qualified reference can name.
    pub(crate) fn go(root: AstNode<'_>) -> Self {
        let mut bindings = BTreeMap::new();
        for spec in ast_scan::nodes_of_kind(root, &["import_spec"]) {
            let Some(path) = spec.child_by_field_name("path").and_then(go_string) else {
                continue;
            };
            let name = match spec.child_by_field_name("name") {
                Some(name) if matches!(name.text(), "_" | ".") => continue,
                Some(name) => name.text().to_string(),
                None => go_package_name(&path).to_string(),
            };
            bindings.insert(name, path);
        }
        Self { bindings }
    }

    /// `module/path.Member` for a qualified reference through an import
    /// (`exec.Command`, `tls.Config`), or the identifier itself when it is
    /// unqualified. `None` for any other expression (a method on a value).
    pub(crate) fn resolve(&self, node: AstNode<'_>) -> Option<String> {
        let (qualifier, member) = match node.kind() {
            "identifier" => return Some(node.text().to_string()),
            "selector_expression" => (
                node.child_by_field_name("operand")?,
                node.child_by_field_name("field")?,
            ),
            "qualified_type" => (
                node.child_by_field_name("package")?,
                node.child_by_field_name("name")?,
            ),
            _ => return None,
        };
        let module = self.bindings.get(qualifier.text())?;
        Some(format!("{module}.{}", member.text()))
    }

    /// The resolved identity of callee text: `pkg.Member` through an import
    /// binding, or a bare name as itself.
    pub(crate) fn resolve_text(&self, callee: &str) -> Option<String> {
        match callee.split_once('.') {
            Some((qualifier, member)) if !member.contains('.') => {
                let module = self.bindings.get(qualifier)?;
                Some(format!("{module}.{member}"))
            }
            Some(_) => None,
            None => Some(callee.to_string()),
        }
    }

    /// Every call in `root` with its resolved callee.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter_map(|call| Some((self.resolve(call.child_by_field_name("function")?)?, call)))
            .collect()
    }

    /// `(field, value, element)` of every keyed element in a composite
    /// literal of the resolved type `type_name`
    /// (`crypto/tls.Config{MinVersion: ..}`).
    pub(crate) fn composite_fields<'a>(
        &self,
        root: AstNode<'a>,
        type_name: &str,
    ) -> Vec<(String, AstNode<'a>, AstNode<'a>)> {
        let mut fields = Vec::new();
        for literal in ast_scan::nodes_of_kind(root, &["composite_literal"]) {
            let resolved = literal
                .child_by_field_name("type")
                .and_then(|kind| self.resolve(kind));
            if resolved.as_deref() != Some(type_name) {
                continue;
            }
            let Some(body) = literal.child_by_field_name("body") else {
                continue;
            };
            for element in body
                .named_children()
                .filter(|child| child.kind() == "keyed_element")
            {
                let mut parts = element.named_children();
                let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let value = value.named_child(0).unwrap_or(value);
                fields.push((key.text().trim().to_string(), value, element));
            }
        }
        fields
    }
}

/// The package name a Go import path binds by default: its last element,
/// without a major-version suffix (`github.com/go-chi/chi/v5` -> `chi`,
/// `gopkg.in/yaml.v3` -> `yaml`).
pub(crate) fn go_package_name(path: &str) -> &str {
    let mut elements = path.rsplit('/');
    let last = elements.next().unwrap_or(path);
    let is_major = |element: &str| {
        element
            .strip_prefix('v')
            .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
    };
    let name = if is_major(last) {
        elements.next().unwrap_or(last)
    } else {
        last
    };
    match name.rsplit_once(".v") {
        Some((stem, version)) if version.chars().all(|c| c.is_ascii_digit()) => stem,
        _ => name,
    }
}

/// The value of a Go string literal (interpreted or raw).
pub(crate) fn go_string(node: AstNode<'_>) -> Option<String> {
    match node.kind() {
        "interpreted_string_literal" | "raw_string_literal" => {
            let text = node.text();
            Some(text.get(1..text.len().checked_sub(1)?)?.to_string())
        }
        _ => None,
    }
}

/// The value of a Go integer literal (`0644`, `0o644`, `0x1ff`, `420`).
pub(crate) fn go_integer(node: AstNode<'_>) -> Option<u32> {
    if node.kind() != "int_literal" {
        return None;
    }
    let text = node.text().replace('_', "").to_ascii_lowercase();
    if let Some(digits) = text.strip_prefix("0o") {
        u32::from_str_radix(digits, 8).ok()
    } else if let Some(digits) = text.strip_prefix("0x") {
        u32::from_str_radix(digits, 16).ok()
    } else if let Some(digits) = text.strip_prefix("0b") {
        u32::from_str_radix(digits, 2).ok()
    } else if text.len() > 1 && text.starts_with('0') {
        u32::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse().ok()
    }
}

/// The argument expressions of a call.
pub(crate) fn arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| list.named_children().collect())
        .unwrap_or_default()
}

/// The package a Go file declares (`package main`).
pub(crate) fn go_package(root: AstNode<'_>) -> Option<&str> {
    root.named_children()
        .find(|child| child.kind() == "package_clause")?
        .named_child(0)
        .map(|name| name.text())
}

/// Every node of `kinds` in `root` with its resolved callee, in source order.
pub(crate) fn resolved_calls<'a>(
    root: AstNode<'a>,
    kinds: &[&str],
    resolve: impl Fn(AstNode<'a>) -> Option<String>,
) -> Vec<(String, AstNode<'a>)> {
    let mut calls = ast_scan::nodes_of_kind(root, kinds)
        .into_iter()
        .filter_map(|call| Some((resolve(call)?, call)))
        .collect::<Vec<_>>();
    calls.sort_by_key(|(_, call)| call.start_byte());
    calls
}

/// The calls whose resolved callee is one of `apis`.
pub(crate) fn calls_named<'a>(
    calls: Vec<(String, AstNode<'a>)>,
    apis: &[&str],
) -> Vec<AstNode<'a>> {
    calls
        .into_iter()
        .filter(|(api, _)| apis.contains(&api.as_str()))
        .map(|(_, call)| call)
        .collect()
}

/// An identifier's identity: its import binding, else a global that no local
/// declaration shadows.
pub(crate) fn resolve_identifier(
    name: &str,
    bindings: &BTreeMap<String, String>,
    declared: &BTreeSet<String>,
    globals: &[&str],
) -> Option<String> {
    bindings
        .get(name)
        .cloned()
        .or_else(|| (globals.contains(&name) && !declared.contains(name)).then(|| name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"package main

import (
	"crypto/tls"
	_ "embed"
	x "os/exec"
	"github.com/go-chi/chi/v5"
	"gopkg.in/yaml.v3"
)

func run() {
	x.Command("sh", "-c", cmd)
	chi.NewRouter()
	yaml.Unmarshal(data, &out)
	panic("stop")
	value.Method()
	_ = &tls.Config{InsecureSkipVerify: true, MinVersion: tls.VersionTLS10}
}
"#;

    #[test]
    fn go_callees_resolve_through_aliases_and_implied_package_names() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Go, SOURCE)
            .unwrap();
        let scope = ImportScope::go(tree.root_node());
        let callees = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(
            callees,
            [
                "os/exec.Command",
                "github.com/go-chi/chi/v5.NewRouter",
                "gopkg.in/yaml.v3.Unmarshal",
                "panic",
            ]
        );
        let fields = scope
            .composite_fields(tree.root_node(), "crypto/tls.Config")
            .into_iter()
            .map(|(field, value, _)| {
                (
                    field,
                    scope.resolve(value).unwrap_or(value.text().to_string()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                ("InsecureSkipVerify".to_string(), "true".to_string()),
                (
                    "MinVersion".to_string(),
                    "crypto/tls.VersionTLS10".to_string()
                ),
            ]
        );
        assert_eq!(go_package(tree.root_node()), Some("main"));
    }

    #[test]
    fn go_literals_and_package_names() {
        assert_eq!(go_package_name("os/exec"), "exec");
        assert_eq!(go_package_name("math/rand/v2"), "rand");
        assert_eq!(go_package_name("gopkg.in/yaml.v3"), "yaml");
        let tree = AstParser::new()
            .unwrap()
            .parse(
                &AstLang::Go,
                "package p\nvar a, b, c = 0644, 0o777, `raw`\n",
            )
            .unwrap();
        let integers = ast_scan::nodes_of_kind(tree.root_node(), &["int_literal"])
            .into_iter()
            .filter_map(go_integer)
            .collect::<Vec<_>>();
        assert_eq!(integers, [0o644, 0o777]);
        let raw = ast_scan::nodes_of_kind(tree.root_node(), &["raw_string_literal"]);
        assert_eq!(go_string(raw[0]).as_deref(), Some("raw"));
    }
}

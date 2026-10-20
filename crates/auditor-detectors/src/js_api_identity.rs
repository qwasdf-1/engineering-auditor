//! Resolved API identity for JavaScript and TypeScript: the module-qualified
//! name a call denotes through the file's module bindings. With
//! `const { exec: run } = require('node:child_process')`, the call `run(..)`
//! is `child_process.exec`; with `import * as cp from 'child_process'`,
//! `cp.execSync(..)` is `child_process.execSync`.
//!
//! Bindings come from ES imports (default, namespace, and named, with
//! aliases) and CommonJS `require` (whole module, a member of it, or a
//! destructured member). A `node:` scheme is dropped. Runtime globals
//! (`process`, `Math`, `eval`, `Function`) resolve to themselves unless the
//! file declares a variable or parameter of that name.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;

/// Runtime globals the API tables name.
const GLOBALS: &[&str] = &[
    "Function",
    "Math",
    "eval",
    "process",
    "setInterval",
    "setTimeout",
];

/// Module bindings and locally declared names of one JS/TS file.
pub(crate) struct JsScope {
    bindings: BTreeMap<String, String>,
    declared: BTreeSet<String>,
}

impl JsScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            bindings: BTreeMap::new(),
            declared: BTreeSet::new(),
        };
        for import in ast_scan::nodes_of_kind(root, &["import_statement"]) {
            scope.bind_import(import);
        }
        for declarator in ast_scan::nodes_of_kind(root, &["variable_declarator"]) {
            if !scope.bind_require(declarator) {
                if let Some(name) = declarator.child_by_field_name("name") {
                    for identifier in declared_identifiers(name) {
                        scope.declared.insert(identifier);
                    }
                }
            }
        }
        for parameter in ast_scan::nodes_of_kind(
            root,
            &[
                "formal_parameters",
                "required_parameter",
                "optional_parameter",
            ],
        ) {
            for identifier in declared_identifiers(parameter) {
                scope.declared.insert(identifier);
            }
        }
        scope.declared.extend(ast_scan::declared_names(
            root,
            &["function_declaration", "class_declaration"],
        ));
        scope
    }

    fn bind_import(&mut self, import: AstNode<'_>) {
        let Some(module) = import
            .child_by_field_name("source")
            .and_then(js_string)
            .map(|module| module_name(&module))
        else {
            return;
        };
        let Some(clause) = import
            .named_children()
            .find(|child| child.kind() == "import_clause")
        else {
            return;
        };
        for part in clause.named_children() {
            match part.kind() {
                // A default import of a Node built-in is the module object.
                "identifier" => {
                    self.bindings
                        .insert(part.text().to_string(), module.clone());
                }
                "namespace_import" => {
                    if let Some(name) = part.named_child(0) {
                        self.bindings
                            .insert(name.text().to_string(), module.clone());
                    }
                }
                "named_imports" => {
                    for specifier in part
                        .named_children()
                        .filter(|child| child.kind() == "import_specifier")
                    {
                        let Some(name) = specifier.child_by_field_name("name") else {
                            continue;
                        };
                        let local = specifier.child_by_field_name("alias").unwrap_or(name);
                        let member = name.text().trim_matches(['"', '\'']);
                        let target = if member == "default" {
                            module.clone()
                        } else {
                            format!("{module}.{member}")
                        };
                        self.bindings.insert(local.text().to_string(), target);
                    }
                }
                _ => {}
            }
        }
    }

    /// `const m = require('m')`, `const x = require('m').x`, or
    /// `const { x, y: z } = require('m')`. Whether the declarator bound one.
    fn bind_require(&mut self, declarator: AstNode<'_>) -> bool {
        let (Some(name), Some(value)) = (
            declarator.child_by_field_name("name"),
            declarator.child_by_field_name("value"),
        ) else {
            return false;
        };
        let Some(target) = required_path(value) else {
            return false;
        };
        match name.kind() {
            "identifier" => {
                self.bindings.insert(name.text().to_string(), target);
            }
            "object_pattern" => {
                for property in name.named_children() {
                    let (key, local) = match property.kind() {
                        "shorthand_property_identifier_pattern" => (property, property),
                        "pair_pattern" => {
                            let (Some(key), Some(value)) = (
                                property.child_by_field_name("key"),
                                property.child_by_field_name("value"),
                            ) else {
                                continue;
                            };
                            (key, value)
                        }
                        _ => continue,
                    };
                    if local.kind() == "identifier"
                        || local.kind() == "shorthand_property_identifier_pattern"
                    {
                        self.bindings
                            .insert(local.text().to_string(), format!("{target}.{}", key.text()));
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// The resolved name of a callee or constructor expression: a binding, a
    /// member path through one, or an unshadowed runtime global.
    pub(crate) fn resolve(&self, node: AstNode<'_>) -> Option<String> {
        match node.kind() {
            "identifier" => crate::api_identity::resolve_identifier(
                node.text(),
                &self.bindings,
                &self.declared,
                GLOBALS,
            ),
            "member_expression" => {
                let property = node.child_by_field_name("property")?;
                if property.kind() != "property_identifier" {
                    return None;
                }
                let object = node.child_by_field_name("object")?;
                let base = match required_path(object) {
                    Some(path) => path,
                    None => self.resolve(object)?,
                };
                Some(format!("{base}.{}", property.text()))
            }
            "parenthesized_expression" => self.resolve(node.named_child(0)?),
            _ => None,
        }
    }

    /// The resolved identity of a call (`child_process.exec`) or construction
    /// (`https.Agent.<init>`).
    pub(crate) fn resolve_call(&self, call: AstNode<'_>) -> Option<String> {
        match call.kind() {
            "call_expression" => self.resolve(call.child_by_field_name("function")?),
            "new_expression" => Some(format!(
                "{}.<init>",
                self.resolve(call.child_by_field_name("constructor")?)?
            )),
            _ => None,
        }
    }

    /// Every resolved call and construction in `root`, in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        crate::api_identity::resolved_calls(root, &["call_expression", "new_expression"], |call| {
            self.resolve_call(call)
        })
    }
}

/// `require('m')` as `m`, and a member chain on it (`require('m').x`).
fn required_path(node: AstNode<'_>) -> Option<String> {
    match node.kind() {
        "call_expression" => {
            let callee = node.child_by_field_name("function")?;
            if callee.kind() != "identifier" || callee.text() != "require" {
                return None;
            }
            let argument = node.child_by_field_name("arguments")?.named_child(0)?;
            Some(module_name(&js_string(argument)?))
        }
        "member_expression" => {
            let property = node.child_by_field_name("property")?;
            let base = required_path(node.child_by_field_name("object")?)?;
            Some(format!("{base}.{}", property.text()))
        }
        _ => None,
    }
}

/// A module specifier without the `node:` scheme.
fn module_name(specifier: &str) -> String {
    specifier
        .strip_prefix("node:")
        .unwrap_or(specifier)
        .to_string()
}

fn declared_identifiers(node: AstNode<'_>) -> Vec<String> {
    if matches!(
        node.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    ) {
        return vec![node.text().to_string()];
    }
    ast_scan::nodes_of_kind(
        node,
        &["identifier", "shorthand_property_identifier_pattern"],
    )
    .into_iter()
    // A type annotation's identifiers are not declared values.
    .filter(|identifier| {
        !std::iter::successors(identifier.parent(), |parent| parent.parent())
            .take_while(|ancestor| ancestor.start_byte() >= node.start_byte())
            .any(|ancestor| ancestor.kind() == "type_annotation")
    })
    .map(|identifier| identifier.text().to_string())
    .collect()
}

/// The value of a JS/TS string literal, or of a template literal without
/// substitutions.
pub(crate) fn js_string(node: AstNode<'_>) -> Option<String> {
    match node.kind() {
        "string" => {
            let text = node.text();
            Some(text.get(1..text.len().checked_sub(1)?)?.to_string())
        }
        "template_string" if !node.text().contains("${") => {
            let text = node.text();
            Some(text.get(1..text.len().checked_sub(1)?)?.to_string())
        }
        _ => None,
    }
}

/// The argument expressions of a call or construction.
pub(crate) fn js_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| {
            list.named_children()
                .filter(|child| !child.kind().ends_with("comment"))
                .collect()
        })
        .unwrap_or_default()
}

/// `(key, value)` of each property in an object literal.
pub(crate) fn object_properties(object: AstNode<'_>) -> Vec<(String, AstNode<'_>)> {
    if object.kind() != "object" {
        return Vec::new();
    }
    object
        .named_children()
        .filter(|child| child.kind() == "pair")
        .filter_map(|pair| {
            let key = pair.child_by_field_name("key")?;
            let name = js_string(key).unwrap_or_else(|| key.text().to_string());
            Some((name, pair.child_by_field_name("value")?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"
import * as cp from 'node:child_process';
import fs from 'fs';
import { createHash as hash } from 'crypto';
const { exec: run, spawnSync } = require('child_process');
const https = require('https');
const readFile = require('fs').readFileSync;

function handler(process) {
  cp.execSync('ls');
  run(line);
  spawnSync('sh', ['-c', line]);
  fs.chmodSync(path, 0o777);
  hash('md5');
  new https.Agent({ rejectUnauthorized: false });
  readFile(path);
  require('vm').runInNewContext(code);
  process.exit(1);
  eval(code);
  local.thing();
}
"#;

    #[test]
    fn js_callees_resolve_through_imports_requires_and_unshadowed_globals() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::JavaScript, SOURCE)
            .unwrap();
        let scope = JsScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(api, _)| api)
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "child_process.execSync",
                "child_process.exec",
                "child_process.spawnSync",
                "fs.chmodSync",
                "crypto.createHash",
                "https.Agent.<init>",
                "fs.readFileSync",
                "vm.runInNewContext",
                "eval",
            ]
        );
    }
}

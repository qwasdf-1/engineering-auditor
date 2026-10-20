//! Resolved API identity for Python: the module-qualified name a call denotes
//! through the file's import bindings. With `import subprocess as sp`, the
//! call `sp.run(..)` is `subprocess.run`; with `from pdb import set_trace`,
//! `set_trace()` is `pdb.set_trace`.
//!
//! Bindings come from `import a.b` (binds `a`), `import a.b as c`, and
//! `from a import b as c`. Builtins (`print`, `exit`, `breakpoint`) resolve
//! to themselves unless the file binds or declares that name.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;

/// Builtins the API tables name.
const BUILTINS: &[&str] = &["SystemExit", "breakpoint", "exit", "print", "quit"];

/// Import bindings and locally declared names of one Python file.
pub(crate) struct PyScope {
    bindings: BTreeMap<String, String>,
    declared: BTreeSet<String>,
}

impl PyScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            bindings: BTreeMap::new(),
            declared: BTreeSet::new(),
        };
        for import in ast_scan::nodes_of_kind(root, &["import_statement"]) {
            for name in import.named_children() {
                scope.bind_module(name);
            }
        }
        for import in ast_scan::nodes_of_kind(root, &["import_from_statement"]) {
            scope.bind_from(import);
        }
        scope.declared.extend(ast_scan::declared_names(
            root,
            &["function_definition", "class_definition"],
        ));
        for target in ast_scan::nodes_of_kind(root, &["parameters", "assignment", "for_statement"])
            .into_iter()
            .filter_map(|node| match node.kind() {
                "parameters" => Some(node),
                _ => node.child_by_field_name("left"),
            })
        {
            for identifier in std::iter::once(target)
                .chain(ast_scan::nodes_of_kind(target, &["identifier"]))
                .filter(|node| node.kind() == "identifier")
            {
                scope.declared.insert(identifier.text().to_string());
            }
        }
        scope
    }

    /// `import a.b` binds `a`; `import a.b as c` binds `c` to `a.b`.
    fn bind_module(&mut self, name: AstNode<'_>) {
        match name.kind() {
            "dotted_name" => {
                if let Some(head) = name.named_child(0) {
                    self.bindings
                        .insert(head.text().to_string(), head.text().to_string());
                }
            }
            "aliased_import" => {
                if let (Some(module), Some(alias)) = (
                    name.child_by_field_name("name"),
                    name.child_by_field_name("alias"),
                ) {
                    self.bindings
                        .insert(alias.text().to_string(), module.text().to_string());
                }
            }
            _ => {}
        }
    }

    /// `from a import b as c` binds `c` (or `b`) to `a.b`.
    fn bind_from(&mut self, import: AstNode<'_>) {
        let Some(module) = import.child_by_field_name("module_name") else {
            return;
        };
        for name in import.named_children().skip(1) {
            let (member, local) = match name.kind() {
                "dotted_name" => (name, name),
                "aliased_import" => match (
                    name.child_by_field_name("name"),
                    name.child_by_field_name("alias"),
                ) {
                    (Some(member), Some(alias)) => (member, alias),
                    _ => continue,
                },
                _ => continue,
            };
            self.bindings.insert(
                local.text().to_string(),
                format!("{}.{}", module.text(), member.text()),
            );
        }
    }

    /// The resolved name of an expression: a binding, an attribute path
    /// through one, or an unshadowed builtin.
    pub(crate) fn resolve(&self, node: AstNode<'_>) -> Option<String> {
        match node.kind() {
            "identifier" => crate::api_identity::resolve_identifier(
                node.text(),
                &self.bindings,
                &self.declared,
                BUILTINS,
            ),
            "attribute" => Some(format!(
                "{}.{}",
                self.resolve(node.child_by_field_name("object")?)?,
                node.child_by_field_name("attribute")?.text()
            )),
            "parenthesized_expression" => self.resolve(node.named_child(0)?),
            _ => None,
        }
    }

    /// Every resolved call in `root`, in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        crate::api_identity::resolved_calls(root, &["call"], |call| {
            self.resolve(call.child_by_field_name("function")?)
        })
    }
}

/// The value of the keyword argument `name` in a call.
pub(crate) fn keyword_argument<'a>(call: AstNode<'a>, name: &str) -> Option<AstNode<'a>> {
    call.child_by_field_name("arguments")?
        .named_children()
        .filter(|argument| argument.kind() == "keyword_argument")
        .find(|argument| {
            argument
                .child_by_field_name("name")
                .is_some_and(|key| key.text() == name)
        })
        .and_then(|argument| argument.child_by_field_name("value"))
}

/// The positional arguments of a call.
pub(crate) fn positional_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|arguments| {
            arguments
                .named_children()
                .filter(|argument| {
                    !matches!(
                        argument.kind(),
                        "keyword_argument" | "comment" | "list_splat" | "dictionary_splat"
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The value of a Python string literal without interpolation.
pub(crate) fn py_string(node: AstNode<'_>) -> Option<String> {
    if node.kind() != "string"
        || node
            .named_children()
            .any(|part| part.kind() == "interpolation")
    {
        return None;
    }
    Some(
        node.named_children()
            .filter(|part| part.kind() == "string_content")
            .map(|part| part.text())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"
import os.path
import subprocess as sp
from pdb import set_trace
from cryptography.hazmat.primitives import hashes as h

def handler(exit):
    sp.run(line, shell=True)
    os.path.join(a, b)
    set_trace()
    h.MD5()
    breakpoint()
    exit(1)
    local.thing()
"#;

    #[test]
    fn python_callees_resolve_through_imports_and_unshadowed_builtins() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Python, SOURCE)
            .unwrap();
        let scope = PyScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(api, _)| api)
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "subprocess.run",
                "os.path.join",
                "pdb.set_trace",
                "cryptography.hazmat.primitives.hashes.MD5",
                "breakpoint",
            ]
        );
    }
}

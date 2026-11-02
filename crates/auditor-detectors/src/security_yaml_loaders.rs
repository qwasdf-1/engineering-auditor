//! Which Python YAML loads construct only plain values: PyYAML's safe
//! loaders and the repository's subclasses of them, and ruamel.yaml's
//! `YAML()` instances, whose default round-trip loader builds no arbitrary
//! objects. Findings-free.

use std::collections::BTreeSet;

use auditor_ast::AstNode;

use crate::ast_scan::{self, AstScan};
use crate::{util, Language};

/// PyYAML loaders that construct only plain values (Bandit B506 exempts
/// them).
const SAFE_LOADERS: &[&str] = &["SafeLoader", "CSafeLoader", "BaseLoader"];

/// PyYAML loaders that construct arbitrary objects; a repository class of
/// the same name cannot stand in for them.
const UNSAFE_LOADERS: &[&str] = &[
    "Loader",
    "UnsafeLoader",
    "FullLoader",
    "CLoader",
    "CUnsafeLoader",
    "CFullLoader",
];

/// The safe YAML loaders a repository's Python code defines and uses.
#[derive(Default)]
pub(crate) struct YamlLoaders {
    /// Safe loader names: PyYAML's, and repository classes deriving from one
    /// that register no `python/` tag constructor.
    safe: BTreeSet<String>,
    /// Files that bind `yaml` to a ruamel.yaml `YAML(..)` instance without
    /// `typ="unsafe"`.
    ruamel_files: BTreeSet<String>,
}

impl YamlLoaders {
    pub(crate) fn collect(scan: AstScan<'_, '_>) -> Self {
        let mut classes = Vec::new();
        let mut unsafe_names = BTreeSet::new();
        let mut ruamel_files = BTreeSet::new();
        ast_scan::visit_ast_files(
            scan,
            |language| *language == Language::Python,
            |file, tree, _| {
                let root = tree.root_node();
                for class in ast_scan::nodes_of_kind(root, &["class_definition"]) {
                    let Some(name) = class.child_by_field_name("name") else {
                        continue;
                    };
                    let bases = class
                        .child_by_field_name("superclasses")
                        .map(|bases| simple_names(bases.text()))
                        .unwrap_or_default();
                    classes.push((name.text().to_string(), bases));
                }
                for call in ast_scan::nodes_of_kind(root, &["call"]) {
                    unsafe_names.extend(python_tag_registration(call));
                }
                if binds_ruamel_instance(root) {
                    ruamel_files.insert(util::path_slash(&file.relative));
                }
            },
        );
        let mut safe = SAFE_LOADERS
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        loop {
            let derived = classes
                .iter()
                .filter(|(name, bases)| {
                    !safe.contains(name)
                        && !unsafe_names.contains(name)
                        && !UNSAFE_LOADERS.contains(&name.as_str())
                        && bases.iter().any(|base| safe.contains(base))
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if derived.is_empty() {
                break;
            }
            safe.extend(derived);
        }
        Self { safe, ruamel_files }
    }

    /// Whether the `yaml.load(..)` call text names a safe loader.
    pub(crate) fn names_safe_loader(&self, call: &str) -> bool {
        util::identifiers(call)
            .iter()
            .any(|name| self.safe.contains(name))
    }

    /// Whether `call` is `yaml.load(..)` on a ruamel.yaml `YAML(..)` instance
    /// bound in its file.
    pub(crate) fn is_ruamel_load(&self, path: &str, call: AstNode<'_>) -> bool {
        call.kind() == "call"
            && call
                .child_by_field_name("function")
                .is_some_and(|callee| callee.text() == "yaml.load")
            && self.ruamel_files.contains(path)
    }
}

/// The simple names in a base-class list: `(yaml.SafeLoader, Mixin)` gives
/// `SafeLoader` and `Mixin`.
fn simple_names(bases: &str) -> Vec<String> {
    bases
        .trim_matches(['(', ')'])
        .split(',')
        .filter(|base| !base.contains('='))
        .filter_map(|base| base.trim().rsplit('.').next())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The loader names a `add_constructor`/`add_multi_constructor` call that
/// registers a `python/` tag constructs arbitrary objects for: its receiver
/// (`UniqueKeyLoader.add_constructor(..)`) or its `Loader=` argument.
fn python_tag_registration(call: AstNode<'_>) -> Vec<String> {
    let Some(callee) = call.child_by_field_name("function") else {
        return Vec::new();
    };
    let callee = callee.text();
    let Some(receiver) = callee
        .strip_suffix(".add_constructor")
        .or_else(|| callee.strip_suffix(".add_multi_constructor"))
    else {
        return Vec::new();
    };
    let text = call.text();
    if !text.contains("python/") {
        return Vec::new();
    }
    let mut names = vec![receiver.rsplit('.').next().unwrap_or(receiver).to_string()];
    if let Some((_, loader)) = text.split_once("Loader=") {
        names.extend(simple_names(
            loader.split([',', ')']).next().unwrap_or_default(),
        ));
    }
    names
}

/// Whether the module binds `yaml` to a ruamel.yaml `YAML(..)` instance
/// whose loader builds no arbitrary objects: the default round-trip loader
/// (`yaml = YAML()`) or `typ` `rt`, `safe`, or `base`
/// (`yaml = ruamel.yaml.YAML(typ="safe")`).
fn binds_ruamel_instance(root: AstNode<'_>) -> bool {
    root.text().contains("ruamel")
        && ast_scan::nodes_of_kind(root, &["assignment"])
            .into_iter()
            .any(|assignment| {
                let target = assignment.child_by_field_name("left");
                let value = assignment.child_by_field_name("right");
                target.is_some_and(|target| target.text() == "yaml")
                    && value.is_some_and(|value| {
                        value.kind() == "call"
                            && value.child_by_field_name("function").is_some_and(|callee| {
                                callee.text() == "YAML" || callee.text().ends_with(".YAML")
                            })
                            && safe_ruamel_type(value.text())
                    })
            })
}

/// A `YAML(..)` call with no `typ`, or a `typ` whose loader builds no
/// arbitrary objects.
fn safe_ruamel_type(call: &str) -> bool {
    let Some((_, typ)) = call.split_once("typ") else {
        return true;
    };
    let value = typ
        .trim_start()
        .strip_prefix('=')
        .unwrap_or_default()
        .trim_start()
        .trim_start_matches(['[', '(']);
    ["rt", "safe", "base"].iter().any(|safe| {
        value.starts_with(&format!("\"{safe}\"")) || value.starts_with(&format!("'{safe}'"))
    })
}

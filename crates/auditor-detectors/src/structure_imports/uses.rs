//! The files a package import's uses land in.
//!
//! A Go import, and a Java wildcard import, names a package, not a file, so
//! its resolved path is a stand-in: the package's first source file. The
//! repository map's import edges should instead reach the files whose
//! declarations the importer uses (`model.APIKey` reaches
//! `internal/model/api_key.go`), or a package's fan-in lands on whichever
//! file sorts first. Detectors keep the stand-in, which is right at package
//! granularity.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{AstNode, AstTree};

use crate::ast_scan::nodes_of_kind;
use crate::Language;

use super::super::{StructureFile, StructureImport};

/// A Go file's package-level names and the qualified names it uses.
#[derive(Clone, Debug, Default)]
pub(crate) struct GoPackageNames {
    /// The functions, types, constants, and variables it declares at the top
    /// level.
    pub(crate) declared: BTreeSet<String>,
    /// Each `qualifier.Name` it writes, as a type or an expression.
    pub(crate) qualified: BTreeSet<(String, String)>,
}

pub(crate) fn go_package_names(language: &Language, tree: &AstTree) -> GoPackageNames {
    if *language != Language::Go {
        return GoPackageNames::default();
    }
    let root = tree.root_node();
    let mut declared = BTreeSet::new();
    for node in root.named_children() {
        match node.kind() {
            "function_declaration" => declared.extend(field_text(node, "name")),
            "type_declaration" => declared.extend(
                node.named_children()
                    .filter(|spec| matches!(spec.kind(), "type_spec" | "type_alias"))
                    .filter_map(|spec| field_text(spec, "name")),
            ),
            "const_declaration" | "var_declaration" => {
                for spec in nodes_of_kind(node, &["const_spec", "var_spec"]) {
                    declared.extend(
                        spec.named_children()
                            .filter(|child| child.kind() == "identifier")
                            .map(|name| name.text().to_string()),
                    );
                }
            }
            _ => {}
        }
    }
    let mut qualified = BTreeSet::new();
    for node in nodes_of_kind(root, &["qualified_type"]) {
        qualified.extend(qualifier_pair(node, "package", "name"));
    }
    for node in nodes_of_kind(root, &["selector_expression"]) {
        qualified.extend(qualifier_pair(node, "operand", "field").filter(|_| {
            node.child_by_field_name("operand").map(|o| o.kind()) == Some("identifier")
        }));
    }
    GoPackageNames {
        declared,
        qualified,
    }
}

fn field_text(node: AstNode<'_>, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|child| child.text().to_string())
}

fn qualifier_pair(node: AstNode<'_>, qualifier: &str, name: &str) -> Option<(String, String)> {
    Some((field_text(node, qualifier)?, field_text(node, name)?))
}

/// Sets each package import's `used_paths`: the package's files that declare
/// a name the importer uses through it.
pub(crate) fn resolve_used_paths(files: &mut [StructureFile]) {
    let mut packages: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        if matches!(file.language, Language::Go | Language::Java) {
            packages
                .entry(directory(&file.path))
                .or_default()
                .push(index);
        }
    }
    let used = files
        .iter()
        .map(|file| {
            file.imports
                .iter()
                .map(|import| used_paths(files, &packages, file, import))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (file, used) in files.iter_mut().zip(used) {
        for (import, used) in file.imports.iter_mut().zip(used) {
            import.used_paths = used;
        }
    }
}

fn used_paths(
    files: &[StructureFile],
    packages: &BTreeMap<&str, Vec<usize>>,
    file: &StructureFile,
    import: &StructureImport,
) -> Vec<String> {
    let Some(stand_in) = import.resolved_path.as_deref() else {
        return Vec::new();
    };
    let Some(members) = packages.get(directory(stand_in)) else {
        return Vec::new();
    };
    let members = members.iter().map(|index| &files[*index]);
    match file.language {
        Language::Go => {
            let Some(binding) = go_binding(members.clone(), stand_in, import) else {
                return Vec::new();
            };
            let names = file
                .go_names
                .qualified
                .iter()
                .filter(|(qualifier, _)| qualifier == binding)
                .map(|(_, name)| name)
                .collect::<BTreeSet<_>>();
            members
                .filter(|member| {
                    member.language == Language::Go && !member.path.ends_with("_test.go")
                })
                .filter(|member| {
                    names
                        .iter()
                        .any(|name| member.go_names.declared.contains(*name))
                })
                .map(|member| member.path.clone())
                .collect()
        }
        Language::Java
            if import
                .canonical
                .as_deref()
                .is_some_and(|c| c.ends_with(".*")) =>
        {
            let names = file
                .references
                .iter()
                .map(|reference| reference.name.as_str())
                .collect::<BTreeSet<_>>();
            members
                .filter(|member| member.language == Language::Java)
                .filter(|member| names.contains(java_class_name(&member.path)))
                .map(|member| member.path.clone())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The name a Go import binds: its alias, or the package clause of the
/// imported package. A blank or dot import binds no qualifier.
fn go_binding<'f>(
    mut members: impl Iterator<Item = &'f StructureFile>,
    stand_in: &str,
    import: &'f StructureImport,
) -> Option<&'f str> {
    if let Some(alias) = import.alias.as_deref() {
        return (!matches!(alias, "_" | ".")).then_some(alias);
    }
    members
        .find(|member| member.path == stand_in)?
        .module
        .as_deref()
}

fn java_class_name(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".java").unwrap_or(name)
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

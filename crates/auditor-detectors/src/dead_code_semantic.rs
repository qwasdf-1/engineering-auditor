use std::collections::{HashMap, HashSet};

use crate::ast_scan::{self, AstScan};
use crate::dead_code::{SymbolDef, MIN_NAME_LENGTH};
use crate::{util, Language};

pub(super) fn collect_definitions(scan: AstScan<'_, '_>) -> Vec<SymbolDef> {
    let mut definitions = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| *language == Language::Rust,
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            for (node_kind, symbol_kind) in [
                ("function_item", "function"),
                ("struct_item", "struct"),
                ("enum_item", "enum"),
                ("trait_item", "trait"),
            ] {
                for node in ast_scan::nodes_of_kind(tree.root_node(), &[node_kind]) {
                    let Some(name) = node.child_by_field_name("name") else {
                        continue;
                    };
                    let name = name.text();
                    if name.len() < MIN_NAME_LENGTH
                        || rust_node_is_public(node)
                        || ast_scan::is_inside_rust_test(node)
                        || node_kind == "function_item" && is_inside_trait_contract(node)
                        || ast_scan::preceding_attributes(node)
                            .any(|attribute| attribute.text().contains("dead_code"))
                    {
                        continue;
                    }
                    definitions.push(SymbolDef {
                        name: name.to_string(),
                        kind: symbol_kind,
                        file: relative.clone(),
                        line: node.start_position().0 + 1,
                        language: Language::Rust,
                    });
                }
            }
        },
    );
    definitions
}

pub(super) fn reference_counts(
    definitions: &[SymbolDef],
    scan: AstScan<'_, '_>,
) -> HashMap<String, usize> {
    let names = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<HashSet<_>>();
    let mut references = HashMap::new();
    ast_scan::visit_ast_files(
        scan,
        |language| *language == Language::Rust,
        |_, tree, _| {
            let facts = tree.semantic_facts();
            for reference in facts.references.iter() {
                let name = tree.text_for_span(reference.span);
                if names.contains(name) {
                    *references.entry(name.to_string()).or_insert(0) += 1;
                }
            }
            // A declaration quarantined for a syntax error may still use a
            // definition; its words count, so recovery cannot orphan one.
            let quarantined = tree
                .quarantined()
                .iter()
                .flat_map(|quarantine| util::identifiers(&quarantine.text));
            let attributes = ast_scan::nodes_of_kind(
                tree.root_node(),
                &["attribute_item", "inner_attribute_item"],
            )
            .into_iter()
            .flat_map(|attribute| util::identifiers(attribute.text()));
            for identifier in quarantined.chain(attributes) {
                if names.contains(identifier.as_str()) {
                    *references.entry(identifier).or_insert(0) += 1;
                }
            }
        },
    );
    references
}

fn rust_node_is_public(node: auditor_ast::AstNode<'_>) -> bool {
    node.text()
        .lines()
        .map(str::trim_start)
        .find(|line| !line.starts_with("#["))
        .is_some_and(|line| line.starts_with("pub ") || line.starts_with("pub("))
}

fn is_inside_trait_contract(mut node: auditor_ast::AstNode<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "trait_item"
            || parent.kind() == "impl_item" && parent.child_by_field_name("trait").is_some()
        {
            return true;
        }
        node = parent;
    }
    false
}

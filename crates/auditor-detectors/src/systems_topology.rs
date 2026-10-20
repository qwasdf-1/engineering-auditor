use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use auditor_ast::SymbolKind;

use super::systems_facts::{TopologyEdge, TopologyEdgeKind, TopologyFile};
use crate::structure_index::{StructureFile, StructureImport, StructureIndex};
use crate::Language;

pub(super) struct TopologyParts {
    pub(super) files: BTreeMap<String, TopologyFile>,
    pub(super) edges: Vec<TopologyEdge>,
    pub(super) cycles: Vec<Vec<String>>,
    pub(super) local_edge_candidates: usize,
}

pub(super) fn build_topology(structure: &StructureIndex) -> TopologyParts {
    let mut files = structure
        .files()
        .iter()
        .filter(|file| file.emittable)
        .map(|file| (file.path.clone(), topology_file(file)))
        .collect::<BTreeMap<_, _>>();
    let roots = local_roots(files.keys().map(String::as_str));
    let known_paths = files.keys().cloned().collect::<BTreeSet<_>>();
    let structure_files = structure
        .files()
        .iter()
        .filter(|file| file.emittable)
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    let mut collector = TopologyCollector::new(&roots, &known_paths, &structure_files);
    for file in structure.files().iter().filter(|file| file.emittable) {
        collector.collect_file(file);
    }
    collector.edges.sort();
    collector.edges.dedup();
    apply_unresolved_counts(&mut files, collector.unresolved);
    TopologyParts {
        cycles: deterministic_cycles(files.keys(), &collector.edges),
        files,
        edges: collector.edges,
        local_edge_candidates: collector.candidates,
    }
}

struct TopologyCollector<'a> {
    roots: &'a BTreeSet<String>,
    known_paths: &'a BTreeSet<String>,
    structure_files: &'a BTreeMap<&'a str, &'a StructureFile>,
    edges: Vec<TopologyEdge>,
    candidates: usize,
    unresolved: BTreeMap<String, usize>,
}

impl<'a> TopologyCollector<'a> {
    fn new(
        roots: &'a BTreeSet<String>,
        known_paths: &'a BTreeSet<String>,
        structure_files: &'a BTreeMap<&'a str, &'a StructureFile>,
    ) -> Self {
        Self {
            roots,
            known_paths,
            structure_files,
            edges: Vec::new(),
            candidates: 0,
            unresolved: BTreeMap::new(),
        }
    }

    fn collect_file(&mut self, file: &StructureFile) {
        self.collect_imports(file);
        self.collect_inheritance(file);
    }

    fn collect_imports(&mut self, file: &StructureFile) {
        for import in &file.imports {
            if import.resolved_path.is_none() && !is_local_import(import, file, self.roots) {
                continue;
            }
            self.candidates += 1;
            let Some(target) =
                resolved_import_target(import, self.known_paths, self.structure_files)
            else {
                *self.unresolved.entry(file.path.clone()).or_default() += 1;
                continue;
            };
            // `pub use self::{..}` re-exports, or an external crate that shares
            // the local crate's name, resolve to the importing file itself.
            if *target == file.path || doc_only_java_import(file, import) {
                continue;
            }
            self.edges.push(import_edge(file, import, target));
        }
    }

    fn collect_inheritance(&mut self, file: &StructureFile) {
        for symbol in file.symbols.iter().filter(|symbol| symbol.base.is_some()) {
            let base = symbol.base.as_deref().unwrap_or_default();
            let Some(import) = file.imports.iter().find(|item| import_matches(item, base)) else {
                continue;
            };
            self.candidates += 1;
            let Some(target) = import
                .resolved_path
                .as_ref()
                .filter(|target| self.known_paths.contains(*target))
            else {
                continue;
            };
            self.edges.push(TopologyEdge {
                source: file.path.clone(),
                target: target.clone(),
                source_package: package_name(&file.path),
                target_package: package_name(target),
                line: symbol.line,
                symbol: Some(base.to_string()),
                kind: TopologyEdgeKind::Inheritance,
            });
        }
    }
}

/// A Java type import named nowhere in code but its own declaration, such as
/// one kept for a javadoc `{@link}` (comments carry no references), is no
/// dependency (operator decision, 2026-10-18).
fn doc_only_java_import(file: &StructureFile, import: &StructureImport) -> bool {
    file.language == Language::Java
        && import.imported.as_deref().is_some_and(|name| {
            name != "*"
                && !file.references.iter().any(|reference| {
                    reference.name == name && reference.span.start_line != import.line
                })
        })
}

fn resolved_import_target<'a>(
    import: &'a StructureImport,
    known_paths: &BTreeSet<String>,
    structure_files: &BTreeMap<&str, &StructureFile>,
) -> Option<&'a String> {
    let target = import
        .resolved_path
        .as_ref()
        .filter(|target| known_paths.contains(*target))?;
    let exports_symbol = import
        .imported
        .as_deref()
        .is_none_or(|name| target_exports_symbol(structure_files, target, name));
    exports_symbol.then_some(target)
}

fn import_edge(file: &StructureFile, import: &StructureImport, target: &str) -> TopologyEdge {
    TopologyEdge {
        source: file.path.clone(),
        target: target.to_string(),
        source_package: package_name(&file.path),
        target_package: package_name(target),
        line: import.line,
        symbol: import.imported.clone().or_else(|| import.alias.clone()),
        kind: if import.reexport {
            TopologyEdgeKind::Reexport
        } else {
            TopologyEdgeKind::Import
        },
    }
}

fn target_exports_symbol(
    files: &BTreeMap<&str, &StructureFile>,
    target: &str,
    imported: &str,
) -> bool {
    imported == "*"
        || files
            .get(target)
            .is_some_and(|file| file.symbols.iter().any(|symbol| symbol.name == imported))
}

fn topology_file(file: &StructureFile) -> TopologyFile {
    let mut result = TopologyFile {
        package: package_name(&file.path),
        ..TopologyFile::default()
    };
    for symbol in &file.symbols {
        match symbol.kind {
            SymbolKind::Trait | SymbolKind::Interface | SymbolKind::Enum => {
                result.abstract_types += 1;
                result.type_names.push(symbol.name.clone());
            }
            SymbolKind::Class | SymbolKind::Object | SymbolKind::Struct | SymbolKind::Union => {
                result.concrete_types += 1;
                result.type_names.push(symbol.name.clone());
            }
            _ => {}
        }
    }
    result.type_names.sort();
    result.type_names.dedup();
    result
}

fn import_matches(import: &StructureImport, name: &str) -> bool {
    import.imported.as_deref() == Some(name)
        || import.alias.as_deref() == Some(name)
        || import
            .canonical
            .as_deref()
            .is_some_and(|canonical| canonical.ends_with(name))
}

fn local_roots<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    paths
        .filter_map(|path| {
            let parts = path.split('/').collect::<Vec<_>>();
            match parts.as_slice() {
                ["crates" | "packages" | "apps", name, ..] => Some((*name).to_string()),
                ["src", name, ..] if !name.contains('.') => Some((*name).to_string()),
                [name, ..] => Some(name.trim_end_matches(".py").to_string()),
                _ => None,
            }
        })
        .collect()
}

fn is_local_import(
    import: &StructureImport,
    file: &StructureFile,
    roots: &BTreeSet<String>,
) -> bool {
    let module = import.module.as_deref().unwrap_or_default();
    match &file.language {
        Language::Rust => {
            module.starts_with("crate::")
                || module.starts_with("self::")
                || module.starts_with("super::")
                || module
                    .split("::")
                    .next()
                    .is_some_and(|root| roots.contains(root))
        }
        Language::Python => {
            module.starts_with('.')
                || module
                    .split('.')
                    .next()
                    .is_some_and(|root| roots.contains(root))
        }
        Language::JavaScript | Language::TypeScript => module.starts_with('.'),
        // A Java import that names a scanned source file is local.
        Language::Java => import.resolved_path.is_some(),
        // A Go import under the file's own module path is local.
        Language::Go => file.go_module.as_ref().is_some_and(|go_module| {
            module
                .strip_prefix(&go_module.path)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        }),
        _ => false,
    }
}

fn package_name(path: &str) -> String {
    let parts = Path::new(path)
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [root @ ("crates" | "packages" | "apps"), name, ..] => format!("{root}/{name}"),
        ["src", name, ..] if !name.contains('.') => format!("src/{name}"),
        [name, ..] if parts.len() > 1 => (*name).to_string(),
        _ => "__root__".to_string(),
    }
}

fn apply_unresolved_counts(
    files: &mut BTreeMap<String, TopologyFile>,
    unresolved: BTreeMap<String, usize>,
) {
    for (path, count) in unresolved {
        if let Some(file) = files.get_mut(&path) {
            file.unresolved_local_edges = count;
        }
    }
}

fn deterministic_cycles<'a>(
    nodes: impl Iterator<Item = &'a String>,
    edges: &[TopologyEdge],
) -> Vec<Vec<String>> {
    let (outgoing, incoming) = adjacency(edges);
    let finish = finishing_order(nodes.cloned().collect(), &outgoing);
    strongly_connected_components(finish, &outgoing, &incoming)
}

type Adjacency = BTreeMap<String, Vec<String>>;

fn adjacency(edges: &[TopologyEdge]) -> (Adjacency, Adjacency) {
    let mut outgoing = Adjacency::new();
    let mut incoming = Adjacency::new();
    for edge in edges {
        outgoing
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        incoming
            .entry(edge.target.clone())
            .or_default()
            .push(edge.source.clone());
    }
    for values in outgoing.values_mut().chain(incoming.values_mut()) {
        values.sort();
        values.dedup();
    }
    (outgoing, incoming)
}

fn finishing_order(nodes: Vec<String>, outgoing: &Adjacency) -> Vec<String> {
    let mut visited = BTreeSet::new();
    let mut finish = Vec::new();
    for node in nodes {
        if visited.contains(&node) {
            continue;
        }
        let mut stack = vec![(node, false)];
        while let Some((current, expanded)) = stack.pop() {
            if expanded {
                finish.push(current);
            } else if visited.insert(current.clone()) {
                stack.push((current.clone(), true));
                stack.extend(
                    outgoing
                        .get(&current)
                        .into_iter()
                        .flatten()
                        .rev()
                        .filter(|next| !visited.contains(*next))
                        .cloned()
                        .map(|next| (next, false)),
                );
            }
        }
    }
    finish
}

fn strongly_connected_components(
    mut finish: Vec<String>,
    outgoing: &Adjacency,
    incoming: &Adjacency,
) -> Vec<Vec<String>> {
    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    while let Some(node) = finish.pop() {
        if assigned.insert(node.clone()) {
            let component = connected_component(node, incoming, &mut assigned);
            if is_cycle(&component, outgoing) {
                components.push(component);
            }
        }
    }
    components.sort();
    components
}

fn connected_component(
    node: String,
    incoming: &Adjacency,
    assigned: &mut BTreeSet<String>,
) -> Vec<String> {
    let mut component = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        component.push(current.clone());
        stack.extend(
            incoming
                .get(&current)
                .into_iter()
                .flatten()
                .rev()
                .filter(|next| assigned.insert((*next).clone()))
                .cloned(),
        );
    }
    component.sort();
    component
}

fn is_cycle(component: &[String], outgoing: &Adjacency) -> bool {
    component.len() > 1
        || component.first().is_some_and(|node| {
            outgoing
                .get(node)
                .is_some_and(|targets| targets.contains(node))
        })
}

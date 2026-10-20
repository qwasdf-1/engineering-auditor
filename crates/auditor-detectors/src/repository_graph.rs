#![cfg_attr(not(feature = "graph-analysis"), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use auditor_core::{Confidence, Finding, Severity};
use serde_json::Value;

const ISOLATED_MIN_LINES: usize = 25;
const HIGH_FANOUT_THRESHOLD: usize = 12;
const LARGE_SPLIT_MIN_LINES: usize = 1000;
const LARGE_SPLIT_MIN_SYMBOLS: usize = 3;
const SMALL_CONSOLIDATION_MAX_LINES: usize = 100;
const SHOTGUN_MIN_CO_EDITS: usize = 3;
const SHOTGUN_MIN_FILES: usize = 3;

fn advisory_at_least(strict: usize) -> usize {
    (strict * 3).div_ceil(4)
}

fn advisory_at_most(strict: usize) -> usize {
    strict + (strict / 2).max(1)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphInputs {
    pub repo_graph: Option<RepoGraph>,
    pub co_edit_journal: Option<CoEditJournal>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoGraph {
    pub repo_graph_id: Option<String>,
    pub nodes: Vec<RepoGraphNode>,
    pub edges: Vec<RepoGraphEdge>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoGraphNode {
    pub path: String,
    pub node_kind: String,
    pub language: Option<String>,
    pub line_count: Option<usize>,
    pub imports: Vec<String>,
    pub file: Option<String>,
    pub abstract_types: Option<usize>,
    pub defined_types: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoGraphEdge {
    pub from_path: String,
    pub to_path: String,
    pub edge_kind: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoEditJournal {
    pub records: Vec<CoEditRecord>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoEditRecord {
    pub change_set_id: String,
    pub files: Vec<String>,
    pub edge_kind: Option<String>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
}

pub trait GraphProvider {
    fn load(&self, repo_root: &Path) -> Result<GraphInputs, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoGraphProvider;

impl GraphProvider for NoGraphProvider {
    fn load(&self, _repo_root: &Path) -> Result<GraphInputs, String> {
        Ok(GraphInputs::default())
    }
}

impl GraphProvider for GraphInputs {
    fn load(&self, _repo_root: &Path) -> Result<GraphInputs, String> {
        Ok(self.clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct JsonGraphProvider {
    pub repo_graph: Option<PathBuf>,
    pub co_edit_journal: Option<PathBuf>,
}

impl GraphProvider for JsonGraphProvider {
    fn load(&self, repo_root: &Path) -> Result<GraphInputs, String> {
        Ok(GraphInputs {
            repo_graph: self
                .repo_graph
                .as_ref()
                .map(|path| parse_repo_graph(&read_input(repo_root, path)?))
                .transpose()?,
            co_edit_journal: self
                .co_edit_journal
                .as_ref()
                .map(|path| parse_co_edit_journal(&read_input(repo_root, path)?))
                .transpose()?,
        })
    }
}

fn read_input(root: &Path, path: &Path) -> Result<String, String> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::read_to_string(&resolved)
        .map_err(|error| format!("failed to read graph input {}: {error}", resolved.display()))
}

pub fn parse_repo_graph(text: &str) -> Result<RepoGraph, String> {
    let object = versioned_object(text, "repo graph")?;
    let nodes = array_field(&object, "repo graph", "nodes")?;
    let edges = array_field(&object, "repo graph", "edges")?;
    let graph = RepoGraph {
        repo_graph_id: string(object.get("repo_graph_id")),
        nodes: nodes.iter().map(parse_node).collect::<Result<_, _>>()?,
        edges: edges.iter().map(parse_edge).collect::<Result<_, _>>()?,
    };
    validate_repo_graph(&graph)?;
    Ok(graph)
}

pub fn parse_co_edit_journal(text: &str) -> Result<CoEditJournal, String> {
    let object = versioned_object(text, "co-edit journal")?;
    let records = array_field(&object, "co-edit journal", "records")?;
    Ok(CoEditJournal {
        records: records
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let item = value
                    .as_object()
                    .ok_or_else(|| format!("co-edit record {index} must be an object"))?;
                let extensions_id = item
                    .get("extensions")
                    .and_then(Value::as_object)
                    .and_then(|v| string(v.get("change_set_id")));
                let record = CoEditRecord {
                    change_set_id: extensions_id
                        .or_else(|| string(item.get("change_set_id")))
                        .unwrap_or_else(|| format!("record-{index}")),
                    files: item
                        .get("files")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(strip_repo_file))
                        .collect(),
                    edge_kind: string(item.get("edge_kind")),
                    from_address: string(item.get("from_address")),
                    to_address: string(item.get("to_address")),
                };
                if record.change_set_id.is_empty()
                    || (record.files.is_empty()
                        && (record.from_address.is_none() || record.to_address.is_none()))
                {
                    return Err(format!(
                        "co-edit record {index} requires a change_set_id and files or an address pair"
                    ));
                }
                Ok(record)
            })
            .collect::<Result<_, String>>()?,
    })
}

/// A JSON document's top-level object, after its schema version check.
fn versioned_object(text: &str, label: &str) -> Result<serde_json::Map<String, Value>, String> {
    let Value::Object(object) =
        serde_json::from_str(text).map_err(|error| format!("invalid {label} JSON: {error}"))?
    else {
        return Err(format!("{label} must be a JSON object"));
    };
    require_schema_version(&object, label)?;
    Ok(object)
}

fn array_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    label: &str,
    key: &str,
) -> Result<&'a Vec<Value>, String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} `{key}` must be an array"))
}

fn require_schema_version(
    object: &serde_json::Map<String, Value>,
    label: &str,
) -> Result<(), String> {
    match object.get("schema_version").and_then(Value::as_u64) {
        Some(1) => Ok(()),
        Some(version) => Err(format!("unsupported {label} schema_version {version}")),
        None => Err(format!("{label} requires integer schema_version 1")),
    }
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}
fn number(value: Option<&Value>) -> Option<usize> {
    value.and_then(Value::as_u64).map(|v| v as usize)
}
fn parse_node(value: &Value) -> Result<RepoGraphNode, String> {
    let item = value
        .as_object()
        .ok_or_else(|| "repo graph node must be an object".to_string())?;
    let path =
        string(item.get("path")).ok_or_else(|| "repo graph node requires path".to_string())?;
    let node_kind = string(item.get("node_kind"))
        .ok_or_else(|| format!("repo graph node `{path}` requires node_kind"))?;
    Ok(RepoGraphNode {
        path,
        node_kind,
        language: string(item.get("language")),
        line_count: number(item.get("line_count")),
        imports: item
            .get("imports")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        file: string(item.get("file")),
        abstract_types: number(item.get("abstract_types")),
        defined_types: number(item.get("defined_types")),
    })
}
fn parse_edge(value: &Value) -> Result<RepoGraphEdge, String> {
    let item = value
        .as_object()
        .ok_or_else(|| "repo graph edge must be an object".to_string())?;
    let edge = RepoGraphEdge {
        from_path: string(item.get("from"))
            .or_else(|| string(item.get("from_path")))
            .ok_or_else(|| "repo graph edge requires from".to_string())?,
        to_path: string(item.get("to"))
            .or_else(|| string(item.get("to_path")))
            .ok_or_else(|| "repo graph edge requires to".to_string())?,
        edge_kind: string(item.get("edge_kind"))
            .ok_or_else(|| "repo graph edge requires edge_kind".to_string())?,
    };
    Ok(edge)
}

fn validate_repo_graph(graph: &RepoGraph) -> Result<(), String> {
    let paths = graph
        .nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<BTreeSet<_>>();
    if paths.len() != graph.nodes.len() {
        return Err("repo graph node paths must be unique".to_string());
    }
    let declared_imports = declared_import_count(graph);
    let resolved_imports = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.edge_kind == "imports"
                && paths.contains(edge.from_path.as_str())
                && paths.contains(edge.to_path.as_str())
        })
        .count()
        + graph
            .nodes
            .iter()
            .flat_map(|node| node.imports.iter())
            .filter(|target| paths.contains(target.as_str()))
            .count();
    if declared_imports > 0 && resolved_imports == 0 {
        return Err(
            "repo graph declares imports but resolves no graph-local file topology".to_string(),
        );
    }
    Ok(())
}
fn strip_repo_file(value: &str) -> String {
    value
        .strip_prefix("repo_file:")
        .unwrap_or(value)
        .to_string()
}
fn source_path(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|v| v.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "mjs" | "py" | "sh")
    )
}
fn test_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.split('/')
        .any(|v| matches!(v, "test" | "tests" | "fixture" | "fixtures"))
        || p.rsplit('/').next().is_some_and(|n| {
            (n.ends_with(".py") && n.starts_with("test_"))
                || n.ends_with("_test.rs")
                || n.ends_with("_tests.rs")
                || n.ends_with("tests.rs")
                || n.contains(".test.")
                || n.contains(".spec.")
        })
}

type FileNodes<'a> = BTreeMap<&'a str, &'a RepoGraphNode>;
type Edges<'a> = BTreeMap<&'a str, BTreeSet<&'a str>>;
fn graph_parts(graph: &RepoGraph) -> (FileNodes<'_>, Edges<'_>) {
    let files: FileNodes<'_> = graph
        .nodes
        .iter()
        .filter(|n| n.node_kind == "file" && source_path(&n.path) && !test_path(&n.path))
        .map(|n| (n.path.as_str(), n))
        .collect();
    let mut edges: Edges<'_> = files.keys().map(|path| (*path, BTreeSet::new())).collect();
    for edge in &graph.edges {
        if edge.edge_kind == "imports"
            && files.contains_key(edge.from_path.as_str())
            && files.contains_key(edge.to_path.as_str())
        {
            edges
                .entry(&edge.from_path)
                .or_default()
                .insert(&edge.to_path);
        }
    }
    for node in files.values() {
        for target in &node.imports {
            if files.contains_key(target.as_str()) {
                edges.entry(&node.path).or_default().insert(target);
            }
        }
    }
    (files, edges)
}

/// Imports the graph declares: `imports` edges plus per-node import lists.
fn declared_import_count(graph: &RepoGraph) -> usize {
    graph
        .edges
        .iter()
        .filter(|edge| edge.edge_kind == "imports")
        .count()
        + graph
            .nodes
            .iter()
            .map(|node| node.imports.len())
            .sum::<usize>()
}

/// The repository graph with its files and import edges, when its import
/// topology is complete enough to judge.
fn usable_topology(inputs: &GraphInputs) -> Option<(&RepoGraph, FileNodes<'_>, Edges<'_>)> {
    let graph = inputs.repo_graph.as_ref()?;
    let (files, edges) = graph_parts(graph);
    import_topology_usable(graph, &edges).then_some((graph, files, edges))
}

fn import_topology_usable(graph: &RepoGraph, edges: &Edges<'_>) -> bool {
    let declared = declared_import_count(graph);
    declared == 0 || edges.values().any(|targets| !targets.is_empty())
}
fn reverse<'a>(edges: &Edges<'a>) -> Edges<'a> {
    let mut result = Edges::new();
    for (from, targets) in edges {
        for target in targets {
            result.entry(target).or_default().insert(from);
        }
    }
    result
}
fn finding(
    id: &str,
    category: &str,
    title: &'static str,
    message: String,
    path: &str,
    recommendation: &str,
) -> Finding {
    auditor_core::finding!(
        id,
        category,
        title,
        message,
        Severity::Medium,
        Confidence::Medium,
        path,
        None,
    )
    .with_recommendation(recommendation)
}

pub(crate) fn repo_graph_isolated_source_file_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some((graph, files, edges)) = usable_topology(inputs) else {
        return Vec::new();
    };
    let incoming = reverse(&edges);
    files.into_iter().filter(|(path,node)| node.line_count.unwrap_or(0)>=advisory_at_least(ISOLATED_MIN_LINES) && edges.get(path).is_none_or(BTreeSet::is_empty) && incoming.get(path).is_none_or(BTreeSet::is_empty)).map(|(path,node)| { let mut result=finding("repo_graph_isolated_source_file","architecture","Isolated source file","Source file is isolated in the repository graph.".into(),path,"Confirm the file is intentionally standalone or connect/remove it through the owning module boundary.").with_evidence("repo_graph_id",graph.repo_graph_id.clone().unwrap_or_default()).with_evidence("line_count",node.line_count.unwrap_or(0)); if node.line_count.unwrap_or(0)<ISOLATED_MIN_LINES {result.severity=Severity::Advisory;} result }).collect()
}
pub(crate) fn repo_graph_high_fanout_source_file_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some((_, _, edges)) = usable_topology(inputs) else {
        return Vec::new();
    };
    edges
        .into_iter()
        .filter(|(_, v)| v.len() >= advisory_at_least(HIGH_FANOUT_THRESHOLD))
        .map(|(path, targets)| {
            let fanout = targets.len();
            let mut result = finding(
                "repo_graph_high_fanout_source_file",
                "architecture",
                "High graph fan-out",
                format!("Source file imports {} graph-local source files.", fanout),
                path,
                "Split the owner boundary behind a narrower facade or dependency set.",
            )
            .with_evidence("fanout", fanout)
            .with_evidence(
                "imports",
                targets.into_iter().map(str::to_string).collect::<Vec<_>>(),
            );
            if fanout < HIGH_FANOUT_THRESHOLD {
                result.severity = Severity::Advisory;
            }
            result
        })
        .collect()
}
fn canonical_cycle(items: &[&str]) -> Vec<String> {
    let mut options = Vec::new();
    for i in 0..items.len() {
        let rotated = items[i..]
            .iter()
            .chain(&items[..i])
            .copied()
            .collect::<Vec<_>>();
        options.push(rotated.iter().map(|v| v.to_string()).collect::<Vec<_>>());
        options.push(
            rotated
                .iter()
                .rev()
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
        );
    }
    options.into_iter().min().unwrap_or_default()
}
fn visit<'a>(
    start: &'a str,
    current: &'a str,
    edges: &Edges<'a>,
    path: &mut Vec<&'a str>,
    cycles: &mut BTreeSet<Vec<String>>,
) {
    if path.contains(&current) {
        return;
    }
    path.push(current);
    for target in edges.get(current).into_iter().flatten() {
        if *target == start && path.len() > 1 {
            cycles.insert(canonical_cycle(path));
        } else if *target >= start {
            visit(start, target, edges, path, cycles);
        }
    }
    path.pop();
}

fn cyclic_nodes<'a>(edges: &Edges<'a>) -> BTreeSet<&'a str> {
    let nodes = edges.keys().copied().collect::<Vec<_>>();
    let indexes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (*node, index))
        .collect::<BTreeMap<_, _>>();
    let adjacency = nodes
        .iter()
        .map(|node| {
            edges
                .get(node)
                .into_iter()
                .flatten()
                .filter_map(|target| indexes.get(target).copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    struct Tarjan<'a> {
        adjacency: &'a [Vec<usize>],
        next: usize,
        indexes: Vec<Option<usize>>,
        low: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        cyclic: BTreeSet<usize>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, node: usize) {
            let index = self.next;
            self.next += 1;
            self.indexes[node] = Some(index);
            self.low[node] = index;
            self.stack.push(node);
            self.on_stack[node] = true;
            for &target in &self.adjacency[node] {
                if self.indexes[target].is_none() {
                    self.visit(target);
                    self.low[node] = self.low[node].min(self.low[target]);
                } else if self.on_stack[target] {
                    self.low[node] = self.low[node].min(self.indexes[target].unwrap());
                }
            }
            if self.low[node] != index {
                return;
            }
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            if component.len() > 1
                || component
                    .first()
                    .is_some_and(|member| self.adjacency[*member].contains(member))
            {
                self.cyclic.extend(component);
            }
        }
    }
    let count = nodes.len();
    let mut tarjan = Tarjan {
        adjacency: &adjacency,
        next: 0,
        indexes: vec![None; count],
        low: vec![0; count],
        stack: Vec::new(),
        on_stack: vec![false; count],
        cyclic: BTreeSet::new(),
    };
    for node in 0..count {
        if tarjan.indexes[node].is_none() {
            tarjan.visit(node);
        }
    }
    tarjan
        .cyclic
        .into_iter()
        .map(|index| nodes[index])
        .collect()
}

pub(crate) fn repo_graph_import_cycle_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some((_, _, edges)) = usable_topology(inputs) else {
        return Vec::new();
    };
    let cyclic = cyclic_nodes(&edges);
    if cyclic.is_empty() {
        return Vec::new();
    }
    let edges: Edges<'_> = edges
        .into_iter()
        .filter(|(source, _)| cyclic.contains(source))
        .map(|(source, targets)| {
            (
                source,
                targets
                    .into_iter()
                    .filter(|target| cyclic.contains(target))
                    .collect(),
            )
        })
        .collect();
    let mut cycles = BTreeSet::new();
    for start in edges.keys() {
        visit(start, start, &edges, &mut vec![], &mut cycles)
    }
    cycles
        .into_iter()
        .map(|cycle| {
            finding(
                "repo_graph_import_cycle",
                "architecture",
                "Repository import cycle",
                format!("Repository graph import cycle spans {} files.", cycle.len()),
                &cycle[0],
                "Break the cycle through dependency inversion or an owner-local abstraction.",
            )
            .with_evidence("cycle", cycle)
        })
        .collect()
}

fn symbol_components(graph: &RepoGraph, file: &str) -> Vec<BTreeSet<String>> {
    let symbols: BTreeSet<String> = graph
        .edges
        .iter()
        .filter(|e| e.edge_kind == "contains_symbol" && e.from_path == file)
        .map(|e| e.to_path.clone())
        .chain(
            graph
                .nodes
                .iter()
                .filter(|n| n.node_kind == "symbol" && n.file.as_deref() == Some(file))
                .map(|n| n.path.clone()),
        )
        .collect();
    let references = graph.edges.iter().filter(|e| {
        e.edge_kind == "references"
            && symbols.contains(&e.from_path)
            && symbols.contains(&e.to_path)
    });
    let mut links = undirected(references.map(|e| (&e.from_path, &e.to_path)));
    for symbol in symbols {
        links.entry(symbol).or_default();
    }
    connected_components(&links)
}

/// An undirected adjacency map of `pairs`.
fn undirected<'a>(
    pairs: impl Iterator<Item = (&'a String, &'a String)>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut links: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (a, b) in pairs {
        links.entry(a.clone()).or_default().insert(b.clone());
        links.entry(b.clone()).or_default().insert(a.clone());
    }
    links
}

/// The connected components of an undirected adjacency map.
fn connected_components(links: &BTreeMap<String, BTreeSet<String>>) -> Vec<BTreeSet<String>> {
    let mut remaining = links.keys().cloned().collect::<BTreeSet<_>>();
    let mut components = vec![];
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed.clone()]);
        let mut stack = vec![seed];
        while let Some(node) = stack.pop() {
            for next in links.get(&node).into_iter().flatten() {
                if remaining.remove(next) {
                    component.insert(next.clone());
                    stack.push(next.clone())
                }
            }
        }
        components.push(component)
    }
    components
}
pub(crate) fn large_file_split_candidate_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some(graph) = &inputs.repo_graph else {
        return vec![];
    };
    let (files, _) = graph_parts(graph);
    files.into_iter().filter_map(|(path,node)|{if node.line_count.unwrap_or(0)<advisory_at_least(LARGE_SPLIT_MIN_LINES){return None}let clusters=symbol_components(graph,path).into_iter().filter(|c|c.len()>=LARGE_SPLIT_MIN_SYMBOLS).collect::<Vec<_>>();if clusters.len()<2{return None}let mut result=finding("large_file_split_candidate","structural","Large file has disjoint symbol clusters",format!("File has {} lines and {} disjoint symbol clusters with no cross-references.",node.line_count.unwrap_or(0),clusters.len()),path,"Split along the cited clusters only when doing so does not increase net source volume.").with_evidence("line_count",node.line_count.unwrap_or(0)).with_evidence("cluster_count",clusters.len()).with_evidence("cross_cluster_references",0usize);if node.line_count.unwrap_or(0)<LARGE_SPLIT_MIN_LINES{result.severity=Severity::Advisory;}Some(result)}).collect()
}
pub(crate) fn small_file_consolidation_candidate_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some((_, files, edges)) = usable_topology(inputs) else {
        return Vec::new();
    };
    let incoming = reverse(&edges);
    let entry = [
        "mod.rs",
        "lib.rs",
        "main.rs",
        "index.ts",
        "index.js",
        "__init__.py",
    ];
    files.into_iter().filter_map(|(path,node)|{let lines=node.line_count.unwrap_or(0);let importers=incoming.get(path)?;if lines==0||lines>advisory_at_most(SMALL_CONSOLIDATION_MAX_LINES)||importers.len()!=1||entry.contains(&Path::new(path).file_name()?.to_str()?){return None}let importer=*importers.iter().next()?;let mut result=finding("small_file_consolidation_candidate","structural","Small single-importer source file",format!("Source file has {lines} lines and a single importer `{importer}`."),path,"Fold the module into its single importer unless it is a deliberate owner boundary.").with_evidence("line_count",lines).with_evidence("importer",importer);if lines>SMALL_CONSOLIDATION_MAX_LINES{result.severity=Severity::Advisory;}Some(result)}).collect()
}

fn co_edit_pairs(journal: &CoEditJournal) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut pairs = BTreeMap::new();
    for record in &journal.records {
        let files = co_edited_files(record);
        for (i, first) in files.iter().enumerate() {
            for second in &files[i + 1..] {
                let key = if first < second {
                    (first.clone(), second.clone())
                } else {
                    (second.clone(), first.clone())
                };
                pairs
                    .entry(key)
                    .or_insert_with(BTreeSet::new)
                    .insert(record.change_set_id.clone());
            }
        }
    }
    pairs
}

/// The distinct files one record co-edits; a work-graph co-edit edge names
/// exactly its two endpoints.
fn co_edited_files(record: &CoEditRecord) -> Vec<String> {
    if record.edge_kind.as_deref() == Some("work_graph_change_set_co_edit") {
        if let (Some(a), Some(b)) = (&record.from_address, &record.to_address) {
            return vec![strip_repo_file(a), strip_repo_file(b)];
        }
    }
    record
        .files
        .iter()
        .map(|v| strip_repo_file(v))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub(crate) fn shotgun_surgery_cluster_findings(inputs: &GraphInputs) -> Vec<Finding> {
    let Some(journal) = &inputs.co_edit_journal else {
        return vec![];
    };
    let coupled = co_edit_pairs(journal)
        .into_iter()
        .filter(|(_, sets)| sets.len() >= SHOTGUN_MIN_CO_EDITS)
        .collect::<BTreeMap<_, _>>();
    let neighbors = undirected(coupled.keys().map(|(a, b)| (a, b)));
    let mut out = vec![];
    for cluster in connected_components(&neighbors) {
        if cluster.len() >= SHOTGUN_MIN_FILES {
            let changes = coupled
                .iter()
                .filter(|((a, b), _)| cluster.contains(a) && cluster.contains(b))
                .flat_map(|(_, s)| s)
                .cloned()
                .collect::<BTreeSet<_>>();
            let files = cluster.into_iter().collect::<Vec<_>>();
            out.push(
                finding(
                    "shotgun_surgery_cluster",
                    "architecture",
                    "Change-coupled file cluster",
                    format!(
                        "{} files are co-edited across {} change sets.",
                        files.len(),
                        changes.len()
                    ),
                    &files[0],
                    "Consolidate the change-coupled behavior behind one owner boundary.",
                )
                .with_evidence("files", files)
                .with_evidence("change_set_count", changes.len()),
            )
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_edge_aliases_and_rejects_bad_shapes() {
        let graph = parse_repo_graph(
            r#"{"schema_version":1,"nodes":[{"path":"a.rs","node_kind":"file"},{"path":"b.rs","node_kind":"file"}],"edges":[{"from_path":"a.rs","to_path":"b.rs","edge_kind":"imports"}]}"#,
        )
        .unwrap();
        assert_eq!(graph.edges[0].from_path, "a.rs");
        assert!(parse_repo_graph("[]").is_err());
        assert!(parse_co_edit_journal(r#"{"schema_version":1,"records":{}}"#).is_err());
    }
    #[test]
    fn graph_detectors_cover_positive_and_noise_cases() {
        let graph=parse_repo_graph(&format!(r#"{{"schema_version":1,"nodes":[{{"path":"solo.rs","node_kind":"file","line_count":25}},{{"path":"hub.rs","node_kind":"file","line_count":30}}{}],"edges":[]}}"#,(0..12).map(|i|format!(r#",{{"path":"d{i}.rs","node_kind":"file","line_count":1}}"#)).collect::<String>())).unwrap();
        let mut graph = graph;
        graph.nodes[1].imports = (0..12).map(|i| format!("d{i}.rs")).collect();
        let input = GraphInputs {
            repo_graph: Some(graph),
            co_edit_journal: None,
        };
        assert!(repo_graph_isolated_source_file_findings(&input)
            .iter()
            .any(|f| f.primary_path() == "solo.rs"));
        assert_eq!(repo_graph_high_fanout_source_file_findings(&input).len(), 1);
    }

    #[test]
    fn unresolved_import_statements_fail_closed() {
        assert!(parse_repo_graph(
            r#"{"schema_version":1,"nodes":[{"path":"app.py","node_kind":"file","line_count":100,"imports":["from helpers import value"]}],"edges":[{"from":"app.py","to":"from helpers import value","edge_kind":"imports"}]}"#,
        )
        .is_err());
    }
}

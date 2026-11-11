//! The report's findings joined with the repository map, indexed once for
//! the orientation pack and the retrieval tools.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use auditor_core::sha256_digest_bytes;
use serde_json::Value;

use super::document::Row;
use super::modules::{cycles, Modules};
use crate::history::{FileHistory, HistoryDigest};
use crate::repository_map::{MapEdge, MapFile, MapSymbol, RepositoryMap};

/// Map symbol kinds that enclose code; a finding is attributed to the
/// innermost one around its line.
const ENCLOSING_KINDS: [&str; 9] = [
    "function",
    "method",
    "class",
    "object",
    "struct",
    "union",
    "interface",
    "trait",
    "enum",
];

/// The path and line range a `source-span:path#Lstart-Lend` ID names.
pub(super) fn source_span(id: &str) -> Option<(&str, usize, usize)> {
    let (path, lines) = id.strip_prefix("source-span:")?.rsplit_once("#L")?;
    let (start, end) = lines.split_once("-L")?;
    Some((path, start.parse().ok()?, end.parse().ok()?))
}

/// Map edges by the node they leave or enter.
type EdgeIndex<'a> = HashMap<&'a str, Vec<usize>>;

/// Locals and fields: kept out of searches and member lists.
pub(super) const LOCAL_KINDS: [&str; 3] = ["variable", "parameter", "field"];

pub(super) struct ReportFinding {
    pub(super) id: String,
    pub(super) detector: String,
    pub(super) layer: String,
    pub(super) severity: String,
    pub(super) message: String,
    pub(super) path: Option<String>,
    pub(super) line: Option<usize>,
    pub(super) file: Option<usize>,
    pub(super) symbol: Option<usize>,
    pub(super) evidence: Value,
}

impl ReportFinding {
    pub(super) fn evidence_id(&self) -> String {
        format!("finding:{}", self.id)
    }

    pub(super) fn location(&self) -> String {
        match (&self.path, self.line) {
            (Some(path), Some(line)) => format!("{path}:{line}"),
            (Some(path), None) => path.clone(),
            _ => "repository".to_string(),
        }
    }
}

pub(super) struct DetectorRun {
    pub(super) id: String,
    pub(super) layer: String,
    pub(super) status: String,
    pub(super) missing_inputs: Vec<String>,
    pub(super) failure_reason: Option<String>,
}

/// A scanned repository as the reviewer reads it. Built from a report and
/// the repository map it references, both already verified by the caller
/// (`load_and_validate_repository_map` checks the map against the live
/// tree).
pub struct RepositoryView<'a> {
    pub(super) map: &'a RepositoryMap,
    pub(super) report: &'a Value,
    pub(super) source_digest: String,
    pub(super) modules: Modules,
    pub(super) findings: Vec<ReportFinding>,
    pub(super) detectors: Vec<DetectorRun>,
    /// Repository-internal imports between modules, by count.
    pub(super) module_imports: BTreeMap<(usize, usize), usize>,
    pub(super) module_cycles: Vec<Vec<usize>>,
    pub(super) history: Option<&'a HistoryDigest>,
    history_by_file: Vec<Option<usize>>,
    file_by_id: HashMap<&'a str, usize>,
    file_by_path: HashMap<&'a str, usize>,
    symbol_by_id: HashMap<&'a str, usize>,
    symbols_by_file: Vec<Vec<usize>>,
    findings_by_file: Vec<Vec<usize>>,
    children: HashMap<usize, Vec<usize>>,
    edges_from: EdgeIndex<'a>,
    edges_to: EdgeIndex<'a>,
}

impl<'a> RepositoryView<'a> {
    pub fn new(report: &'a Value, map: &'a RepositoryMap) -> Result<Self, String> {
        if report["repository_map"]["source_snapshot_digest"].as_str()
            != Some(map.source_snapshot_digest.as_str())
        {
            return Err("the repository map is not the one the report references".to_string());
        }
        let file_by_id = index_by(&map.files, |file| file.id.as_str());
        let symbol_by_id = index_by(&map.symbols, |symbol| symbol.id.as_str());
        let (symbols_by_file, children) = symbol_indexes(map, &file_by_id, &symbol_by_id);
        let (edges_from, edges_to) = edge_indexes(map);
        let modules = Modules::partition(map.files.iter().map(|file| file.path.as_str()));
        let module_imports = module_imports(map, &file_by_id, &modules);
        let module_cycles = cycles(modules.len(), &module_imports);
        let layers = detector_layers(report);
        let mut view = Self {
            map,
            report,
            source_digest: source_digest(report, map)?,
            modules,
            findings: Vec::new(),
            detectors: detector_runs(report),
            module_imports,
            module_cycles,
            history: None,
            history_by_file: Vec::new(),
            file_by_id,
            file_by_path: index_by(&map.files, |file| file.path.as_str()),
            symbol_by_id,
            symbols_by_file,
            findings_by_file: vec![Vec::new(); map.files.len()],
            children,
            edges_from,
            edges_to,
        };
        let findings = report["findings"]
            .as_array()
            .ok_or("the report has no findings list")?
            .iter()
            .map(|finding| view.report_finding(finding, &layers))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, finding) in findings.iter().enumerate() {
            if let Some(file) = finding.file {
                view.findings_by_file[file].push(index);
            }
        }
        view.findings = findings;
        Ok(view)
    }

    /// Joins a history digest (`load_history_digest` checked it against the
    /// repository): its files by path, its bytes bound into every
    /// projection.
    pub fn with_history(mut self, history: &'a HistoryDigest) -> Self {
        let mut by_file = vec![None; self.map.files.len()];
        for (index, entry) in history.files.iter().enumerate() {
            if let Some(file) = self.file_by_path(&entry.path) {
                by_file[file] = Some(index);
            }
        }
        let bound = format!("{}\n{}", self.source_digest, history.sha256);
        self.source_digest = sha256_digest_bytes(bound.as_bytes());
        self.history = Some(history);
        self.history_by_file = by_file;
        self
    }

    fn report_finding(
        &self,
        finding: &Value,
        layers: &HashMap<&str, &str>,
    ) -> Result<ReportFinding, String> {
        let text = |field: &str| finding[field].as_str().unwrap_or_default().to_string();
        let id = text("id");
        if id.is_empty() {
            return Err("a report finding has no id".to_string());
        }
        let location = &finding["locations"][0];
        let path = location["path"].as_str().map(ToString::to_string);
        let line = location["line"].as_u64().map(|line| line as usize);
        let file = path
            .as_deref()
            .and_then(|path| self.file_by_path.get(path).copied());
        let symbol = file
            .zip(line)
            .and_then(|(file, line)| self.enclosing_symbol(file, line));
        let detector = text("detector");
        Ok(ReportFinding {
            layer: layers
                .get(detector.as_str())
                .map_or("L?", |layer| layer)
                .to_string(),
            detector,
            severity: text("severity"),
            message: text("message"),
            evidence: finding["evidence"].clone(),
            id,
            path,
            line,
            file,
            symbol,
        })
    }

    /// The innermost function or type around a line.
    fn enclosing_symbol(&self, file: usize, line: usize) -> Option<usize> {
        self.symbols_by_file[file]
            .iter()
            .copied()
            .filter(|index| {
                let symbol = &self.map.symbols[*index];
                ENCLOSING_KINDS.contains(&symbol.kind.as_str())
                    && symbol.span.start_line <= line
                    && line <= symbol.span.end_line
            })
            .min_by_key(|index| {
                let span = self.map.symbols[*index].span;
                (span.end_byte - span.start_byte, *index)
            })
    }

    pub(super) fn file(&self, index: usize) -> &'a MapFile {
        &self.map.files[index]
    }

    pub(super) fn symbol(&self, index: usize) -> &'a MapSymbol {
        &self.map.symbols[index]
    }

    pub(super) fn file_by_id(&self, id: &str) -> Option<usize> {
        self.file_by_id.get(id).copied()
    }

    pub(super) fn file_by_path(&self, path: &str) -> Option<usize> {
        self.file_by_path.get(path).copied()
    }

    pub(super) fn symbol_by_id(&self, id: &str) -> Option<usize> {
        self.symbol_by_id.get(id).copied()
    }

    pub(super) fn symbol_file(&self, symbol: usize) -> usize {
        self.file_by_id[self.symbol(symbol).file_id.as_str()]
    }

    pub(super) fn symbols_in_file(&self, file: usize) -> &[usize] {
        &self.symbols_by_file[file]
    }

    pub(super) fn findings_in_file(&self, file: usize) -> &[usize] {
        &self.findings_by_file[file]
    }

    pub(super) fn children(&self, symbol: usize) -> &[usize] {
        self.children.get(&symbol).map_or(&[], Vec::as_slice)
    }

    /// `Outer.inner`: the symbol's name under its parents' names.
    pub(super) fn qualified_name(&self, symbol: usize) -> String {
        let mut names = vec![self.symbol(symbol).name.as_str()];
        let mut current = self.symbol(symbol);
        while let Some(parent) = current
            .parent_id
            .as_deref()
            .and_then(|id| self.symbol_by_id(id))
        {
            current = self.symbol(parent);
            names.push(current.name.as_str());
        }
        names.retain(|name| !name.is_empty());
        names.reverse();
        names.join(".")
    }

    /// `path:start-end` of a symbol.
    pub(super) fn symbol_location(&self, symbol: usize) -> String {
        let span = self.symbol(symbol).span;
        format!(
            "{}:{}-{}",
            self.file(self.symbol_file(symbol)).path,
            span.start_line,
            span.end_line
        )
    }

    /// Edges of `kind` leaving `node`, by map index.
    pub(super) fn edges_from(&self, node: &str, kind: &str) -> Vec<(usize, &'a MapEdge)> {
        self.edges(self.edges_from.get(node), kind)
    }

    /// Edges of `kind` entering `node`, by map index.
    pub(super) fn edges_to(&self, node: &str, kind: &str) -> Vec<(usize, &'a MapEdge)> {
        self.edges(self.edges_to.get(node), kind)
    }

    fn edges(&self, indices: Option<&Vec<usize>>, kind: &str) -> Vec<(usize, &'a MapEdge)> {
        indices
            .into_iter()
            .flatten()
            .map(|index| (*index, &self.map.edges[*index]))
            .filter(|(_, edge)| edge.kind == kind)
            .collect()
    }

    /// Repository files a file imports and is imported by, each counted once.
    pub(super) fn import_degree(&self, file: usize) -> (usize, usize) {
        let id = self.file(file).id.as_str();
        let distinct = |edges: Vec<(usize, &MapEdge)>, end: fn(&MapEdge) -> &str| {
            edges
                .into_iter()
                .map(|(_, edge)| end(edge))
                .filter(|other| *other != id)
                .collect::<BTreeSet<_>>()
                .len()
        };
        (
            distinct(self.edges_to(id, "imports"), |edge| edge.from.as_str()),
            distinct(self.edges_from(id, "imports"), |edge| edge.to.as_str()),
        )
    }

    /// A file's changes in the history digest's window.
    pub(super) fn file_history(&self, file: usize) -> Option<&'a FileHistory> {
        let index = (*self.history_by_file.get(file)?)?;
        Some(&self.history?.files[index])
    }

    /// Whether `left` imports `right`, and whether `right` imports `left`.
    pub(super) fn import_links(&self, left: usize, right: usize) -> (bool, bool) {
        let imports = |from: usize, to: usize| {
            self.edges_from(&self.file(from).id, "imports")
                .iter()
                .any(|(_, edge)| edge.to == self.file(to).id)
        };
        (imports(left, right), imports(right, left))
    }

    pub(super) fn module_lines(&self, module: usize) -> usize {
        self.modules
            .members(module)
            .iter()
            .map(|file| self.file(*file).lines)
            .sum()
    }

    pub(super) fn module_findings(&self, module: usize) -> impl Iterator<Item = usize> + '_ {
        self.modules
            .members(module)
            .iter()
            .flat_map(|file| self.findings_in_file(*file).iter().copied())
    }

    /// Whether a file is production or tooling code, not data or prose:
    /// what size and coupling rankings cover.
    pub(super) fn is_production_code(&self, file: usize) -> bool {
        let file = self.file(file);
        matches!(file.source_role.as_str(), "production" | "tooling")
            && file.lines > 0
            && !matches!(
                file.language.as_str(),
                "json" | "yaml" | "toml" | "markdown" | "xml" | "text" | "csv" | "ini"
            )
    }
}

/// Rows the pack and the tools share, each citing what it shows.
impl RepositoryView<'_> {
    pub(super) fn symbol_row(&self, symbol: usize) -> Row {
        Row::new(
            format!(
                "`{}` {} at {}",
                self.qualified_name(symbol),
                self.symbol(symbol).kind,
                self.symbol_location(symbol)
            ),
            [self.symbol(symbol).id.clone()],
        )
    }

    pub(super) fn module_row(&self, module: usize) -> Row {
        Row::new(
            format!(
                "{}: {}, {}, {}",
                self.modules.name(module),
                count(self.modules.members(module).len(), "file"),
                count(self.module_lines(module), "line"),
                count(self.module_findings(module).count(), "finding")
            ),
            [self.modules.id(module)],
        )
    }

    pub(super) fn entry_row(&self, index: usize) -> Row {
        let entry = &self.map.entry_points[index];
        let path = self
            .file_by_id(&entry.file_id)
            .map_or("?", |file| self.file(file).path.as_str());
        let mut text = format!("{} `{}` at {path}:{}", entry.kind, entry.detail, entry.line);
        let mut ids = vec![format!("entry:{index}")];
        if let Some(symbol) = entry
            .symbol_id
            .as_deref()
            .and_then(|id| self.symbol_by_id(id))
        {
            text.push_str(&format!(" in `{}`", self.qualified_name(symbol)));
            ids.push(self.symbol(symbol).id.clone());
        }
        Row::new(text, ids)
    }

    pub(super) fn finding_row(&self, finding: usize) -> Row {
        let finding = &self.findings[finding];
        Row::new(
            format!(
                "{} {} at {}: {}",
                finding.severity,
                finding.detector,
                finding.location(),
                finding.message
            ),
            [finding.evidence_id()],
        )
    }

    /// A finding row with the detector's evidence values after the message.
    pub(super) fn finding_detail_row(&self, finding: usize) -> Row {
        let mut row = self.finding_row(finding);
        if let Some(evidence) = super::evidence::evidence_line(&self.findings[finding].evidence) {
            row.text.push_str(&format!(" (evidence: {evidence})"));
        }
        row
    }

    pub(super) fn detector_row(&self, detector: &str, findings: &[usize]) -> Row {
        let highest = findings
            .iter()
            .map(|finding| self.findings[*finding].severity.as_str())
            .min_by_key(|severity| severity_rank(severity))
            .unwrap_or_default();
        let layer = findings
            .first()
            .map_or("L?", |finding| self.findings[*finding].layer.as_str());
        Row::new(
            format!(
                "{detector} ({layer}): {}, highest {highest}",
                count(findings.len(), "finding")
            ),
            [format!("detector:{detector}")],
        )
    }

    pub(super) fn dependency_row(&self, from: usize, to: usize, imports: usize) -> Row {
        let (from, to) = (self.modules.name(from), self.modules.name(to));
        Row::new(
            format!("{from} → {to}: {}", count(imports, "import")),
            [format!("module-dependency:{from}->{to}")],
        )
    }

    /// A module cycle by its position in [`Self::module_cycles`].
    pub(super) fn cycle_row(&self, cycle: usize) -> Row {
        let names = self.module_cycles[cycle]
            .iter()
            .map(|module| self.modules.name(*module))
            .collect::<Vec<_>>();
        Row::new(
            format!(
                "{} modules import each other: {}",
                names.len(),
                names.join(", ")
            ),
            [format!("module-cycle:{}", cycle + 1)],
        )
    }

    /// Findings inside a symbol's span, most severe first.
    pub(super) fn findings_in_symbol(&self, symbol: usize) -> impl Iterator<Item = usize> + '_ {
        let span = self.symbol(symbol).span;
        let file = self.symbol_file(symbol);
        self.ranked_findings(move |finding| {
            finding.file == Some(file)
                && finding
                    .line
                    .is_some_and(|line| (span.start_line..=span.end_line).contains(&line))
        })
    }

    /// A symbol's members other than locals and fields, in source order.
    pub(super) fn members(&self, symbol: usize) -> Vec<usize> {
        let mut members = self
            .children(symbol)
            .iter()
            .copied()
            .filter(|child| !LOCAL_KINDS.contains(&self.symbol(*child).kind.as_str()))
            .collect::<Vec<_>>();
        members.sort_by_key(|child| self.symbol(*child).span.start_line);
        members
    }

    /// Visibility, async, metrics, external base, and enclosing symbol.
    pub(super) fn symbol_details(&self, symbol: usize) -> String {
        let mapped = self.symbol(symbol);
        let mut details = vec![format!("{} visibility", mapped.visibility)];
        if mapped.is_async {
            details.push("async".to_string());
        }
        if let Some(metrics) = mapped.metrics {
            details.push(format!(
                "{}, {}, {}",
                count(metrics.statements, "statement"),
                count(metrics.locals, "local"),
                count(metrics.returns, "return")
            ));
        }
        if let Some(base) = &mapped.external_base {
            details.push(format!("extends external `{base}`"));
        }
        let parent = mapped
            .parent_id
            .as_deref()
            .and_then(|parent| self.symbol_by_id(parent))
            .map(|parent| self.qualified_name(parent))
            .filter(|parent| !parent.is_empty());
        if let Some(parent) = parent {
            details.push(format!("inside `{parent}`"));
        }
        details.join("; ")
    }

    /// One row per edge, naming the symbol at its far end.
    pub(super) fn edge_rows(
        &self,
        edges: Vec<(usize, &MapEdge)>,
        far_end: fn(&MapEdge) -> &str,
    ) -> Vec<Row> {
        edges
            .into_iter()
            .filter_map(|(index, edge)| {
                let other = self.symbol_by_id(far_end(edge))?;
                let line = edge
                    .line
                    .map_or(String::new(), |line| format!(" at line {line}"));
                Some(Row::new(
                    format!(
                        "`{}` ({}){line}",
                        self.qualified_name(other),
                        self.symbol_location(other)
                    ),
                    [format!("edge:{index}"), self.symbol(other).id.clone()],
                ))
            })
            .collect()
    }

    /// Detector rows for a set of findings, the most frequent first.
    pub(super) fn detector_rows(&self, findings: impl IntoIterator<Item = usize>) -> Vec<Row> {
        let mut detectors: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for finding in findings {
            detectors
                .entry(self.findings[finding].detector.as_str())
                .or_default()
                .push(finding);
        }
        let mut detectors = detectors.into_iter().collect::<Vec<_>>();
        detectors.sort_by_key(|(id, findings)| (std::cmp::Reverse(findings.len()), *id));
        detectors
            .into_iter()
            .map(|(id, findings)| self.detector_row(id, &findings))
            .collect()
    }

    /// A module's imports (`outgoing`) or importers, by import count.
    pub(super) fn module_dependency_rows(&self, module: usize, outgoing: bool) -> Vec<Row> {
        let mut pairs = self
            .module_imports
            .iter()
            .filter(|((from, to), _)| {
                if outgoing {
                    *from == module
                } else {
                    *to == module
                }
            })
            .collect::<Vec<_>>();
        pairs.sort_by_key(|(pair, count)| (std::cmp::Reverse(**count), **pair));
        pairs
            .into_iter()
            .map(|((from, to), count)| self.dependency_row(*from, *to, *count))
            .collect()
    }

    /// The findings that match, most severe first, then by location.
    pub(super) fn ranked_findings(
        &self,
        matches: impl Fn(&ReportFinding) -> bool,
    ) -> impl Iterator<Item = usize> {
        let mut selected = self
            .findings
            .iter()
            .enumerate()
            .filter(|(_, finding)| matches(finding))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        selected.sort_by(|left, right| {
            let (left, right) = (&self.findings[*left], &self.findings[*right]);
            (
                severity_rank(&left.severity),
                &left.path,
                left.line,
                &left.id,
            )
                .cmp(&(
                    severity_rank(&right.severity),
                    &right.path,
                    right.line,
                    &right.id,
                ))
        });
        selected.into_iter()
    }
}

/// The evidence ID of two files changing together, the paths in order.
pub(super) fn co_change_id(left: &str, right: &str) -> String {
    let (first, second) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    format!("co-change:{first}+{second}")
}

/// `1 file`, `2 files`.
pub(super) fn count(number: usize, noun: &str) -> String {
    if number == 1 {
        format!("1 {noun}")
    } else {
        format!("{number} {noun}s")
    }
}

/// Orders severities from the most severe.
pub(super) fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// The report's and the map's digests, bound together.
fn source_digest(report: &Value, map: &RepositoryMap) -> Result<String, String> {
    let report_bytes = serde_json::to_vec(report).map_err(|error| error.to_string())?;
    let bound = format!(
        "{}\n{}",
        sha256_digest_bytes(&report_bytes),
        map.source_snapshot_digest
    );
    Ok(sha256_digest_bytes(bound.as_bytes()))
}

/// Each file's symbols, and each symbol's children.
fn symbol_indexes(
    map: &RepositoryMap,
    file_by_id: &HashMap<&str, usize>,
    symbol_by_id: &HashMap<&str, usize>,
) -> (Vec<Vec<usize>>, HashMap<usize, Vec<usize>>) {
    let mut symbols_by_file = vec![Vec::new(); map.files.len()];
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, symbol) in map.symbols.iter().enumerate() {
        symbols_by_file[file_by_id[symbol.file_id.as_str()]].push(index);
        if let Some(parent) = symbol
            .parent_id
            .as_deref()
            .and_then(|id| symbol_by_id.get(id))
        {
            children.entry(*parent).or_default().push(index);
        }
    }
    (symbols_by_file, children)
}

/// Non-containment edges by the node they leave and the node they enter.
fn edge_indexes(map: &RepositoryMap) -> (EdgeIndex<'_>, EdgeIndex<'_>) {
    let mut edges_from: EdgeIndex<'_> = HashMap::new();
    let mut edges_to: EdgeIndex<'_> = HashMap::new();
    for (index, edge) in map.edges.iter().enumerate() {
        if edge.kind != "contains" {
            edges_from
                .entry(edge.from.as_str())
                .or_default()
                .push(index);
            edges_to.entry(edge.to.as_str()).or_default().push(index);
        }
    }
    (edges_from, edges_to)
}

fn index_by<T>(items: &[T], key: impl Fn(&T) -> &str) -> HashMap<&str, usize> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| (key(item), index))
        .collect()
}

fn module_imports(
    map: &RepositoryMap,
    files: &HashMap<&str, usize>,
    modules: &Modules,
) -> BTreeMap<(usize, usize), usize> {
    // An import statement counts once, though a package import has an edge
    // to each file whose names it uses.
    let mut statements = BTreeSet::new();
    for edge in map.edges.iter().filter(|edge| edge.kind == "imports") {
        let (Some(from), Some(to)) = (files.get(edge.from.as_str()), files.get(edge.to.as_str()))
        else {
            continue;
        };
        let (from_module, to_module) = (modules.of_file(*from), modules.of_file(*to));
        if from_module != to_module {
            let statement = edge.line.ok_or(*to);
            statements.insert((from_module, to_module, *from, statement));
        }
    }
    let mut imports = BTreeMap::new();
    for (from, to, _, _) in statements {
        *imports.entry((from, to)).or_insert(0) += 1;
    }
    imports
}

fn detector_layers(report: &Value) -> HashMap<&str, &str> {
    report["capability_executions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|run| Some((run["detector_id"].as_str()?, run["layer"].as_str()?)))
        .collect()
}

fn detector_runs(report: &Value) -> Vec<DetectorRun> {
    report["capability_executions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|run| {
            let text = |field: &str| run[field].as_str().unwrap_or_default().to_string();
            DetectorRun {
                id: text("detector_id"),
                layer: text("layer"),
                status: text("status"),
                missing_inputs: run["missing_inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect(),
                failure_reason: run["failure_reason"].as_str().map(ToString::to_string),
            }
        })
        .collect()
}

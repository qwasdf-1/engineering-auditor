use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use auditor_core::Sha256;
use auditor_report::{RepositoryMapReference, REPOSITORY_MAP_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};

const DEFAULT_LOOKUP_REQUESTS: usize = 8;
const DEFAULT_LOOKUP_EXCERPTS: usize = 16;
const DEFAULT_EXCERPT_BYTES: usize = 16 * 1024;
const DEFAULT_LOOKUP_BYTES: usize = 256 * 1024;
const DEFAULT_LOOKUP_HOPS: usize = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RepositoryMap {
    pub schema_version: u32,
    pub extractor_version: u32,
    pub profile: String,
    pub analysis_method: String,
    pub source_snapshot_digest: String,
    pub files: Vec<MapFile>,
    pub symbols: Vec<MapSymbol>,
    pub edges: Vec<MapEdge>,
    pub completeness: MapCompleteness,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapFile {
    pub id: String,
    pub path: String,
    pub language: String,
    pub source_role: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapSymbol {
    pub id: String,
    pub file_id: String,
    pub kind: String,
    pub name: String,
    pub span: MapSpan,
    pub parent_id: Option<String>,
    pub visibility: String,
    #[serde(rename = "async")]
    pub is_async: bool,
    pub metrics: Option<MapMetrics>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MapSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceExcerptKind {
    /// Excerpt resolved from a `symbol:` AST id via the repository map.
    Symbol,
    /// Complete enclosing symbol (or complete file when no symbol contains
    /// the location) resolved from a finding id at its verified `path:line`.
    Finding,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceExcerpt {
    /// Stable, unique id of the excerpt within a review. The id's
    /// value is namespaced by `kind`:
    /// - `SourceExcerptKind::Symbol` -> `symbol:<stable-digest>` (the
    ///   `MapSymbol.id`).
    /// - `SourceExcerptKind::Finding` -> `finding-excerpt:<path>:<start>-<end>`.
    pub id: String,
    pub kind: SourceExcerptKind,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MapMetrics {
    pub locals: usize,
    pub returns: usize,
    pub statements: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapEdge {
    pub from: String,
    pub kind: String,
    pub to: String,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapCompleteness {
    pub complete: bool,
    pub ambiguous_calls: usize,
    pub dynamic_imports: usize,
    pub truncated_call_edges: usize,
    pub unresolved_imports: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryMapError {
    Contract(String),
    Io(String),
    ArtifactDigestMismatch,
    StaleSourceSnapshot(String),
    LookupBudgetExceeded(String),
}

impl fmt::Display for RepositoryMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(message) | Self::Io(message) => formatter.write_str(message),
            Self::ArtifactDigestMismatch => formatter.write_str("repository map digest mismatch"),
            Self::StaleSourceSnapshot(path) => {
                write!(
                    formatter,
                    "repository map source snapshot is stale at {path}"
                )
            }
            Self::LookupBudgetExceeded(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RepositoryMapError {}

pub fn load_and_validate_repository_map(
    report_path: &Path,
    repo_root: &Path,
    reference: &RepositoryMapReference,
) -> Result<RepositoryMap, RepositoryMapError> {
    if reference.status != "present" {
        return Err(RepositoryMapError::Contract(
            "review requires an available repository map".to_string(),
        ));
    }
    let relative = reference
        .path
        .as_deref()
        .ok_or_else(|| RepositoryMapError::Contract("repository map path is absent".to_string()))?;
    let relative_path = safe_relative_path(relative, "repository map reference")?;
    if relative_path.components().count() != 1 {
        return Err(RepositoryMapError::Contract(
            "repository map must be a sibling of the report".to_string(),
        ));
    }
    let map_path = report_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative_path);
    let expected_digest = reference.digest.as_deref().ok_or_else(|| {
        RepositoryMapError::Contract("repository map digest is absent".to_string())
    })?;
    if digest_file(&map_path)? != expected_digest {
        return Err(RepositoryMapError::ArtifactDigestMismatch);
    }
    let reader = BufReader::new(
        File::open(&map_path).map_err(|error| RepositoryMapError::Io(error.to_string()))?,
    );
    let map: RepositoryMap = serde_json::from_reader(reader).map_err(|error| {
        RepositoryMapError::Contract(format!("invalid repository map: {error}"))
    })?;
    validate_contract(&map, reference)?;
    validate_live_snapshot(repo_root, &map)?;
    Ok(map)
}

fn validate_contract(
    map: &RepositoryMap,
    reference: &RepositoryMapReference,
) -> Result<(), RepositoryMapError> {
    if map.schema_version != REPOSITORY_MAP_SCHEMA_VERSION
        || map.profile != "full"
        || !matches!(map.analysis_method.as_str(), "auto" | "ast")
        || !map.completeness.complete
        || map.files.len() != reference.file_count
        || map.symbols.len() != reference.symbol_count
        || map.edges.len() != reference.edge_count
        || Some(map.source_snapshot_digest.as_str()) != reference.source_snapshot_digest.as_deref()
    {
        return Err(RepositoryMapError::Contract(
            "repository map metadata does not match its report reference".to_string(),
        ));
    }
    let mut nodes = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for file in &map.files {
        safe_relative_path(&file.path, "repository map")?;
        if !valid_sha256(&file.sha256)
            || !nodes.insert(file.id.as_str())
            || !paths.insert(file.path.as_str())
        {
            return Err(RepositoryMapError::Contract(
                "repository map contains an invalid or duplicate file".to_string(),
            ));
        }
    }
    for symbol in &map.symbols {
        let file_bytes = map
            .files
            .iter()
            .find(|file| file.id == symbol.file_id)
            .map(|file| file.bytes as usize);
        if file_bytes.is_none()
            || symbol.span.end_byte < symbol.span.start_byte
            || symbol.span.end_byte > file_bytes.unwrap_or(0)
            || symbol.span.end_line < symbol.span.start_line
            || !nodes.insert(symbol.id.as_str())
        {
            return Err(RepositoryMapError::Contract(
                "repository map contains an invalid or duplicate symbol".to_string(),
            ));
        }
    }
    if map.symbols.iter().any(|symbol| {
        symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent| !nodes.contains(parent.as_str()))
    }) {
        return Err(RepositoryMapError::Contract(
            "repository map symbol references an unknown parent".to_string(),
        ));
    }
    if map.edges.iter().any(|edge| {
        !matches!(edge.kind.as_str(), "contains" | "imports" | "calls")
            || !nodes.contains(edge.from.as_str())
            || !nodes.contains(edge.to.as_str())
    }) {
        return Err(RepositoryMapError::Contract(
            "repository map edge references an unknown node".to_string(),
        ));
    }
    Ok(())
}

fn validate_live_snapshot(root: &Path, map: &RepositoryMap) -> Result<(), RepositoryMapError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
    let mut snapshot = Sha256::new();
    for file in &map.files {
        let path = checked_source_path(&canonical_root, &file.path)?;
        let metadata = path
            .metadata()
            .map_err(|_| RepositoryMapError::StaleSourceSnapshot(file.path.clone()))?;
        let hash = digest_file(&path)?;
        if metadata.len() != file.bytes || hash != file.sha256 {
            return Err(RepositoryMapError::StaleSourceSnapshot(file.path.clone()));
        }
        let bytes = file.bytes.to_string();
        snapshot.update_record([
            file.path.as_str(),
            file.sha256.as_str(),
            file.language.as_str(),
            file.source_role.as_str(),
            bytes.as_str(),
        ]);
    }
    if snapshot.finalize() != map.source_snapshot_digest {
        return Err(RepositoryMapError::StaleSourceSnapshot(
            "<snapshot>".to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LookupBudget {
    pub max_requests: usize,
    pub max_excerpts: usize,
    pub max_excerpt_bytes: usize,
    pub max_total_bytes: usize,
    pub max_hops: usize,
}

impl Default for LookupBudget {
    fn default() -> Self {
        Self {
            max_requests: DEFAULT_LOOKUP_REQUESTS,
            max_excerpts: DEFAULT_LOOKUP_EXCERPTS,
            max_excerpt_bytes: DEFAULT_EXCERPT_BYTES,
            max_total_bytes: DEFAULT_LOOKUP_BYTES,
            max_hops: DEFAULT_LOOKUP_HOPS,
        }
    }
}

impl LookupBudget {
    pub fn tightened_by(mut self, brief: &crate::ReviewBrief) -> Self {
        if let Some(value) = brief.budgets.max_lookup_requests {
            self.max_requests = self.max_requests.min(value);
        }
        if let Some(value) = brief.budgets.max_lookup_hops {
            self.max_hops = self.max_hops.min(value);
        }
        self
    }
}

pub struct SourceLookup<'a> {
    root: PathBuf,
    map: &'a RepositoryMap,
    budget: LookupBudget,
    requests: usize,
    excerpts: usize,
    bytes: usize,
}

impl<'a> SourceLookup<'a> {
    pub fn new(
        root: &Path,
        map: &'a RepositoryMap,
        budget: LookupBudget,
    ) -> Result<Self, RepositoryMapError> {
        if budget.max_requests == 0
            || budget.max_excerpts == 0
            || budget.max_excerpt_bytes == 0
            || budget.max_total_bytes == 0
            || budget.max_hops > 2
        {
            return Err(RepositoryMapError::Contract(
                "source lookup budget is invalid or exceeds the hard two-hop limit".to_string(),
            ));
        }
        Ok(Self {
            root: root
                .canonicalize()
                .map_err(|error| RepositoryMapError::Io(error.to_string()))?,
            map,
            budget,
            requests: 0,
            excerpts: 0,
            bytes: 0,
        })
    }

    pub fn symbol_neighborhood(
        &mut self,
        symbol_id: &str,
        hops: usize,
    ) -> Result<Vec<SourceExcerpt>, RepositoryMapError> {
        if hops > self.budget.max_hops || self.requests >= self.budget.max_requests {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "source lookup request exceeds the configured budget".to_string(),
            ));
        }
        self.requests += 1;
        let selected =
            map_symbol_neighborhood(self.map, &BTreeSet::from([symbol_id.to_string()]), hops);
        let symbols = selected
            .iter()
            .filter_map(|id| self.map.symbols.iter().find(|symbol| symbol.id == *id))
            .cloned()
            .collect::<Vec<_>>();
        symbols
            .iter()
            .map(|symbol| self.read_symbol(symbol))
            .collect()
    }

    fn read_symbol(&mut self, symbol: &MapSymbol) -> Result<SourceExcerpt, RepositoryMapError> {
        if self.excerpts >= self.budget.max_excerpts {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "source lookup excerpt count exceeds the configured budget".to_string(),
            ));
        }
        let file = self
            .map
            .files
            .iter()
            .find(|file| file.id == symbol.file_id)
            .ok_or_else(|| RepositoryMapError::Contract("symbol file is absent".to_string()))?;
        let path = checked_source_path(&self.root, &file.path)?;
        if digest_file(&path)? != file.sha256 {
            return Err(RepositoryMapError::StaleSourceSnapshot(file.path.clone()));
        }
        let available = symbol.span.end_byte.saturating_sub(symbol.span.start_byte);
        self.check_byte_budget(available)?;
        let mut source = vec![0; available];
        let mut handle =
            File::open(&path).map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        handle
            .seek(SeekFrom::Start(symbol.span.start_byte as u64))
            .and_then(|_| handle.read_exact(&mut source))
            .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        self.excerpts += 1;
        self.bytes += available;
        Ok(SourceExcerpt {
            id: symbol.id.clone(),
            kind: SourceExcerptKind::Symbol,
            path: file.path.clone(),
            start_line: symbol.span.start_line,
            end_line: symbol.span.end_line,
            content: String::from_utf8(source).map_err(|_| {
                RepositoryMapError::Contract(format!(
                    "mapped source span is not valid UTF-8: {}",
                    file.path
                ))
            })?,
        })
    }

    pub(crate) fn read_source_excerpt_for_finding(
        &mut self,
        finding_id: &str,
        relative_path: &str,
        line: Option<usize>,
    ) -> Result<SourceExcerpt, RepositoryMapError> {
        if self.excerpts >= self.budget.max_excerpts {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "source lookup excerpt count exceeds the configured budget".to_string(),
            ));
        }
        let map_file = self
            .map
            .files
            .iter()
            .find(|file| file.path == relative_path)
            .ok_or_else(|| {
                RepositoryMapError::Contract(format!(
                    "source file is absent from the repository map: finding {finding_id} path {relative_path}"
                ))
            })?;
        let path = checked_source_path(&self.root, relative_path)?;
        if digest_file(&path)? != map_file.sha256 {
            return Err(RepositoryMapError::StaleSourceSnapshot(
                map_file.path.clone(),
            ));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        let total_lines = content.lines().count();
        if total_lines == 0 {
            return Err(RepositoryMapError::Contract(format!(
                "source file is empty: {}",
                path.display()
            )));
        }
        let target_line = line.unwrap_or(1);
        if target_line == 0 || target_line > total_lines {
            return Err(RepositoryMapError::Contract(format!(
                "finding line {} is out of range for {} ({} lines)",
                target_line,
                path.display(),
                total_lines
            )));
        }
        if let Some(symbol) = self
            .map
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.file_id == map_file.id
                    && symbol.span.start_line <= target_line
                    && symbol.span.end_line >= target_line
            })
            .min_by_key(|symbol| symbol.span.end_byte.saturating_sub(symbol.span.start_byte))
            .cloned()
        {
            let mut excerpt = self.read_symbol(&symbol)?;
            excerpt.id = format!(
                "finding-excerpt:{}:{}-{}",
                map_file.path, excerpt.start_line, excerpt.end_line
            );
            excerpt.kind = SourceExcerptKind::Finding;
            return Ok(excerpt);
        }
        let read_len = content.len();
        self.check_byte_budget(read_len)?;
        let id = format!("finding-excerpt:{}:{}-{}", map_file.path, 1, total_lines);
        self.excerpts += 1;
        self.bytes += read_len;
        Ok(SourceExcerpt {
            id,
            kind: SourceExcerptKind::Finding,
            path: map_file.path.clone(),
            start_line: 1,
            end_line: total_lines,
            content,
        })
    }

    fn check_byte_budget(&self, bytes: usize) -> Result<(), RepositoryMapError> {
        if bytes > self.budget.max_excerpt_bytes {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "complete source item exceeds the configured per-excerpt budget".to_string(),
            ));
        }
        if self.bytes + bytes > self.budget.max_total_bytes {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "source lookup byte count exceeds the configured budget".to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn map_symbol_neighborhood(
    map: &RepositoryMap,
    roots: &BTreeSet<String>,
    hops: usize,
) -> BTreeSet<String> {
    let mut frontier = roots.clone();
    let mut selected = frontier.clone();
    for _ in 0..hops {
        frontier = map
            .edges
            .iter()
            .filter(|edge| frontier.contains(&edge.from) || frontier.contains(&edge.to))
            .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
            .filter(|node| node.starts_with("symbol:"))
            .filter(|node| selected.insert(node.clone()))
            .collect();
    }
    selected
}

pub(crate) fn safe_relative_path(
    value: &str,
    subject: &str,
) -> Result<PathBuf, RepositoryMapError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RepositoryMapError::Contract(format!(
            "{subject} contains unsafe path: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn checked_source_path(root: &Path, relative: &str) -> Result<PathBuf, RepositoryMapError> {
    let path = root.join(safe_relative_path(relative, "repository map")?);
    let canonical = path
        .canonicalize()
        .map_err(|_| RepositoryMapError::StaleSourceSnapshot(relative.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(RepositoryMapError::Contract(format!(
            "repository source path escapes root: {relative}"
        )));
    }
    Ok(canonical)
}

fn digest_file(path: &Path) -> Result<String, RepositoryMapError> {
    let mut reader = BufReader::new(
        File::open(path).map_err(|error| RepositoryMapError::Io(error.to_string()))?,
    );
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

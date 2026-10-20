use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use auditor_ast::{SourceSpan, SymbolKind};
use auditor_core::{sha256_digest_bytes, RepositoryMapArtifact, Sha256, EXTRACTOR_VERSION};
use serde_json::{json, Value};

use crate::structure_index::{StructureFile, StructureIndex, StructureSymbol};
use crate::{AnalysisMethod, AuditedFile, ScanProfile};

const REPOSITORY_MAP_SCHEMA_VERSION: u32 = 1;
const MAX_CALL_EDGES_PER_SYMBOL: usize = 32;
const SYMBOL_KIND_NAMES: [&str; 16] = [
    "function",
    "method",
    "class",
    "object",
    "struct",
    "union",
    "interface",
    "trait",
    "enum",
    "module",
    "constant",
    "static",
    "type_alias",
    "variable",
    "parameter",
    "field",
];

pub(crate) fn write_repository_map(
    files: &[AuditedFile],
    structure: &StructureIndex,
    report_output: &Path,
    profile: ScanProfile,
    method: AnalysisMethod,
) -> io::Result<RepositoryMapArtifact> {
    let parent = report_output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stem = report_output
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("audit");
    let temporary = parent.join(format!(".{stem}.repository-map.{}.tmp", std::process::id()));
    let metadata = match write_temporary_map(files, structure, &temporary, profile, method) {
        Ok(metadata) => metadata,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    let digest = digest_file(&temporary)?;
    let name = format!("{stem}.repository-map.{digest}.json");
    let final_path = parent.join(&name);
    if final_path.exists() {
        if digest_file(&final_path)? != digest {
            let _ = fs::remove_file(&temporary);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "digest-addressed repository map path contains different bytes",
            ));
        }
        fs::remove_file(&temporary)?;
    } else {
        fs::rename(&temporary, final_path)?;
    }
    Ok(RepositoryMapArtifact {
        path: name,
        digest,
        source_snapshot_digest: metadata.source_snapshot_digest,
        completeness: "complete_with_explicit_unresolved_edges".to_string(),
        file_count: metadata.file_count,
        symbol_count: metadata.symbol_count,
        edge_count: metadata.edge_count,
    })
}

struct MapMetadata {
    source_snapshot_digest: String,
    file_count: usize,
    symbol_count: usize,
    edge_count: usize,
}

fn write_temporary_map(
    files: &[AuditedFile],
    structure: &StructureIndex,
    path: &Path,
    profile: ScanProfile,
    method: AnalysisMethod,
) -> io::Result<MapMetadata> {
    let mut ordered_files = files.iter().collect::<Vec<_>>();
    ordered_files.sort_by(|left, right| left.relative.cmp(&right.relative));
    let file_hashes = ordered_files
        .iter()
        .map(|file| Ok((slash(&file.relative), digest_file(&file.absolute)?)))
        .collect::<io::Result<BTreeMap<_, _>>>()?;
    let source_snapshot_digest = source_snapshot_digest(&ordered_files, &file_hashes);
    let mut writer = BufWriter::new(File::create(path)?);
    write!(
        writer,
        "{{\"schema_version\":{REPOSITORY_MAP_SCHEMA_VERSION},\"extractor_version\":{EXTRACTOR_VERSION},\"profile\":{},\"analysis_method\":{},\"source_snapshot_digest\":{},\"files\":[",
        json_string(profile.as_str())?,
        json_string(method.as_str())?,
        json_string(&source_snapshot_digest)?,
    )?;
    write_file_entries(&mut writer, &ordered_files, &file_hashes)?;
    writer.write_all(b"],\"symbols\":[")?;
    let symbol_ids = symbol_ids(structure);
    let symbol_count = write_symbol_entries(&mut writer, structure, &symbol_ids)?;
    writer.write_all(b"],\"edges\":[")?;
    let mut first = true;
    let mut edge_count = 0;
    let (mut unresolved_imports, mut dynamic_imports) = (0, 0);
    for (file_index, file) in structure.files().iter().enumerate() {
        edge_count +=
            write_containment_edges(&mut writer, &mut first, file_index, file, &symbol_ids)?;
        let imports = write_import_edges(&mut writer, &mut first, file)?;
        edge_count += imports.resolved;
        unresolved_imports += imports.unresolved;
        dynamic_imports += imports.dynamic;
    }
    let (call_edges, ambiguous_calls, truncated_calls) =
        write_call_edges(&mut writer, &mut first, structure, &symbol_ids)?;
    edge_count += call_edges;
    write!(
        writer,
        "],\"completeness\":{{\"complete\":true,\"ambiguous_calls\":{ambiguous_calls},\"dynamic_imports\":{dynamic_imports},\"truncated_call_edges\":{truncated_calls},\"unresolved_imports\":{unresolved_imports}}}}}"
    )?;
    writer.flush()?;
    Ok(MapMetadata {
        source_snapshot_digest,
        file_count: ordered_files.len(),
        symbol_count,
        edge_count,
    })
}

fn write_file_entries(
    writer: &mut impl Write,
    files: &[&AuditedFile],
    file_hashes: &BTreeMap<String, String>,
) -> io::Result<()> {
    let mut first = true;
    for file in files {
        write_separator(writer, &mut first)?;
        let path = slash(&file.relative);
        write_json(
            writer,
            &json!({
                "bytes": file.bytes,
                "id": file_id(&path),
                "language": file.language.as_str(),
                "sha256": file_hashes[&path],
                "path": path,
                "source_role": file.role.as_str(),
            }),
        )?;
    }
    Ok(())
}

/// Writes every symbol with its function metrics; returns the count.
fn write_symbol_entries(
    writer: &mut impl Write,
    structure: &StructureIndex,
    symbol_ids: &BTreeMap<(usize, usize), String>,
) -> io::Result<usize> {
    let mut first = true;
    let mut count = 0;
    for (file_index, file) in structure.files().iter().enumerate() {
        for (symbol_index, symbol) in file.symbols.iter().enumerate() {
            write_separator(writer, &mut first)?;
            let metrics = file
                .function_metrics
                .get(&(symbol_index as u32))
                .map(|metrics| {
                    json!({
                        "locals": metrics.locals,
                        "returns": metrics.returns,
                        "statements": metrics.statements,
                    })
                });
            write_json(
                writer,
                &json!({
                    "async": symbol.is_async,
                    "file_id": file_id(&file.path),
                    "id": symbol_ids[&(file_index, symbol_index)],
                    "kind": symbol_kind(symbol.kind),
                    "metrics": metrics,
                    "name": symbol.name,
                    "parent_id": symbol.parent.and_then(|parent| symbol_ids.get(&(file_index, parent as usize))),
                    "span": span_value(symbol.span),
                    "visibility": if symbol.is_public { "public" } else { "non_public" },
                }),
            )?;
            count += 1;
        }
    }
    Ok(count)
}

struct ImportCounts {
    resolved: usize,
    unresolved: usize,
    dynamic: usize,
}

/// Writes a file's resolved import edges and counts the rest.
fn write_import_edges(
    writer: &mut impl Write,
    first: &mut bool,
    file: &StructureFile,
) -> io::Result<ImportCounts> {
    let mut counts = ImportCounts {
        resolved: 0,
        unresolved: 0,
        dynamic: 0,
    };
    for import in &file.imports {
        counts.dynamic += usize::from(import.dynamic);
        let Some(target) = import.resolved_path.as_deref() else {
            counts.unresolved += 1;
            continue;
        };
        write_separator(writer, first)?;
        write_json(
            writer,
            &json!({
                "from": file_id(&file.path),
                "kind": "imports",
                "line": import.line,
                "to": file_id(target),
            }),
        )?;
        counts.resolved += 1;
    }
    Ok(counts)
}

fn write_containment_edges(
    writer: &mut impl Write,
    first: &mut bool,
    file_index: usize,
    file: &StructureFile,
    symbol_ids: &BTreeMap<(usize, usize), String>,
) -> io::Result<usize> {
    let mut count = 0;
    for (symbol_index, symbol) in file.symbols.iter().enumerate() {
        let Some(parent) = symbol.parent else {
            continue;
        };
        let Some(parent_id) = symbol_ids.get(&(file_index, parent as usize)) else {
            continue;
        };
        write_separator(writer, first)?;
        write_json(
            writer,
            &json!({
                "from": parent_id,
                "kind": "contains",
                "to": symbol_ids[&(file_index, symbol_index)],
            }),
        )?;
        count += 1;
    }
    Ok(count)
}

fn write_call_edges(
    writer: &mut impl Write,
    first: &mut bool,
    structure: &StructureIndex,
    symbol_ids: &BTreeMap<(usize, usize), String>,
) -> io::Result<(usize, usize, usize)> {
    let mut symbols_by_name: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        for (symbol_index, symbol) in file.symbols.iter().enumerate() {
            symbols_by_name
                .entry(&symbol.name)
                .or_default()
                .push((file_index, symbol_index));
        }
    }
    let mut emitted_by_owner: BTreeMap<String, usize> = BTreeMap::new();
    let mut emitted = 0;
    let mut ambiguous = 0;
    let mut truncated = 0;
    for (file_index, file) in structure.files().iter().enumerate() {
        for call in &file.calls {
            let (Some(owner), Some(targets)) =
                (call.owner, symbols_by_name.get(call.callee.as_str()))
            else {
                continue;
            };
            if targets.len() != 1 {
                ambiguous += 1;
                continue;
            }
            let Some(from) = symbol_ids.get(&(file_index, owner as usize)) else {
                continue;
            };
            let owner_count = emitted_by_owner.entry(from.clone()).or_default();
            if *owner_count >= MAX_CALL_EDGES_PER_SYMBOL {
                truncated += 1;
                continue;
            }
            write_separator(writer, first)?;
            write_json(
                writer,
                &json!({
                    "from": from,
                    "kind": "calls",
                    "line": call.line,
                    "to": symbol_ids[&targets[0]],
                }),
            )?;
            *owner_count += 1;
            emitted += 1;
        }
    }
    Ok((emitted, ambiguous, truncated))
}

fn symbol_ids(index: &StructureIndex) -> BTreeMap<(usize, usize), String> {
    index
        .files()
        .iter()
        .enumerate()
        .flat_map(|(file_index, file)| {
            file.symbols
                .iter()
                .enumerate()
                .map(move |(symbol_index, symbol)| {
                    ((file_index, symbol_index), symbol_id(&file.path, symbol))
                })
        })
        .collect()
}

fn source_snapshot_digest(files: &[&AuditedFile], hashes: &BTreeMap<String, String>) -> String {
    let mut digest = Sha256::new();
    for file in files {
        let path = slash(&file.relative);
        let bytes = file.bytes.to_string();
        digest.update_record([
            path.as_str(),
            hashes[&path].as_str(),
            file.language.as_str(),
            file.role.as_str(),
            bytes.as_str(),
        ]);
    }
    digest.finalize()
}

fn digest_file(path: &Path) -> io::Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize())
}

fn file_id(path: &str) -> String {
    format!("file:{}", sha256_digest_bytes(path.as_bytes()))
}

fn symbol_id(path: &str, symbol: &StructureSymbol) -> String {
    let identity = format!(
        "{path}\0{}\0{}\0{}\0{}",
        symbol_kind(symbol.kind),
        symbol.name,
        symbol.span.start_byte,
        symbol.span.end_byte,
    );
    format!("symbol:{}", sha256_digest_bytes(identity.as_bytes()))
}

fn span_value(span: SourceSpan) -> Value {
    json!({
        "end_byte": span.end_byte,
        "end_column": span.end_column,
        "end_line": span.end_line,
        "start_byte": span.start_byte,
        "start_column": span.start_column,
        "start_line": span.start_line,
    })
}

fn symbol_kind(kind: SymbolKind) -> &'static str {
    SYMBOL_KIND_NAMES[kind as usize]
}

fn write_separator(writer: &mut impl Write, first: &mut bool) -> io::Result<()> {
    if *first {
        *first = false;
    } else {
        writer.write_all(b",")?;
    }
    Ok(())
}

fn write_json(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(writer, value).map_err(io::Error::other)
}

fn json_string(value: &str) -> io::Result<String> {
    serde_json::to_string(value).map_err(io::Error::other)
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

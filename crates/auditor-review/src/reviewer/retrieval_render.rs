//! The retrieval tools' answers, each a bounded document.

use std::collections::BTreeMap;

use auditor_core::sha256_digest_bytes;

use super::document::{BoundedDocument, Item, Row, Section};
use super::retrieval::{ReadSource, RetrievalHost, SearchQuery};
use super::view::{count, RepositoryView, LOCAL_KINDS};
use crate::repository_map::checked_source_path;

impl RetrievalHost<'_, '_> {
    pub(super) fn read_source(
        &self,
        request: &ReadSource,
        limit: usize,
    ) -> Result<BoundedDocument, String> {
        let view = self.view;
        let file = view
            .file_by_path(request.path.trim_start_matches("./"))
            .ok_or_else(|| format!("`{}` is not a mapped file", request.path))?;
        let mapped = view.file(file);
        if request.start_line.is_none()
            && request.end_line.is_none()
            && mapped.bytes as usize > limit
        {
            return Err(format!(
                "`{}` has {} bytes in {} lines, over the {limit}-byte result limit; request a line range",
                mapped.path, mapped.bytes, mapped.lines
            ));
        }
        let path =
            checked_source_path(&self.root, &mapped.path).map_err(|error| error.to_string())?;
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        if sha256_digest_bytes(&bytes) != mapped.sha256 {
            return Err(format!(
                "`{}` changed since the scan; it no longer matches the map",
                mapped.path
            ));
        }
        let text =
            String::from_utf8(bytes).map_err(|_| format!("`{}` is not UTF-8 text", mapped.path))?;
        let lines = text.split_inclusive('\n').collect::<Vec<_>>();
        let start = request.start_line.unwrap_or(1);
        let end = request.end_line.unwrap_or(lines.len()).min(lines.len());
        if start == 0 || start > end {
            return Err(format!(
                "lines {start}-{end} are outside `{}`, which has {} lines",
                mapped.path,
                lines.len()
            ));
        }
        let symbols = view
            .symbols_in_file(file)
            .iter()
            .copied()
            .filter(|symbol| {
                let symbol = view.symbol(*symbol);
                !LOCAL_KINDS.contains(&symbol.kind.as_str())
                    && (start..=end).contains(&symbol.span.start_line)
            })
            .map(|symbol| view.symbol_row(symbol));
        Ok(BoundedDocument::new(
            format!("Source of {}", mapped.path),
            vec![Section::new(
                "Source",
                vec![
                    Item::text(
                        "Range",
                        format!(
                            "lines {start}-{end} of {}, {}",
                            lines.len(),
                            mapped.language
                        ),
                    ),
                    Item::Source {
                        label: "Text".to_string(),
                        language: mapped.language.clone(),
                        content: lines[start - 1..end].concat(),
                        ids: vec![
                            format!("source-span:{}#L{start}-L{end}", mapped.path),
                            mapped.id.clone(),
                        ],
                    },
                    Item::rows("Symbols starting here", symbols, 20),
                ],
            )],
        ))
    }

    pub(super) fn symbol_info(&self, query: &str) -> Result<BoundedDocument, String> {
        let view = self.view;
        let candidates = self.matching_symbols(query);
        let symbol = match candidates.as_slice() {
            [] => return Err(format!("no symbol matches `{query}`; try search_symbols")),
            [symbol] => *symbol,
            _ => {
                return Ok(BoundedDocument::new(
                    format!("Symbols named {query}"),
                    vec![Section::new(
                        "Candidates",
                        vec![
                            Item::text(
                                "Ambiguous",
                                format!("{} symbols match; ask again by ID", candidates.len()),
                            ),
                            Item::rows(
                                "Matches",
                                candidates.iter().map(|symbol| view.symbol_row(*symbol)),
                                25,
                            ),
                        ],
                    )],
                ));
            }
        };
        let id = view.symbol(symbol).id.as_str();
        let calls_item = |label: &str, rows: Vec<Row>| {
            if rows.is_empty() {
                Item::text(label, "none resolved")
            } else {
                Item::rows(label, rows, 25)
            }
        };
        let entries = view
            .map
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.symbol_id.as_deref() == Some(id))
            .map(|(index, _)| view.entry_row(index));
        Ok(BoundedDocument::new(
            format!("Symbol {}", view.qualified_name(symbol)),
            vec![
                Section::new(
                    "Symbol",
                    vec![
                        Item::rows("Identity", [view.symbol_row(symbol)], 1),
                        Item::text("Details", view.symbol_details(symbol)),
                        Item::rows(
                            "Inherits from",
                            view.edge_rows(view.edges_from(id, "inherits"), |edge| &edge.to),
                            4,
                        ),
                        Item::rows(
                            "Subtypes",
                            view.edge_rows(view.edges_to(id, "inherits"), |edge| &edge.from),
                            20,
                        ),
                        Item::rows(
                            "Members",
                            view.members(symbol)
                                .into_iter()
                                .map(|child| view.symbol_row(child)),
                            30,
                        ),
                    ],
                ),
                Section::new(
                    "Calls",
                    vec![
                        calls_item(
                            "Callers",
                            view.edge_rows(view.edges_to(id, "calls"), |edge| &edge.from),
                        ),
                        calls_item(
                            "Callees",
                            view.edge_rows(view.edges_from(id, "calls"), |edge| &edge.to),
                        ),
                    ],
                ),
                Section::new(
                    "Context",
                    vec![
                        Item::rows("Entry points", entries, 10),
                        Item::rows(
                            "Findings inside",
                            view.findings_in_symbol(symbol)
                                .map(|finding| view.finding_row(finding)),
                            20,
                        ),
                    ],
                ),
            ],
        ))
    }

    /// Symbols an ID or a (dotted) name names; locals only by ID.
    fn matching_symbols(&self, query: &str) -> Vec<usize> {
        let view = self.view;
        let query = query.trim();
        if let Some(symbol) = view.symbol_by_id(query) {
            return vec![symbol];
        }
        let dotted = query.replace("::", ".");
        let name = dotted.rsplit('.').next().unwrap_or(&dotted);
        let suffix = format!(".{dotted}");
        (0..view.map.symbols.len())
            .filter(|symbol| {
                let mapped = view.symbol(*symbol);
                mapped.name == name && !LOCAL_KINDS.contains(&mapped.kind.as_str()) && {
                    let qualified = view.qualified_name(*symbol);
                    qualified == dotted || qualified.ends_with(&suffix)
                }
            })
            .collect()
    }

    pub(super) fn search_symbols(&self, request: &SearchQuery) -> Result<BoundedDocument, String> {
        let view = self.view;
        let query = request.query.trim().to_lowercase();
        if query.is_empty() {
            return Err("the query is empty".to_string());
        }
        let mut matches = (0..view.map.symbols.len())
            .filter_map(|symbol| {
                let mapped = view.symbol(symbol);
                let kind_matches = match &request.kind {
                    Some(kind) => mapped.kind == *kind,
                    None => !LOCAL_KINDS.contains(&mapped.kind.as_str()),
                };
                let name = mapped.name.to_lowercase();
                let rank = if name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else if name.contains(&query) {
                    2
                } else {
                    return None;
                };
                kind_matches.then_some((rank, symbol))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(rank, symbol)| {
            let file = view.symbol_file(*symbol);
            (
                *rank,
                !view.is_production_code(file),
                view.file(file).path.as_str(),
                view.symbol(*symbol).span.start_line,
            )
        });
        Ok(BoundedDocument::new(
            format!("Symbols matching {}", request.query.trim()),
            vec![Section::new(
                "Matches",
                vec![
                    Item::text("Count", count(matches.len(), "symbol")),
                    Item::rows(
                        "Symbols",
                        matches
                            .into_iter()
                            .map(|(_, symbol)| view.symbol_row(symbol)),
                        25,
                    ),
                ],
            )],
        ))
    }

    pub(super) fn findings_in(&self, scope: &str) -> Result<BoundedDocument, String> {
        let view = self.view;
        let scope = scope.trim();
        let (row, findings) = if let Some(symbol) = view.symbol_by_id(scope) {
            (
                view.symbol_row(symbol),
                view.findings_in_symbol(symbol).collect::<Vec<_>>(),
            )
        } else if let Some(file) = view
            .file_by_id(scope)
            .or_else(|| view.file_by_path(scope.trim_start_matches("./")))
        {
            (
                Row::new(view.file(file).path.clone(), [view.file(file).id.clone()]),
                view.ranked_findings(|finding| finding.file == Some(file))
                    .collect(),
            )
        } else if let Some(module) = view.modules.find(scope) {
            (
                Row::new(
                    format!("module {}", view.modules.name(module)),
                    [view.modules.id(module)],
                ),
                view.ranked_findings(|finding| {
                    finding
                        .file
                        .is_some_and(|file| view.modules.of_file(file) == module)
                })
                .collect(),
            )
        } else {
            return Err(format!("`{scope}` is not a mapped file, module, or symbol"));
        };
        Ok(BoundedDocument::new(
            "Findings",
            vec![Section::new(
                "Findings",
                vec![
                    Item::rows("Scope", [row], 1),
                    Item::text("Count", count(findings.len(), "finding")),
                    Item::rows(
                        "Findings",
                        findings
                            .into_iter()
                            .map(|finding| view.finding_row(finding)),
                        40,
                    ),
                ],
            )],
        ))
    }

    pub(super) fn module_overview(&self, query: &str) -> Result<BoundedDocument, String> {
        let view = self.view;
        let module = view
            .modules
            .find(query.trim())
            .ok_or_else(|| format!("`{}` is not a module of the orientation pack", query.trim()))?;
        let cycles = (0..view.module_cycles.len())
            .filter(|cycle| view.module_cycles[*cycle].contains(&module))
            .map(|cycle| view.cycle_row(cycle));
        let top = view.ranked_findings(|finding| {
            finding
                .file
                .is_some_and(|file| view.modules.of_file(file) == module)
        });
        let entries = view
            .map
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                view.file_by_id(&entry.file_id)
                    .is_some_and(|file| view.modules.of_file(file) == module)
            })
            .map(|(index, _)| view.entry_row(index));
        Ok(BoundedDocument::new(
            format!("Module {}", view.modules.name(module)),
            vec![
                Section::new(
                    "Module",
                    vec![
                        Item::rows("Identity", [view.module_row(module)], 1),
                        Item::text("Source roles", module_roles(view, module)),
                        Item::rows("Files", module_file_rows(view, module), 30),
                    ],
                ),
                Section::new(
                    "Dependencies",
                    vec![
                        Item::rows("Imports", view.module_dependency_rows(module, true), 15),
                        Item::rows(
                            "Imported by",
                            view.module_dependency_rows(module, false),
                            15,
                        ),
                        Item::rows("Cycles", cycles, 5),
                    ],
                ),
                Section::new(
                    "Findings",
                    vec![
                        Item::rows(
                            "By detector",
                            view.detector_rows(view.module_findings(module)),
                            20,
                        ),
                        Item::rows(
                            "Most severe",
                            top.map(|finding| view.finding_row(finding)),
                            15,
                        ),
                        Item::rows("Entry points", entries, 15),
                    ],
                ),
            ],
        ))
    }
}

/// `30 production, 5 test`: a module's files by source role.
fn module_roles(view: &RepositoryView<'_>, module: usize) -> String {
    let mut roles: BTreeMap<&str, usize> = BTreeMap::new();
    for file in view.modules.members(module) {
        *roles
            .entry(view.file(*file).source_role.as_str())
            .or_default() += 1;
    }
    roles
        .iter()
        .map(|(role, number)| format!("{number} {role}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A module's files, the largest first.
fn module_file_rows(view: &RepositoryView<'_>, module: usize) -> Vec<Row> {
    let mut files = view.modules.members(module).to_vec();
    files.sort_by_key(|file| (std::cmp::Reverse(view.file(*file).lines), *file));
    files
        .into_iter()
        .map(|file| {
            Row::new(
                format!(
                    "{}: {}, {}",
                    view.file(file).path,
                    count(view.file(file).lines, "line"),
                    count(view.findings_in_file(file).len(), "finding")
                ),
                [view.file(file).id.clone()],
            )
        })
        .collect()
}

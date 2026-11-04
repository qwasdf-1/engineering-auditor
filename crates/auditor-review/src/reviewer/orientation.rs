//! The orientation pack: what a reviewer is given before its first
//! question. Shape, structure, hotspots, finding distribution, and coverage,
//! built deterministically from the report and the repository map.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::document::{BoundedDocument, Item, Row, Section};
use super::view::{count, severity_rank, RepositoryView};
use crate::{ModelProjectionAudit, ReviewBrief};

pub const TARGET_ORIENTATION_BYTES: usize = 24 * 1024;
pub const MINIMUM_ORIENTATION_BYTES: usize = 4 * 1024;

/// Entry kinds the pack lists per program, in this order; HTTP routes are
/// counted per file instead.
const ENTRY_KINDS: [&str; 4] = ["main", "cli_command", "process_run", "signal_handler"];

/// Detectors whose findings describe module structure.
const STRUCTURE_DETECTORS: [&str; 2] = ["architectural_layering", "package_coupling_metrics"];

const CHURN_DETECTOR: &str = "churn_complexity_hotspot";

/// Finding IDs one cluster row lists.
const CLUSTER_FINDINGS: usize = 6;

#[derive(Clone, Debug)]
pub struct OrientationPack {
    pub markdown: String,
    pub audit: ModelProjectionAudit,
    /// Every evidence ID the pack shows, and so may be cited.
    pub evidence_ids: BTreeSet<String>,
    /// Whether the budget cut any list.
    pub truncated: bool,
}

impl RepositoryView<'_> {
    /// The pack within [`TARGET_ORIENTATION_BYTES`], or the brief's
    /// smaller context budget.
    pub fn orientation_pack(&self, brief: Option<&ReviewBrief>) -> Result<OrientationPack, String> {
        let budget = brief
            .and_then(|brief| brief.budgets.max_context_bytes)
            .map_or(TARGET_ORIENTATION_BYTES, |bytes| {
                bytes.min(TARGET_ORIENTATION_BYTES)
            });
        if budget < MINIMUM_ORIENTATION_BYTES {
            return Err(format!(
                "the orientation pack needs at least {MINIMUM_ORIENTATION_BYTES} bytes; the brief allows {budget}"
            ));
        }
        let rendered = BoundedDocument::new(
            "Repository orientation",
            vec![
                shape(self),
                structure(self),
                hotspots(self),
                findings(self),
                coverage(self),
            ],
        )
        .render(budget, &self.source_digest)?;
        Ok(OrientationPack {
            markdown: rendered.markdown,
            audit: rendered.audit,
            evidence_ids: rendered.evidence_ids,
            truncated: rendered.truncated,
        })
    }
}

#[derive(Default)]
struct Tally {
    files: usize,
    lines: usize,
}

fn shape(view: &RepositoryView<'_>) -> Section {
    let mut languages: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut roles: BTreeMap<&str, Tally> = BTreeMap::new();
    for file in &view.map.files {
        for tally in [
            languages.entry(file.language.as_str()).or_default(),
            roles.entry(file.source_role.as_str()).or_default(),
        ] {
            tally.files += 1;
            tally.lines += file.lines;
        }
    }
    let tally_rows = |prefix: &str, tallies: BTreeMap<&str, Tally>| {
        let mut tallies = tallies.into_iter().collect::<Vec<_>>();
        tallies.sort_by(|left, right| right.1.lines.cmp(&left.1.lines).then(left.0.cmp(right.0)));
        tallies
            .into_iter()
            .map(|(name, tally)| {
                Row::new(
                    format!(
                        "{name}: {}, {}",
                        count(tally.files, "file"),
                        count(tally.lines, "line")
                    ),
                    [format!("{prefix}:{name}")],
                )
            })
            .collect::<Vec<_>>()
    };
    let total_lines = view.map.files.iter().map(|file| file.lines).sum::<usize>();
    let mut modules = (0..view.modules.len()).collect::<Vec<_>>();
    modules.sort_by_key(|module| (std::cmp::Reverse(view.module_lines(*module)), *module));
    let module_rows = modules.into_iter().map(|module| view.module_row(module));
    let mut items = vec![
        Item::text(
            "Totals",
            format!(
                "{}, {}, {}, {}",
                count(view.map.files.len(), "file"),
                count(total_lines, "line"),
                count(view.modules.len(), "module"),
                count(view.map.symbols.len(), "symbol")
            ),
        ),
        Item::rows("Languages", tally_rows("language", languages), 12),
        Item::rows("Source roles", tally_rows("role", roles), 9),
        Item::rows("Modules by size", module_rows, 20),
    ];
    items.extend(entry_items(view));
    Section::new("Shape", items)
}

/// Programs and commands, one row per program, then HTTP routes counted
/// per file; entries in test, fixture, and example files are left out.
fn entry_items(view: &RepositoryView<'_>) -> [Item; 3] {
    let mut programs: BTreeMap<(usize, &str), Vec<usize>> = BTreeMap::new();
    let mut routes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, entry) in view.map.entry_points.iter().enumerate() {
        let Some(file) = view.file_by_id(&entry.file_id) else {
            continue;
        };
        if !view.is_production_code(file) {
            continue;
        }
        *kinds.entry(entry.kind.as_str()).or_default() += 1;
        let path = view.file(file).path.as_str();
        match ENTRY_KINDS.iter().position(|kind| *kind == entry.kind) {
            Some(kind) => programs.entry((kind, path)).or_default().push(index),
            None => *routes.entry(path).or_default() += 1,
        }
    }
    let program_rows = programs.into_iter().map(|((kind, path), entries)| {
        let entries = entries
            .iter()
            .map(|index| (*index, &view.map.entry_points[*index]))
            .collect::<Vec<_>>();
        let details = entries
            .iter()
            .filter(|(_, entry)| entry.symbol_id.is_none())
            .map(|(_, entry)| entry.detail.as_str())
            .collect::<Vec<_>>();
        let symbols = entries
            .iter()
            .filter_map(|(_, entry)| view.symbol_by_id(entry.symbol_id.as_deref()?))
            .collect::<Vec<_>>();
        let mut text = format!("{} at {path}:{}", ENTRY_KINDS[kind], entries[0].1.line);
        if !details.is_empty() {
            text.push_str(&format!(" ({})", details.join("; ")));
        }
        if !symbols.is_empty() {
            let names = symbols
                .iter()
                .map(|symbol| format!("`{}`", view.qualified_name(*symbol)))
                .collect::<Vec<_>>();
            text.push_str(&format!(", reaching {}", names.join(", ")));
        }
        let ids = entries
            .iter()
            .map(|(index, _)| format!("entry:{index}"))
            .chain(symbols.iter().map(|symbol| view.symbol(*symbol).id.clone()));
        Row::new(text, ids)
    });
    let mut routes = routes.into_iter().collect::<Vec<_>>();
    routes.sort_by_key(|(path, count)| (std::cmp::Reverse(*count), *path));
    let route_rows = routes.into_iter().filter_map(|(path, routes)| {
        let file = view.file(view.file_by_path(path)?);
        Some(Row::new(
            format!("{path}: {}", count(routes, "route")),
            [file.id.clone()],
        ))
    });
    let summary = if kinds.is_empty() {
        "none resolved in production code".to_string()
    } else {
        kinds
            .iter()
            .map(|(kind, number)| format!("{number} {kind}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    [
        Item::text("Entry points", summary),
        Item::rows("Programs and commands", program_rows, 12),
        Item::rows("HTTP routes by file", route_rows, 10),
    ]
}

fn structure(view: &RepositoryView<'_>) -> Section {
    let mut dependencies = view.module_imports.iter().collect::<Vec<_>>();
    dependencies.sort_by_key(|(pair, count)| (std::cmp::Reverse(**count), **pair));
    let dependency_rows = dependencies
        .into_iter()
        .map(|((from, to), count)| view.dependency_row(*from, *to, *count));
    let cycle_rows = (0..view.module_cycles.len()).map(|cycle| view.cycle_row(cycle));
    let structure_rows = view
        .ranked_findings(|finding| STRUCTURE_DETECTORS.contains(&finding.detector.as_str()))
        .map(|finding| view.finding_row(finding));
    let mut items = vec![];
    if view.module_imports.is_empty() {
        items.push(Item::text(
            "Module dependencies",
            "none: the map resolves no import between modules",
        ));
    } else {
        items.push(Item::rows("Module dependencies", dependency_rows, 16));
    }
    items.push(Item::rows("Module cycles", cycle_rows, 6));
    items.push(Item::rows("Structure findings", structure_rows, 10));
    Section::new("Structure", items)
}

fn hotspots(view: &RepositoryView<'_>) -> Section {
    let mut churn = view
        .findings
        .iter()
        .filter(|finding| finding.detector == CHURN_DETECTOR)
        .map(|finding| {
            let number = |key: &str| finding.evidence[key].as_u64().unwrap_or(0);
            (
                finding,
                number("sum_ccx"),
                number("commit_count"),
                number("loc_delta"),
            )
        })
        .collect::<Vec<_>>();
    churn.sort_by_key(|(finding, ccx, commits, _)| {
        (std::cmp::Reverse(ccx * commits), finding.location())
    });
    let churn_rows = churn.into_iter().map(|(finding, ccx, commits, delta)| {
        let mut ids = vec![finding.evidence_id()];
        ids.extend(finding.file.map(|file| view.file(file).id.clone()));
        Row::new(
            format!(
                "{}: complexity {ccx}, {commits} commits, +{delta} lines",
                finding.path.as_deref().unwrap_or("?")
            ),
            ids,
        )
    });
    let code_files = (0..view.map.files.len())
        .filter(|file| view.is_production_code(*file))
        .collect::<Vec<_>>();
    let file_row = |file: usize, detail: String| {
        Row::new(
            format!("{}: {detail}", view.file(file).path),
            [view.file(file).id.clone()],
        )
    };
    let mut largest = code_files.clone();
    largest.sort_by_key(|file| (std::cmp::Reverse(view.file(*file).lines), *file));
    let largest_rows = largest.into_iter().map(|file| {
        file_row(
            file,
            format!(
                "{}, {}",
                count(view.file(file).lines, "line"),
                count(view.findings_in_file(file).len(), "finding")
            ),
        )
    });
    let mut coupled = code_files
        .iter()
        .map(|file| (*file, view.import_degree(*file)))
        .filter(|(_, (fan_in, fan_out))| fan_in + fan_out > 0)
        .collect::<Vec<_>>();
    coupled.sort_by_key(|(file, (fan_in, fan_out))| (std::cmp::Reverse(fan_in + fan_out), *file));
    let coupled_rows = coupled.into_iter().map(|(file, (fan_in, fan_out))| {
        file_row(
            file,
            format!("imported by {}, imports {fan_out}", count(fan_in, "file")),
        )
    });
    let mut dense = (0..view.map.files.len())
        .filter(|file| !view.findings_in_file(*file).is_empty())
        .collect::<Vec<_>>();
    dense.sort_by_key(|file| (std::cmp::Reverse(view.findings_in_file(*file).len()), *file));
    let dense_rows = dense.into_iter().map(|file| {
        let detectors = view
            .findings_in_file(file)
            .iter()
            .map(|finding| view.findings[*finding].detector.as_str())
            .collect::<BTreeSet<_>>();
        file_row(
            file,
            format!(
                "{} from {}",
                count(view.findings_in_file(file).len(), "finding"),
                count(detectors.len(), "detector")
            ),
        )
    });
    let churn_item = if view
        .findings
        .iter()
        .any(|finding| finding.detector == CHURN_DETECTOR)
    {
        Item::rows("Complexity × churn", churn_rows, 10)
    } else {
        Item::text(
            "Complexity × churn",
            "no history-weighted hotspot in the report (see Coverage)",
        )
    };
    Section::new(
        "Hotspots",
        vec![
            churn_item,
            Item::rows("Largest production files", largest_rows, 10),
            Item::rows("Most-coupled production files", coupled_rows, 10),
            Item::rows("Files with the most findings", dense_rows, 10),
        ],
    )
}

fn findings(view: &RepositoryView<'_>) -> Section {
    let mut severities: BTreeMap<(u8, &str), usize> = BTreeMap::new();
    let mut layers: BTreeMap<&str, usize> = BTreeMap::new();
    let mut symbols: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, finding) in view.findings.iter().enumerate() {
        *severities
            .entry((severity_rank(&finding.severity), finding.severity.as_str()))
            .or_default() += 1;
        *layers.entry(finding.layer.as_str()).or_default() += 1;
        if let Some(symbol) = finding.symbol {
            symbols.entry(symbol).or_default().push(index);
        }
    }
    let joined = |counts: Vec<String>| {
        if counts.is_empty() {
            "none".to_string()
        } else {
            counts.join(", ")
        }
    };
    let mut clusters = symbols
        .into_iter()
        .filter(|(_, findings)| findings.len() > 1)
        .collect::<Vec<_>>();
    clusters.sort_by_key(|(symbol, findings)| (std::cmp::Reverse(findings.len()), *symbol));
    let cluster_rows = clusters.into_iter().map(|(symbol, mut findings)| {
        findings.sort_by_key(|finding| {
            let finding = &view.findings[*finding];
            (
                severity_rank(&finding.severity),
                finding.line,
                finding.id.clone(),
            )
        });
        let detectors = findings
            .iter()
            .map(|finding| view.findings[*finding].detector.as_str())
            .collect::<BTreeSet<_>>();
        let mut ids = vec![view.symbol(symbol).id.clone()];
        ids.extend(
            findings
                .iter()
                .take(CLUSTER_FINDINGS)
                .map(|finding| view.findings[*finding].evidence_id()),
        );
        Row::new(
            format!(
                "`{}` ({}, {}): {} from {}",
                view.qualified_name(symbol),
                view.symbol(symbol).kind,
                view.symbol_location(symbol),
                count(findings.len(), "finding"),
                count(detectors.len(), "detector")
            ),
            ids,
        )
    });
    Section::new(
        "Findings",
        vec![
            Item::text(
                "Totals",
                format!(
                    "{}: {}",
                    count(view.findings.len(), "finding"),
                    joined(
                        severities
                            .iter()
                            .map(|((_, severity), count)| format!("{count} {severity}"))
                            .collect()
                    )
                ),
            ),
            Item::text(
                "By layer",
                joined(
                    layers
                        .iter()
                        .map(|(layer, count)| format!("{layer} {count}"))
                        .collect(),
                ),
            ),
            Item::rows(
                "By detector",
                view.detector_rows(0..view.findings.len()),
                20,
            ),
            Item::rows("Densest symbols", cluster_rows, 10),
        ],
    )
}

fn coverage(view: &RepositoryView<'_>) -> Section {
    let with_status = |status: &str| {
        view.detectors
            .iter()
            .filter(|run| run.status == status)
            .count()
    };
    let incomplete = view
        .detectors
        .iter()
        .filter(|run| !run.missing_inputs.is_empty() || run.failure_reason.is_some())
        .map(|run| {
            let mut reason = run.status.clone();
            if !run.missing_inputs.is_empty() {
                reason.push_str(&format!(", missing {}", run.missing_inputs.join(", ")));
            }
            if let Some(failure) = &run.failure_reason {
                reason.push_str(&format!(": {failure}"));
            }
            Row::new(
                format!("{} ({}): {reason}", run.id, run.layer),
                [format!("detector:{}", run.id)],
            )
        });
    let coverage = &view.report["analysis_coverage"];
    let number = |value: &Value| value.as_u64().unwrap_or(0);
    let completeness = &view.map.completeness;
    let tests = view
        .map
        .files
        .iter()
        .filter(|file| file.source_role == "test")
        .count();
    let mut unknowns = vec![
        "History: no history digest was supplied, so change frequency, growth, and co-change are unknown; only the report's complexity × churn findings carry history.".to_string(),
        "Runtime: no measurements; cost judgments rest on structure alone.".to_string(),
    ];
    unknowns.push(if tests == 0 {
        "Tests: the repository has no test files.".to_string()
    } else {
        format!(
            "Tests: {}, but no coverage or results, so which code they exercise is unknown.",
            count(tests, "test file")
        )
    });
    Section::new(
        "Coverage",
        vec![
            Item::text(
                "Detectors",
                format!(
                    "{} of {} executed; {} support none of the repository's languages; {} not applicable",
                    with_status("executed"),
                    view.detectors.len(),
                    with_status("unsupported"),
                    with_status("not_applicable")
                ),
            ),
            Item::rows("Missing inputs or failed", incomplete, 12),
            Item::text(
                "Analysis",
                format!(
                    "{} syntax-recovered files; evidence graph {}; map: {} unresolved calls, {} unresolved imports, {} dynamic imports, {} call edges truncated",
                    number(&coverage["syntax_recovered_files"]),
                    if coverage["graph_complete"].as_bool() == Some(true) { "complete" } else { "incomplete" },
                    completeness.unresolved_calls,
                    completeness.unresolved_imports,
                    completeness.dynamic_imports,
                    completeness.truncated_call_edges,
                ),
            ),
            Item::notes("Unknown", unknowns),
        ],
    )
}

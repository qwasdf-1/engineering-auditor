//! The rankings the orientation pack lists and the questions pick their
//! subjects from: hotspot files by each signal, and finding clusters.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::view::{severity_rank, RepositoryView};
use crate::history::FileHistory;

/// The detector whose findings weight complexity by churn.
pub(super) const CHURN_DETECTOR: &str = "churn_complexity_hotspot";

/// Detectors whose findings describe module structure.
pub(super) const STRUCTURE_DETECTORS: [&str; 5] = [
    "architectural_layering",
    "package_coupling_metrics",
    "god_module",
    "repo_graph_import_cycle",
    "repo_graph_high_fanout_source_file",
];

pub(super) struct ChurnHotspot {
    pub(super) finding: usize,
    pub(super) complexity: u64,
    pub(super) commits: u64,
    pub(super) growth: u64,
}

impl<'a> RepositoryView<'a> {
    /// Churn findings, complexity × commits first.
    pub(super) fn churn_hotspots(&self) -> Vec<ChurnHotspot> {
        let mut hotspots = self
            .findings
            .iter()
            .enumerate()
            .filter(|(_, finding)| finding.detector == CHURN_DETECTOR)
            .map(|(index, finding)| {
                let number = |key: &str| finding.evidence[key].as_u64().unwrap_or(0);
                ChurnHotspot {
                    finding: index,
                    complexity: number("sum_ccx"),
                    commits: number("commit_count"),
                    growth: number("loc_delta"),
                }
            })
            .collect::<Vec<_>>();
        hotspots.sort_by_key(|hotspot| {
            (
                Reverse(hotspot.complexity * hotspot.commits),
                self.findings[hotspot.finding].location(),
            )
        });
        hotspots
    }

    pub(super) fn production_files(&self) -> Vec<usize> {
        (0..self.map.files.len())
            .filter(|file| self.is_production_code(*file))
            .collect()
    }

    /// Production files, longest first.
    pub(super) fn largest_files(&self) -> Vec<usize> {
        let mut files = self.production_files();
        files.sort_by_key(|file| (Reverse(self.file(*file).lines), *file));
        files
    }

    /// Production files with any import in or out, by fan-in plus fan-out.
    pub(super) fn most_coupled_files(&self) -> Vec<(usize, (usize, usize))> {
        let mut files = self
            .production_files()
            .into_iter()
            .map(|file| (file, self.import_degree(file)))
            .filter(|(_, (fan_in, fan_out))| fan_in + fan_out > 0)
            .collect::<Vec<_>>();
        files.sort_by_key(|(file, (fan_in, fan_out))| (Reverse(fan_in + fan_out), *file));
        files
    }

    /// Files with findings, the most first.
    pub(super) fn densest_files(&self) -> Vec<usize> {
        let mut files = (0..self.map.files.len())
            .filter(|file| !self.findings_in_file(*file).is_empty())
            .collect::<Vec<_>>();
        files.sort_by_key(|file| (Reverse(self.findings_in_file(*file).len()), *file));
        files
    }

    /// With a history digest: production files by commits × lines.
    pub(super) fn most_changed_files(&self) -> Vec<(usize, &'a FileHistory)> {
        let mut files = self
            .production_files()
            .into_iter()
            .filter_map(|file| Some((file, self.file_history(file)?)))
            .collect::<Vec<_>>();
        files.sort_by_key(|(file, history)| {
            (Reverse(history.commits * self.file(*file).lines), *file)
        });
        files
    }

    /// Production file pairs that change together with no import between
    /// them, by shared commits.
    pub(super) fn unlinked_co_changes(&self) -> Vec<((usize, usize), usize)> {
        let mut pairs = BTreeMap::new();
        for (file, history) in self.most_changed_files() {
            for partner in &history.co_changes {
                let Some(other) = self
                    .file_by_path(&partner.path)
                    .filter(|other| self.is_production_code(*other))
                else {
                    continue;
                };
                if self.import_links(file, other) == (false, false) {
                    pairs.insert((file.min(other), file.max(other)), partner.commits);
                }
            }
        }
        let mut pairs = pairs.into_iter().collect::<Vec<_>>();
        pairs.sort_by_key(|(pair, shared)| (Reverse(*shared), *pair));
        pairs
    }

    /// Symbols holding more than one finding, the most first; each
    /// cluster's findings most severe first, then by line.
    pub(super) fn finding_clusters(&self) -> Vec<(usize, Vec<usize>)> {
        let mut symbols: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, finding) in self.findings.iter().enumerate() {
            if let Some(symbol) = finding.symbol {
                symbols.entry(symbol).or_default().push(index);
            }
        }
        let mut clusters = symbols
            .into_iter()
            .filter(|(_, findings)| findings.len() > 1)
            .collect::<Vec<_>>();
        for (_, findings) in &mut clusters {
            findings.sort_by_key(|finding| {
                let finding = &self.findings[*finding];
                (
                    severity_rank(&finding.severity),
                    finding.line,
                    finding.id.clone(),
                )
            });
        }
        clusters.sort_by_key(|(symbol, findings)| (Reverse(findings.len()), *symbol));
        clusters
    }
}

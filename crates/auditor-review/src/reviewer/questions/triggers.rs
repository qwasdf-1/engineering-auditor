//! Triggers: which subjects each kind of question asks about, chosen from
//! the same rankings the orientation pack shows.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use super::{Draft, QuestionSubject};
use crate::reviewer::rankings::{CHURN_DETECTOR, STRUCTURE_DETECTORS};
use crate::reviewer::view::{count, ReportFinding, RepositoryView};

/// Findings one cluster question lists; its scope is the ones it lists.
const CLUSTER_FINDINGS: usize = 16;

/// The top production files of every hotspot ranking, taken in turn
/// (first of each, then second of each) until `limit` distinct files.
pub(super) fn hotspots(view: &RepositoryView<'_>, limit: usize) -> Vec<Draft> {
    let churn = view.churn_hotspots();
    let rankings: [Vec<usize>; 5] = [
        churn
            .iter()
            .filter_map(|hotspot| view.findings[hotspot.finding].file)
            .filter(|file| view.is_production_code(*file))
            .collect(),
        view.most_changed_files()
            .into_iter()
            .map(|(file, _)| file)
            .collect(),
        view.largest_files(),
        view.most_coupled_files()
            .into_iter()
            .map(|(file, _)| file)
            .collect(),
        view.densest_files()
            .into_iter()
            .filter(|file| view.is_production_code(*file))
            .collect(),
    ];
    let depth = rankings.iter().map(Vec::len).max().unwrap_or(0);
    let mut chosen = Vec::new();
    let mut seen = BTreeSet::new();
    for position in 0..depth {
        for file in rankings.iter().filter_map(|ranking| ranking.get(position)) {
            if chosen.len() < limit && seen.insert(*file) {
                chosen.push(*file);
            }
        }
    }
    if chosen.is_empty() {
        return Vec::new();
    }
    let subjects = chosen
        .iter()
        .map(|file| {
            let mapped = view.file(*file);
            let (fan_in, fan_out) = view.import_degree(*file);
            let mut details = vec![
                count(mapped.lines, "line"),
                count(view.findings_in_file(*file).len(), "finding"),
                format!("imported by {}, imports {fan_out}", count(fan_in, "file")),
            ];
            let mut ids = vec![mapped.id.clone()];
            if let Some(hotspot) = churn
                .iter()
                .find(|hotspot| view.findings[hotspot.finding].file == Some(*file))
            {
                details.push(format!(
                    "complexity {} over {}",
                    hotspot.complexity,
                    count(hotspot.commits as usize, "commit")
                ));
                ids.push(view.findings[hotspot.finding].evidence_id());
            }
            if let Some(history) = view.file_history(*file) {
                details.push(format!(
                    "{} in the history window",
                    count(history.commits, "commit")
                ));
                ids.push(format!("history:{}", mapped.path));
            }
            QuestionSubject {
                id: mapped.id.clone(),
                summary: format!("{}: {}", mapped.path, details.join(", ")),
                evidence_ids: ids,
            }
        })
        .collect();
    let finding_ids = chosen
        .iter()
        .flat_map(|file| view.findings_in_file(*file))
        .map(|finding| view.findings[*finding].evidence_id())
        .collect();
    vec![Draft {
        subjects,
        finding_ids: sorted(finding_ids),
        ..Draft::default()
    }]
}

/// Symbols whose findings come from two or more detectors, those spanning
/// the most layers first, then the largest; one question each.
pub(super) fn clusters(view: &RepositoryView<'_>, limit: usize) -> Vec<Draft> {
    let mut clusters = view
        .finding_clusters()
        .into_iter()
        .filter_map(|(symbol, findings)| {
            let findings = findings
                .into_iter()
                .filter(|finding| view.findings[*finding].detector != CHURN_DETECTOR)
                .collect::<Vec<_>>();
            let distinct = |key: fn(&ReportFinding) -> &str| {
                findings
                    .iter()
                    .map(|finding| key(&view.findings[*finding]))
                    .collect::<BTreeSet<_>>()
                    .len()
            };
            let detectors = distinct(|finding| finding.detector.as_str());
            let layers = distinct(|finding| finding.layer.as_str());
            (detectors > 1).then_some((symbol, findings, detectors, layers))
        })
        .collect::<Vec<_>>();
    clusters.sort_by_key(|(symbol, findings, _, layers)| {
        (Reverse(*layers), Reverse(findings.len()), *symbol)
    });
    clusters
        .into_iter()
        .take(limit)
        .map(|(symbol, findings, detectors, layers)| {
            let row = view.symbol_row(symbol);
            let shown = &findings[..findings.len().min(CLUSTER_FINDINGS)];
            Draft {
                subjects: vec![QuestionSubject {
                    id: row.ids[0].clone(),
                    summary: format!(
                        "{}: {} from {} in {}",
                        row.text,
                        count(findings.len(), "finding"),
                        count(detectors, "detector"),
                        count(layers, "layer")
                    ),
                    evidence_ids: row.ids,
                }],
                omitted_subjects: 0,
                omitted_findings: findings.len() - shown.len(),
                finding_rows: shown
                    .iter()
                    .map(|finding| view.finding_row(*finding))
                    .collect(),
                finding_ids: shown
                    .iter()
                    .map(|finding| view.findings[*finding].evidence_id())
                    .collect(),
            }
        })
        .collect()
}

/// Module cycles, then structure findings taken a detector at a time (each
/// detector's most severe first), so one prolific detector cannot crowd
/// out the rest, up to `limit`; asked when there is at least one.
pub(super) fn structure(view: &RepositoryView<'_>, limit: usize) -> Vec<Draft> {
    let mut rows = (0..view.module_cycles.len())
        .map(|cycle| view.cycle_row(cycle))
        .collect::<Vec<_>>();
    let mut by_detector = BTreeMap::<&str, Vec<usize>>::new();
    for finding in
        view.ranked_findings(|finding| STRUCTURE_DETECTORS.contains(&finding.detector.as_str()))
    {
        by_detector
            .entry(view.findings[finding].detector.as_str())
            .or_default()
            .push(finding);
    }
    let depth = by_detector.values().map(Vec::len).max().unwrap_or(0);
    rows.extend(
        (0..depth)
            .flat_map(|position| {
                by_detector
                    .values()
                    .filter_map(move |findings| findings.get(position))
            })
            .map(|finding| view.finding_row(*finding)),
    );
    if rows.is_empty() {
        return Vec::new();
    }
    let omitted = rows.len().saturating_sub(limit);
    rows.truncate(limit);
    let subjects = rows
        .into_iter()
        .map(QuestionSubject::from_row)
        .collect::<Vec<_>>();
    let finding_ids = subjects
        .iter()
        .filter(|subject| subject.id.starts_with("finding:"))
        .map(|subject| subject.id.clone())
        .collect();
    vec![Draft {
        subjects,
        omitted_subjects: omitted,
        finding_ids,
        ..Draft::default()
    }]
}

/// The plan: no subjects of its own, every finding in scope.
pub(super) fn plan(view: &RepositoryView<'_>) -> Draft {
    Draft {
        finding_ids: sorted(
            view.findings
                .iter()
                .map(|finding| finding.evidence_id())
                .collect(),
        ),
        ..Draft::default()
    }
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

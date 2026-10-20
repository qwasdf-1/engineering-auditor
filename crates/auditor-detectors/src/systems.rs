//! Layer 4 finding constructors over typed systems facts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use auditor_core::{Confidence, Finding, Severity};

use crate::finding_via::through_calls;
use crate::systems_facts::{EffectKind, SystemEffect, SystemsIndex};
use crate::AuditedFile;

pub(crate) fn loop_artifact_rewrite_findings(index: &SystemsIndex) -> Vec<Finding> {
    let mut groups = BTreeMap::<(String, String, usize), Vec<&SystemEffect>>::new();
    for effect in index.effects().iter().filter(|effect| {
        matches!(
            effect.kind,
            EffectKind::ArtifactRead | EffectKind::ArtifactWrite
        )
    }) {
        if let Some(loop_line) = effect.loop_line {
            groups
                .entry((effect.path.clone(), effect.function.clone(), loop_line))
                .or_default()
                .push(effect);
        }
    }
    groups
        .into_iter()
        .filter_map(|((path, function, loop_line), effects)| {
            let (read, write) = artifact_rewrite_pair(&effects)?;
            let finding = through_calls(
                grouped_finding(
                    "loop_artifact_rewrite",
                    "Loop rewrites whole artifact",
                    format!(
                        "Function `{function}` performs whole-artifact read and write effects inside one loop."
                    ),
                    "Use incremental updates or move whole-artifact I/O outside the loop.",
                    (path, Some(loop_line)),
                ),
                write.via.as_deref().or(read.via.as_deref()),
            );
            Some(
                finding
            .with_evidence("function", function)
            .with_evidence("loop_line", loop_line)
            .with_identity("read_artifact", read.artifact.clone().unwrap_or_default())
            .with_identity("write_artifact", write.artifact.clone().unwrap_or_default())
            .with_evidence("read_effect", EffectKind::ArtifactRead.as_str())
            .with_evidence("write_effect", EffectKind::ArtifactWrite.as_str())
            .with_evidence("read_artifact", read.artifact.clone().unwrap_or_default())
            .with_evidence("write_artifact", write.artifact.clone().unwrap_or_default())
            .with_evidence(
                "value_lineage",
                read.value_lineage.clone().unwrap_or_default(),
            )
            .with_evidence("write_artifact_loop_variant", false),
            )
        })
        .collect()
}

fn artifact_rewrite_pair<'a>(
    effects: &'a [&'a SystemEffect],
) -> Option<(&'a SystemEffect, &'a SystemEffect)> {
    effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == EffectKind::ArtifactRead)
        .find_map(|read| {
            let lineage = read.value_lineage.as_deref()?;
            effects
                .iter()
                .copied()
                .filter(|write| write.kind == EffectKind::ArtifactWrite)
                .find(|write| {
                    read.line <= write.line
                        && write.artifact.is_some()
                        && !write.loop_variant
                        && write.value_lineage.as_deref() == Some(lineage)
                })
                .map(|write| (read, write))
        })
}

pub(crate) fn latest_only_unbounded_cache_findings(index: &SystemsIndex) -> Vec<Finding> {
    let groups = group_effects(index, |effect| {
        matches!(
            effect.kind,
            EffectKind::CacheInsert | EffectKind::CacheLatestRead | EffectKind::CacheBound
        )
        .then(|| {
            effect.subject.as_ref().map(|subject| {
                (
                    effect.path.clone(),
                    effect.function.clone(),
                    subject.clone(),
                )
            })
        })
        .flatten()
    });
    groups
        .into_iter()
        .filter(|(_, kinds)| {
            kinds.contains(&EffectKind::CacheInsert)
                && kinds.contains(&EffectKind::CacheLatestRead)
                && !kinds.contains(&EffectKind::CacheBound)
        })
        .map(|((path, function, collection), _)| {
            grouped_finding(
                "latest_only_unbounded_cache",
                "Cache retains history but consumers select only latest",
                format!(
                    "Function `{function}` grows `{collection}` while consuming only its current entry."
                ),
                "Retain one current entry, add an explicit bound, or expose real history consumers.",
                (path, None),
            )
            .with_identity("function", function.clone())
            .with_identity("collection", collection.clone())
            .with_evidence("function", function)
            .with_evidence("collection", collection)
            .with_evidence("insert_effect", EffectKind::CacheInsert.as_str())
            .with_evidence("latest_effect", EffectKind::CacheLatestRead.as_str())
            .with_evidence("bounded", false)
        })
        .collect()
}

pub(crate) fn repeated_parse_scan_serialize_findings(index: &SystemsIndex) -> Vec<Finding> {
    let groups = group_effects(index, |effect| {
        matches!(
            effect.kind,
            EffectKind::Parse | EffectKind::Scan | EffectKind::Serialize
        )
        .then(|| {
            Some((
                effect.path.clone(),
                effect.function.clone(),
                effect.loop_line?,
                effect.subject.clone()?,
            ))
        })
        .flatten()
    });
    groups
        .into_iter()
        .filter(|(_, kinds)| {
            [EffectKind::Parse, EffectKind::Scan, EffectKind::Serialize]
                .iter()
                .all(|kind| kinds.contains(kind))
        })
        .map(|((path, function, loop_line, value), _)| {
            grouped_finding(
                "repeated_parse_scan_serialize",
                "Repeated parse/scan/serialize churn",
                format!(
                    "Function `{function}` repeatedly parses, scans, and serializes `{value}` in one loop."
                ),
                "Parse once, retain the structured representation, and serialize once after mutation.",
                (path, Some(loop_line)),
            )
            .with_identity("value", value.clone())
            .with_evidence("function", function)
            .with_evidence("value_lineage", value)
            .with_evidence("loop_line", loop_line)
            .with_evidence("effects", vec!["parse", "scan", "serialize"])
        })
        .collect()
}

pub(crate) fn blocking_io_in_async_context_findings(index: &SystemsIndex) -> Vec<Finding> {
    // One-shot tooling runs nothing concurrently, so blocking there stalls no one.
    distinct_effects(index, |effect| {
        effect.in_async
            && effect.kind == EffectKind::BlockingIo
            && crate::source_role_for_path(Path::new(&effect.path)) != crate::SourceRole::Tooling
    })
    .into_iter()
    .map(|effect| {
        effect_finding(
            effect,
            "blocking_io_in_async_context",
            "Blocking I/O in async context",
            "executes blocking file I/O from an async function",
            "Use the runtime's asynchronous I/O API or isolate blocking work on a blocking executor.",
        )
    })
    .collect()
}

pub(crate) fn per_item_external_io_findings(index: &SystemsIndex) -> Vec<Finding> {
    distinct_effects(index, |effect| {
        effect.loop_line.is_some() && effect.kind == EffectKind::ExternalIo && !effect.batched_loop
    })
    .into_iter()
    .map(|effect| {
        effect_finding(
            effect,
            "per_item_external_io",
            "Per-item external I/O",
            "performs an external network operation for each loop item",
            "Batch the remote operation, prefetch concurrently with an explicit bound, or move it outside the loop.",
        )
    })
    .collect()
}

pub(crate) fn repeated_expensive_initialization_findings(index: &SystemsIndex) -> Vec<Finding> {
    distinct_effects(index, |effect| {
        effect.loop_line.is_some()
            && effect.kind == EffectKind::ExpensiveInitialization
            && !effect.loop_variant
    })
    .into_iter()
    .map(|effect| {
        effect_finding(
            effect,
            "repeated_expensive_initialization",
            "Repeated expensive initialization",
            "constructs expensive reusable state inside a loop",
            "Initialize the reusable client, parser, runtime, or graph state once before the loop.",
        )
    })
    .collect()
}

pub(crate) fn request_path_repository_scan_findings(index: &SystemsIndex) -> Vec<Finding> {
    distinct_effects(index, |effect| {
        effect.in_request_path && effect.kind == EffectKind::RepositoryScan
    })
    .into_iter()
    .map(|effect| {
        effect_finding(
            effect,
            "request_path_repository_scan",
            "Repository scan in request path",
            "scans repository files from a request/hot path",
            "Build repository state outside request handling and query the retained index.",
        )
    })
    .collect()
}

fn distinct_effects(
    index: &SystemsIndex,
    predicate: impl Fn(&SystemEffect) -> bool,
) -> Vec<&SystemEffect> {
    let mut seen = BTreeSet::new();
    index
        .effects()
        .iter()
        .filter(|effect| predicate(effect))
        .filter(|effect| {
            seen.insert((
                effect.path.as_str(),
                effect.function.as_str(),
                effect.loop_line,
                effect.kind,
            ))
        })
        .collect()
}

fn group_effects<K: Ord>(
    index: &SystemsIndex,
    mut key: impl FnMut(&SystemEffect) -> Option<K>,
) -> BTreeMap<K, BTreeSet<EffectKind>> {
    let mut groups = BTreeMap::new();
    for effect in index.effects() {
        if let Some(key) = key(effect) {
            groups
                .entry(key)
                .or_insert_with(BTreeSet::new)
                .insert(effect.kind);
        }
    }
    groups
}

fn grouped_finding(
    detector: &str,
    title: &'static str,
    message: String,
    recommendation: &str,
    location: (String, Option<usize>),
) -> Finding {
    auditor_core::finding!(
        detector,
        "systems_economics",
        title,
        message,
        Severity::Low,
        Confidence::High,
        location.0,
        location.1,
    )
    .with_recommendation(recommendation)
}

fn effect_finding(
    effect: &SystemEffect,
    detector: &str,
    title: &'static str,
    behavior: &str,
    recommendation: &str,
) -> Finding {
    let finding = auditor_core::finding!(
        detector,
        "systems_economics",
        title,
        format!("Function `{}` {behavior}.", effect.function),
        Severity::Low,
        Confidence::High,
        effect.path.clone(),
        Some(effect.loop_line.unwrap_or(effect.line)),
    )
    .with_recommendation(recommendation)
    .with_identity("operation", effect.operation.clone())
    .with_identity("effect", effect.kind.as_str())
    .with_evidence("function", effect.function.clone())
    .with_evidence("operation", effect.operation.clone())
    .with_evidence("effect", effect.kind.as_str())
    .with_evidence("language", effect.language.as_str())
    .with_evidence("loop_line", effect.loop_line.unwrap_or(0))
    .with_evidence("inside_loop", effect.loop_line.is_some())
    .with_evidence("async_context", effect.in_async)
    .with_evidence("hot_path", effect.in_hot_path)
    .with_evidence("request_path", effect.in_request_path);
    through_calls(finding, effect.via.as_deref())
}

/// Find files that are both high-churn and non-trivially complex.
pub(crate) fn churn_complexity_hotspot_findings(
    files: &[AuditedFile],
    churn: &HashMap<PathBuf, crate::churn::FileChurn>,
    complexity: &crate::cognitive::ComplexityIndex,
) -> Vec<Finding> {
    if churn.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for file in files {
        let change = match churn.get(&file.relative) {
            Some(change) if change.commits >= 2 => change,
            _ => continue,
        };
        if let Some(sum_ccx) = complexity.branch_sum(&file.relative) {
            candidates.push((
                file.relative.clone(),
                sum_ccx,
                change.loc_delta.max(0) as usize,
                change.commits,
            ));
        }
    }
    if candidates.len() < 8 {
        return Vec::new();
    }
    let mut complexities = candidates.iter().map(|item| item.1).collect::<Vec<_>>();
    complexities.sort_unstable();
    let mut growth = candidates.iter().map(|item| item.2).collect::<Vec<_>>();
    growth.sort_unstable();
    let cutoff = |values: &[usize]| values[(values.len() * 3).div_ceil(4).saturating_sub(1)];
    let complexity_cutoff = cutoff(&complexities);
    let growth_cutoff = cutoff(&growth);
    candidates
        .into_iter()
        .filter(|(_, ccx, delta, _)| *ccx >= complexity_cutoff && *delta >= growth_cutoff)
        .map(|(path, ccx, delta, commits)| {
            auditor_core::finding!(
                "churn_complexity_hotspot",
                "systems_economics",
                "Growth-weighted file complexity hotspot",
                format!("{} is complex and growing (CCX={ccx}, growth=+{delta} LOC).", path.display()),
                Severity::Medium,
                Confidence::Medium,
                path.to_string_lossy(),
                None,
            )
            .with_recommendation("Simplify the high-growth file while preserving its owner boundary and reducing net complexity.")
            .with_evidence("sum_ccx", ccx)
            .with_evidence("loc_delta", delta)
            .with_evidence("commit_count", commits)
            .with_evidence("quadrant", "hotspot")
        })
        .collect()
}

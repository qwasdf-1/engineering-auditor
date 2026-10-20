//! Resolved-topology architecture and package metrics.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use auditor_core::{Confidence, Finding, Severity};

use crate::config::{AuditorConfig, LayeringConfig};
use crate::systems_facts::{SystemsIndex, TopologyEdge};

enum Collector {
    Glob(String),
    Regex(regex::Regex),
    Type(String),
}

struct LayerResolver {
    layers: Vec<(String, Vec<Collector>)>,
}

impl LayerResolver {
    fn new(config: &LayeringConfig) -> Self {
        let layers = config
            .layers
            .iter()
            .map(|layer| {
                let collectors = layer
                    .collectors
                    .iter()
                    .filter_map(|collector| match collector.collector_type.as_str() {
                        "glob" => Some(Collector::Glob(collector.pattern.clone())),
                        "regex" => regex::Regex::new(&collector.pattern)
                            .ok()
                            .map(Collector::Regex),
                        "type" => Some(Collector::Type(collector.pattern.clone())),
                        _ => None,
                    })
                    .collect();
                (layer.name.clone(), collectors)
            })
            .collect();
        Self { layers }
    }

    fn resolve(&self, path: &str, type_names: &[String]) -> Vec<String> {
        self.layers
            .iter()
            .filter(|(_, collectors)| {
                collectors.iter().any(|collector| match collector {
                    Collector::Glob(pattern) => matches_glob(pattern, path),
                    Collector::Regex(pattern) => pattern.is_match(path),
                    Collector::Type(pattern) => type_names.iter().any(|name| name == pattern),
                })
            })
            .map(|(name, _)| name.clone())
            .collect()
    }
}

struct DependencyMatcher {
    rules: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
}

impl DependencyMatcher {
    fn new(config: &LayeringConfig) -> Self {
        Self {
            rules: config
                .rules
                .iter()
                .map(|rule| {
                    (
                        rule.layer.clone(),
                        (
                            rule.allow.iter().cloned().collect(),
                            rule.deny.iter().cloned().collect(),
                        ),
                    )
                })
                .collect(),
        }
    }

    fn violation(&self, source: &str, target: &str) -> Option<&'static str> {
        let (allow, deny) = self.rules.get(source)?;
        if deny.contains(target) {
            Some("layer must not depend on forbidden layer")
        } else if !allow.is_empty() && !allow.contains(target) {
            Some("layer must not depend on non-allowed layer")
        } else {
            None
        }
    }
}

struct Baseline {
    entries: BTreeSet<String>,
}

impl Baseline {
    fn from_config(root: &Path, path: Option<&str>) -> Self {
        let entries = path
            .map(Path::new)
            .map(|path| {
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    root.join(path)
                }
            })
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|source| {
                source
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self { entries }
    }

    fn suppresses(&self, source: &str, target: &str, edge: &TopologyEdge) -> bool {
        self.entries.contains(&format!(
            "{source}|{target}|{}|{}",
            edge.source, edge.target
        ))
    }
}

pub(crate) fn architectural_layering_findings(
    root: &Path,
    index: &SystemsIndex,
    config: &AuditorConfig,
) -> Vec<Finding> {
    let mut findings = cycle_findings(index);
    if !config.layering.has_layers() {
        return findings;
    }
    let resolver = LayerResolver::new(&config.layering);
    let matcher = DependencyMatcher::new(&config.layering);
    let baseline = Baseline::from_config(root, config.layering.baseline_path.as_deref());
    for edge in index.edges() {
        let Some(source_file) = index.files().get(&edge.source) else {
            continue;
        };
        let Some(target_file) = index.files().get(&edge.target) else {
            continue;
        };
        if !index.topology_complete_for(&edge.source) {
            continue;
        }
        let source_layers = resolver.resolve(&edge.source, &source_file.type_names);
        let mut target_types = target_file.type_names.clone();
        target_types.extend(edge.symbol.iter().cloned());
        let target_layers = resolver.resolve(&edge.target, &target_types);
        for source_layer in &source_layers {
            for target_layer in &target_layers {
                let Some(rule) = matcher.violation(source_layer, target_layer) else {
                    continue;
                };
                if baseline.suppresses(source_layer, target_layer, edge) {
                    continue;
                }
                findings.push(
                    auditor_core::finding!(
                        "architectural_layering",
                        "systems_economics",
                        "Architectural layer dependency violation",
                        format!(
                            "{source_layer} layer depends on {target_layer} layer through resolved {} edge: {rule}.",
                            edge.kind.as_str()
                        ),
                        Severity::Medium,
                        Confidence::High,
                        edge.source.clone(),
                        Some(edge.line),
                    )
                    .with_recommendation(
                        "Point the dependency inward or introduce an abstraction owned by the inner layer.",
                    )
                    .with_anchor(auditor_core::FindingAnchor::Structural(format!(
                        "layer-edge:{}->{}:{source_layer}->{target_layer}",
                        edge.source, edge.target
                    )))
                    .with_evidence("source_layer", source_layer.clone())
                    .with_evidence("target_layer", target_layer.clone())
                    .with_evidence("source_module", edge.source.clone())
                    .with_evidence("target_module", edge.target.clone())
                    .with_evidence("edge_kind", edge.kind.as_str())
                    .with_evidence("rule", rule)
                    .with_evidence("topology_complete", true)
                    .with_evidence("graph_input_provenance", "local_resolved_imports"),
                );
            }
        }
    }
    findings
}

/// Rotate a cycle to start at its least module so the same cycle discovered
/// from a different entry point keeps one identity.
fn canonical_cycle(cycle: &[String]) -> Vec<&str> {
    let start = cycle
        .iter()
        .enumerate()
        .min_by_key(|(_, module)| module.as_str())
        .map_or(0, |(index, _)| index);
    cycle[start..]
        .iter()
        .chain(&cycle[..start])
        .map(String::as_str)
        .collect()
}

fn cycle_findings(index: &SystemsIndex) -> Vec<Finding> {
    index
        .cycles()
        .iter()
        .filter_map(|cycle| {
            if cycle.iter().any(|path| !index.topology_complete_for(path)) {
                return None;
            }
            let path = cycle.first()?.clone();
            let line = index
                .edges()
                .iter()
                .filter(|edge| cycle.contains(&edge.source) && cycle.contains(&edge.target))
                .map(|edge| edge.line)
                .min();
            Some(
                auditor_core::finding!(
                    "architectural_layering",
                    "systems_economics",
                    "Resolved local dependency cycle",
                    format!(
                        "Resolved module cycle contains {}.",
                        cycle.join(" -> ")
                    ),
                    Severity::Medium,
                    Confidence::High,
                    path,
                    line,
                )
                .with_recommendation(
                    "Break the cycle by moving the shared contract inward or inverting one dependency.",
                )
                .with_anchor(auditor_core::FindingAnchor::Structural(format!(
                    "cycle:{}",
                    canonical_cycle(cycle).join("->")
                )))
                .with_evidence("cycle_modules", cycle.clone())
                .with_evidence("cycle_size", cycle.len())
                .with_evidence("topology_complete", true)
                .with_evidence("graph_input_provenance", "local_resolved_imports"),
            )
        })
        .collect()
}

pub(crate) fn package_coupling_metrics_findings(index: &SystemsIndex) -> Vec<Finding> {
    let packages = index
        .files()
        .values()
        .map(|file| file.package.clone())
        .collect::<BTreeSet<_>>();
    if packages.len() < 2 {
        return Vec::new();
    }
    let incomplete = index
        .files()
        .values()
        .filter(|file| file.unresolved_local_edges > 0)
        .map(|file| file.package.clone())
        .collect::<BTreeSet<_>>();
    let metrics = PackageMetrics::from_index(index);
    packages
        .into_iter()
        .filter(|package| !incomplete.contains(package))
        .filter_map(|package| package_finding(&package, &metrics))
        .collect()
}

struct PackageMetrics {
    afferent: BTreeMap<String, BTreeSet<String>>,
    efferent: BTreeMap<String, BTreeSet<String>>,
    abstract_types: BTreeMap<String, usize>,
    concrete_types: BTreeMap<String, usize>,
    topology_completeness: f64,
}

impl PackageMetrics {
    fn from_index(index: &SystemsIndex) -> Self {
        let mut metrics = Self {
            afferent: BTreeMap::new(),
            efferent: BTreeMap::new(),
            abstract_types: BTreeMap::new(),
            concrete_types: BTreeMap::new(),
            topology_completeness: index.topology_completeness(),
        };
        for edge in index
            .edges()
            .iter()
            .filter(|edge| edge.source_package != edge.target_package)
        {
            record_coupling(
                &mut metrics.efferent,
                &edge.source_package,
                &edge.target_package,
            );
            record_coupling(
                &mut metrics.afferent,
                &edge.target_package,
                &edge.source_package,
            );
        }
        for file in index.files().values() {
            *metrics
                .abstract_types
                .entry(file.package.clone())
                .or_default() += file.abstract_types;
            *metrics
                .concrete_types
                .entry(file.package.clone())
                .or_default() += file.concrete_types;
        }
        metrics
    }
}

fn record_coupling(
    connections: &mut BTreeMap<String, BTreeSet<String>>,
    source: &str,
    target: &str,
) {
    connections
        .entry(source.to_string())
        .or_default()
        .insert(target.to_string());
}

fn package_finding(package: &str, metrics: &PackageMetrics) -> Option<Finding> {
    let ca = metrics.afferent.get(package).map_or(0, BTreeSet::len);
    let ce = metrics.efferent.get(package).map_or(0, BTreeSet::len);
    // With one coupling edge, instability is 0 or 1 by construction and the
    // distance merely restates abstractness, so it says nothing about balance.
    if ca + ce < 2 {
        return None;
    }
    let abstract_count = metrics.abstract_types.get(package).copied().unwrap_or(0);
    let concrete_count = metrics.concrete_types.get(package).copied().unwrap_or(0);
    if abstract_count + concrete_count == 0 {
        return None;
    }
    let instability = ce as f64 / (ca + ce) as f64;
    let abstractness = abstract_count as f64 / (abstract_count + concrete_count) as f64;
    let distance = (abstractness + instability - 1.0).abs();
    let zone = classify_zone(instability, abstractness, distance);
    if zone == "Normal" {
        return None;
    }
    Some(
        auditor_core::finding!(
            "package_coupling_metrics",
            "systems_economics",
            "Package lies outside the main sequence",
            format!(
                "Package `{package}` has Ca={ca}, Ce={ce}, I={instability:.3}, A={abstractness:.3}, D={distance:.3}."
            ),
            Severity::Low,
            Confidence::High,
            package,
            None,
        )
        .with_recommendation(
            "Balance package abstraction and instability while reducing unnecessary cross-package edges.",
        )
        .with_anchor(auditor_core::FindingAnchor::Structural(format!(
            "package:{package}"
        )))
        .with_identity("zone", zone)
        .with_evidence("package", package)
        .with_evidence("ca", ca)
        .with_evidence("ce", ce)
        .with_evidence("instability", instability)
        .with_evidence("abstractness", abstractness)
        .with_evidence("distance", distance)
        .with_evidence("zone", zone)
        .with_evidence("topology_completeness", metrics.topology_completeness)
        .with_evidence("graph_input_provenance", "local_resolved_imports"),
    )
}

fn classify_zone(instability: f64, abstractness: f64, distance: f64) -> &'static str {
    if distance >= 0.5 {
        "Warning"
    } else if instability > 0.5 && abstractness > 0.5 {
        "Useless"
    } else if instability < 0.5 && abstractness < 0.5 {
        "Painful"
    } else {
        "Normal"
    }
}

fn matches_glob(pattern: &str, path: &str) -> bool {
    glob_match(
        &pattern.split('/').collect::<Vec<_>>(),
        &path.split('/').collect::<Vec<_>>(),
    )
}

fn glob_match(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (Some((segment, rest)), _) if *segment == "**" => {
            glob_match(rest, path)
                || path
                    .split_first()
                    .is_some_and(|(_, tail)| glob_match(pattern, tail))
        }
        (Some((segment, rest)), Some((value, tail))) => {
            wildcard_match(segment.as_bytes(), value.as_bytes()) && glob_match(rest, tail)
        }
        _ => false,
    }
}

fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    match (pattern.split_first(), value.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => {
            wildcard_match(rest, value)
                || value
                    .split_first()
                    .is_some_and(|(_, tail)| wildcard_match(pattern, tail))
        }
        (Some((b'?', rest)), Some((_, tail))) => wildcard_match(rest, tail),
        (Some((left, rest)), Some((right, tail))) if left == right => wildcard_match(rest, tail),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::matches_glob;

    #[test]
    fn glob_supports_zero_or_more_segments() {
        assert!(matches_glob("src/domain/**/*.rs", "src/domain/user.rs"));
        assert!(matches_glob(
            "src/domain/**/*.rs",
            "src/domain/model/user.rs"
        ));
        assert!(!matches_glob("src/domain/**/*.rs", "src/app/user.rs"));
    }
}

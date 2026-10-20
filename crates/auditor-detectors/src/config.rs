use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use auditor_core::{EvidenceValue, Finding, Severity};

#[derive(Clone, Debug)]
pub struct CollectorConfig {
    pub collector_type: String,
    pub pattern: String,
}

#[derive(Clone, Debug)]
pub struct LayerConfig {
    pub name: String,
    pub collectors: Vec<CollectorConfig>,
}

#[derive(Clone, Debug)]
pub struct DependencyRule {
    pub layer: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// `[exclusions.<name>]`: findings of the listed detectors under the listed
/// path prefixes are dropped. `.eaignore` removes paths from the whole audit;
/// an exclusion silences only some detectors there (detector source code that
/// quotes the patterns it detects).
#[derive(Clone, Debug)]
pub struct PathExclusion {
    pub name: String,
    pub paths: Vec<String>,
    pub detectors: Vec<String>,
}

impl PathExclusion {
    fn covers(&self, finding: &Finding) -> bool {
        self.detectors.contains(&finding.detector)
            && self
                .paths
                .iter()
                .any(|prefix| finding.primary_path().starts_with(prefix.as_str()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct LayeringConfig {
    pub layers: Vec<LayerConfig>,
    pub rules: Vec<DependencyRule>,
    pub baseline_path: Option<String>,
}

impl LayeringConfig {
    pub fn has_layers(&self) -> bool {
        !self.layers.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct AuditorConfig {
    pub detectors: BTreeMap<String, DetectorConfig>,
    pub layering: LayeringConfig,
    pub exclusions: Vec<PathExclusion>,
}

#[derive(Clone, Debug, Default)]
pub struct DetectorConfig {
    pub enabled: Option<bool>,
    pub threshold: Option<usize>,
    pub severity: Option<Severity>,
    pub entropy_threshold: Option<f64>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub ratio_threshold: Option<f64>,
    pub min_count: Option<usize>,
}

impl AuditorConfig {
    pub fn from_root(root: &Path) -> Self {
        let native = root.join("auditor.toml");
        if native.exists() {
            return Self::from_path(&native);
        }
        let donor_compat = root.join(".slop.toml");
        if donor_compat.exists() {
            let text = fs::read_to_string(donor_compat).unwrap_or_default();
            return Self::parse_slop_compat(&text);
        }
        Self::default()
    }

    pub fn from_path(path: &Path) -> Self {
        let text = fs::read_to_string(path).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let sections = parse_sections(text);
        let mut detectors: BTreeMap<String, DetectorConfig> = BTreeMap::new();
        let mut layering = LayeringConfig::default();
        let mut exclusions = Vec::new();
        for (section, props) in &sections {
            if let Some(name) = section.strip_prefix("exclusions.") {
                exclusions.push(parse_exclusion(name, props));
            } else {
                apply_config_section(section, props, &mut detectors, &mut layering);
            }
        }
        AuditorConfig {
            detectors,
            layering,
            exclusions,
        }
    }

    pub fn parse_slop_compat(text: &str) -> Self {
        let sections = parse_sections(text);
        let mut config = Self::default();
        for (section, key, detector) in [
            (
                "rules.structural.complexity",
                "cyclomatic_threshold",
                "function_complexity",
            ),
            (
                "rules.structural.complexity",
                "cognitive_threshold",
                "cognitive_complexity",
            ),
            (
                "rules.structural.complexity",
                "npath_threshold",
                "npath_complexity",
            ),
            (
                "rules.information.volume",
                "threshold",
                "halstead_information_density",
            ),
            (
                "rules.structural.class.complexity",
                "threshold",
                "class_structure_metrics",
            ),
            ("rules.structural.god_module", "threshold", "god_module"),
            (
                "rules.lexical.stutter",
                "min_overlap_tokens",
                "identifier_stutter",
            ),
            (
                "rules.structural.redundancy",
                "min_shared",
                "sibling_call_redundancy",
            ),
            (
                "rules.structural.local_imports",
                "threshold",
                "function_local_import",
            ),
            (
                "rules.information.magic_literals",
                "threshold",
                "magic_literal_cluster",
            ),
        ] {
            if let Some(value) =
                section_value(&sections, section, key).and_then(|value| value.parse::<usize>().ok())
            {
                config
                    .detectors
                    .entry(detector.to_string())
                    .or_default()
                    .threshold = Some(value);
            }
        }
        for (section, detector) in [
            ("rules.structural.hotspots", "churn_complexity_hotspot"),
            ("rules.structural.orphans", "orphan_symbol"),
        ] {
            if let Some(value) = section_value(&sections, section, "enabled") {
                config
                    .detectors
                    .entry(detector.to_string())
                    .or_default()
                    .enabled = Some(value == "true");
            }
        }
        for (section, key, detector) in [
            (
                "rules.lexical.verbosity",
                "max_mean_tokens",
                "identifier_verbosity",
            ),
            ("rules.lexical.tersity", "max_density", "identifier_tersity"),
            (
                "rules.structural.redundancy",
                "min_score",
                "sibling_call_redundancy",
            ),
        ] {
            if let Some(value) =
                section_value(&sections, section, key).and_then(|value| value.parse::<f64>().ok())
            {
                config
                    .detectors
                    .entry(detector.to_string())
                    .or_default()
                    .ratio_threshold = Some(value);
            }
        }
        for (section, detector) in [
            ("rules.lexical.verbosity", "identifier_verbosity"),
            ("rules.lexical.tersity", "identifier_tersity"),
        ] {
            if let Some(value) = section_value(&sections, section, "min_identifiers")
                .and_then(|value| value.parse::<usize>().ok())
            {
                config
                    .detectors
                    .entry(detector.to_string())
                    .or_default()
                    .min_count = Some(value);
            }
        }
        config
    }

    pub fn is_detector_enabled(&self, id: &str) -> bool {
        self.detectors
            .get(id)
            .and_then(|d| d.enabled)
            .unwrap_or(true)
    }

    pub fn threshold_for(&self, id: &str, default: usize) -> usize {
        self.detectors
            .get(id)
            .and_then(|d| d.threshold)
            .unwrap_or(default)
    }

    pub fn accepts_finding(&self, finding: &Finding) -> bool {
        !self
            .exclusions
            .iter()
            .any(|exclusion| exclusion.covers(finding))
            && self
                .detectors
                .get(&finding.detector)
                .is_none_or(|detector| detector.accepts(finding))
    }

    pub fn severity_for(&self, id: &str, default: Severity) -> Severity {
        self.detectors
            .get(id)
            .and_then(|d| d.severity)
            .unwrap_or(default)
    }

    pub fn entropy_threshold_for(&self, id: &str, default: f64) -> f64 {
        self.detectors
            .get(id)
            .and_then(|d| d.entropy_threshold)
            .unwrap_or(default)
    }

    pub fn min_length_for(&self, id: &str, default: usize) -> usize {
        self.detectors
            .get(id)
            .and_then(|d| d.min_length)
            .unwrap_or(default)
    }

    pub fn max_length_for(&self, id: &str, default: usize) -> usize {
        self.detectors
            .get(id)
            .and_then(|d| d.max_length)
            .unwrap_or(default)
    }
}

fn apply_config_section(
    section: &str,
    props: &BTreeMap<String, String>,
    detectors: &mut BTreeMap<String, DetectorConfig>,
    layering: &mut LayeringConfig,
) {
    if section == "layering" {
        layering.baseline_path = props.get("baseline_path").cloned();
    } else if let Some(layer_name) = section.strip_prefix("layering.layer.") {
        layering.layers.push(parse_layer(layer_name, props));
    } else if let Some(rule_layer) = section.strip_prefix("layering.rule.") {
        layering
            .rules
            .push(parse_dependency_rule(rule_layer, props));
    } else if let Some(detector_id) = section.strip_prefix("detectors.") {
        detectors.insert(detector_id.to_string(), parse_detector_config(props));
    }
}

fn parse_layer(name: &str, props: &BTreeMap<String, String>) -> LayerConfig {
    let collectors = props
        .iter()
        .flat_map(|(collector_type, value)| {
            comma_separated(value).map(|pattern| CollectorConfig {
                collector_type: collector_type.clone(),
                pattern: pattern.to_string(),
            })
        })
        .collect();
    LayerConfig {
        name: name.to_string(),
        collectors,
    }
}

fn parse_exclusion(name: &str, props: &BTreeMap<String, String>) -> PathExclusion {
    let list = |key: &str| {
        props
            .get(key)
            .map(|value| comma_separated(value).map(str::to_string).collect())
            .unwrap_or_default()
    };
    PathExclusion {
        name: name.to_string(),
        paths: list("paths"),
        detectors: list("detectors"),
    }
}

fn parse_dependency_rule(layer: &str, props: &BTreeMap<String, String>) -> DependencyRule {
    DependencyRule {
        layer: layer.to_string(),
        allow: props
            .get("allow")
            .map(|value| comma_separated(value).map(str::to_string).collect())
            .unwrap_or_default(),
        deny: props
            .get("deny")
            .map(|value| comma_separated(value).map(str::to_string).collect())
            .unwrap_or_default(),
    }
}

fn comma_separated(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn parse_detector_config(props: &BTreeMap<String, String>) -> DetectorConfig {
    let mut config = DetectorConfig::default();
    for (key, value) in props {
        match key.as_str() {
            "enabled" => config.enabled = Some(value == "true"),
            "threshold" => config.threshold = value.parse().ok(),
            "severity" => config.severity = parse_severity(value),
            "entropy_threshold" => config.entropy_threshold = value.parse().ok(),
            "min_length" => config.min_length = value.parse().ok(),
            "max_length" => config.max_length = value.parse().ok(),
            "ratio_threshold" => config.ratio_threshold = value.parse().ok(),
            "min_count" => config.min_count = value.parse().ok(),
            _ => {}
        }
    }
    config
}

fn parse_severity(value: &str) -> Option<Severity> {
    match value {
        "advisory" => Some(Severity::Advisory),
        "low" => Some(Severity::Low),
        "medium" => Some(Severity::Medium),
        "high" => Some(Severity::High),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

fn parse_sections(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections = BTreeMap::new();
    let mut current_section: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = Some(trimmed[1..trimmed.len() - 1].trim().to_string());
            continue;
        }
        let Some(section) = current_section.as_ref() else {
            continue;
        };
        if let Some((key, value)) = trimmed.split_once('=') {
            sections
                .entry(section.clone())
                .or_insert_with(BTreeMap::new)
                .insert(
                    key.trim().to_string(),
                    value.trim().trim_matches('"').to_string(),
                );
        }
    }
    sections
}

fn section_value<'a>(
    sections: &'a BTreeMap<String, BTreeMap<String, String>>,
    section: &str,
    key: &str,
) -> Option<&'a str> {
    sections.get(section)?.get(key).map(String::as_str)
}

fn integer_evidence(finding: &Finding, key: &str) -> Option<usize> {
    match finding.evidence.get(key)? {
        EvidenceValue::Integer(value) => usize::try_from(*value).ok(),
        _ => None,
    }
}

fn float_evidence(finding: &Finding, key: &str) -> Option<f64> {
    match finding.evidence.get(key)? {
        EvidenceValue::Float(value) => Some(*value),
        EvidenceValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

impl DetectorConfig {
    /// Whether this detector's settings keep `finding`: enabled, and at or
    /// past every configured minimum and threshold.
    fn accepts(&self, finding: &Finding) -> bool {
        self.enabled != Some(false)
            && self.min_count.is_none_or(|min_count| {
                integer_evidence(finding, "identifier_count").is_none_or(|value| value >= min_count)
            })
            && clears(self.threshold, threshold_metric(finding))
            && clears(self.ratio_threshold, ratio_metric(finding))
    }
}

/// Whether a measured value clears its configured threshold; an `inclusive`
/// metric keeps a value equal to the threshold.
fn clears<T: PartialOrd>(threshold: Option<T>, metric: Option<(T, bool)>) -> bool {
    match (threshold, metric) {
        (Some(threshold), Some((value, true))) => value >= threshold,
        (Some(threshold), Some((value, false))) => value > threshold,
        _ => true,
    }
}

fn threshold_metric(finding: &Finding) -> Option<(usize, bool)> {
    let (key, inclusive) = match finding.detector.as_str() {
        "function_complexity" => ("branch_count", false),
        "cognitive_complexity" => ("cognitive_complexity", false),
        "npath_complexity" => ("npath", false),
        "halstead_information_density" => ("halstead_volume", false),
        "class_structure_metrics" => ("method_count", false),
        "god_module" => ("definition_count", false),
        "identifier_stutter" => ("overlap_count", true),
        "sibling_call_redundancy" => ("shared_count", true),
        "function_local_import" => ("count", false),
        "magic_literal_cluster" => ("distinct_count", false),
        _ => return None,
    };
    integer_evidence(finding, key).map(|value| (value, inclusive))
}

fn ratio_metric(finding: &Finding) -> Option<(f64, bool)> {
    let (key, inclusive) = match finding.detector.as_str() {
        "identifier_verbosity" => ("mean_words", false),
        "identifier_tersity" => ("density", false),
        "sibling_call_redundancy" => ("score", true),
        _ => return None,
    };
    float_evidence(finding, key).map(|value| (value, inclusive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_defaults() {
        let config = AuditorConfig::parse("");
        assert!(config.is_detector_enabled("large_file"));
        assert_eq!(config.threshold_for("large_file", 1000), 1000);
        assert_eq!(
            config.severity_for("large_file", Severity::Medium),
            Severity::Medium
        );
    }

    #[test]
    fn disable_detector() {
        let config = AuditorConfig::parse("[detectors.large_file]\nenabled = false\n");
        assert!(!config.is_detector_enabled("large_file"));
    }

    #[test]
    fn override_threshold() {
        let config = AuditorConfig::parse("[detectors.large_file]\nthreshold = 500\n");
        assert_eq!(config.threshold_for("large_file", 1000), 500);
    }

    #[test]
    fn override_severity() {
        let config = AuditorConfig::parse("[detectors.large_file]\nseverity = \"high\"\n");
        assert_eq!(
            config.severity_for("large_file", Severity::Medium),
            Severity::High
        );
    }

    #[test]
    fn comments_are_ignored() {
        let config = AuditorConfig::parse("# this is a comment\n[detectors.foo]\nenabled = true\n");
        assert!(config.is_detector_enabled("foo"));
    }

    #[test]
    fn multiple_detectors() {
        let config = AuditorConfig::parse(
            "[detectors.first]\nenabled = false\n\n[detectors.second]\nthreshold = 42\n",
        );
        assert!(!config.is_detector_enabled("first"));
        assert!(config.is_detector_enabled("second"));
        assert_eq!(config.threshold_for("second", 99), 42);
    }

    #[test]
    fn slop_compat_maps_promoted_rule_settings() {
        let config = AuditorConfig::parse_slop_compat(
            "[rules.structural.complexity]\ncognitive_threshold = 28\n\
             [rules.information.volume]\nthreshold = 2400\n\
             [rules.structural.god_module]\nthreshold = 75\n\
             [rules.lexical.stutter]\nmin_overlap_tokens = 5\n\
             [rules.structural.orphans]\nenabled = false\n\
             [rules.lexical.verbosity]\nmax_mean_tokens = 5.0\nmin_identifiers = 8\n",
        );
        assert_eq!(config.threshold_for("cognitive_complexity", 10), 28);
        assert_eq!(
            config.threshold_for("halstead_information_density", 1500),
            2400
        );
        assert_eq!(config.threshold_for("god_module", 20), 75);
        assert_eq!(config.threshold_for("identifier_stutter", 2), 5);
        assert!(!config.is_detector_enabled("orphan_symbol"));
        let verbosity = config.detectors.get("identifier_verbosity").unwrap();
        assert_eq!(verbosity.ratio_threshold, Some(5.0));
        assert_eq!(verbosity.min_count, Some(8));
    }

    #[test]
    fn configured_thresholds_filter_typed_finding_evidence() {
        let config =
            AuditorConfig::parse_slop_compat("[rules.lexical.stutter]\nmin_overlap_tokens = 5\n");
        let weak = auditor_core::finding!(
            "identifier_stutter",
            "lexical",
            "weak",
            "weak",
            Severity::Medium,
            auditor_core::Confidence::Medium,
            "src/lib.rs",
            Some(1),
        )
        .with_evidence("overlap_count", 4usize);
        let strong = auditor_core::finding!(
            "identifier_stutter",
            "lexical",
            "strong",
            "strong",
            Severity::Medium,
            auditor_core::Confidence::Medium,
            "src/lib.rs",
            Some(2),
        )
        .with_evidence("overlap_count", 5usize);
        assert!(!config.accepts_finding(&weak));
        assert!(config.accepts_finding(&strong));
    }

    #[test]
    fn exclusions_drop_listed_detectors_under_listed_paths() {
        let config = AuditorConfig::parse(
            "[exclusions.detector-patterns]\npaths = src/rules/, tooling/\ndetectors = shell_execution, eval_in_production\n",
        );
        let finding = |detector: &str, path: &str| {
            auditor_core::finding!(
                detector,
                "security",
                "t",
                "m",
                Severity::High,
                auditor_core::Confidence::High,
                path,
                Some(1),
            )
        };
        assert!(!config.accepts_finding(&finding("shell_execution", "src/rules/shell.rs")));
        assert!(!config.accepts_finding(&finding("eval_in_production", "tooling/run.py")));
        assert!(config.accepts_finding(&finding("shell_execution", "src/main.rs")));
        assert!(config.accepts_finding(&finding("dead_code", "src/rules/shell.rs")));
        assert_eq!(config.exclusions[0].name, "detector-patterns");
    }

    #[test]
    fn layering_config_parses_layers_and_rules() {
        let toml = r#"
[layering.layer.domain]
glob = src/domain/**/*.rs, src/domain/**/*.py

[layering.layer.app]
glob = src/app/**/*.rs

[layering.layer.infrastructure]
glob = src/infra/**/*.rs

[layering.rule.app]
allow = domain

[layering.rule.domain]
deny = app, infrastructure

[layering]
baseline_path = .auditor/layering-baseline.yaml
"#;
        let config = AuditorConfig::parse(toml);
        assert!(config.layering.has_layers());
        assert_eq!(config.layering.layers.len(), 3);
        // Sections are stored in BTreeMap (alphabetical order)
        assert_eq!(config.layering.layers[0].name, "app");
        assert_eq!(config.layering.layers[0].collectors.len(), 1);
        assert_eq!(
            config.layering.layers[0].collectors[0].collector_type,
            "glob"
        );
        assert_eq!(
            config.layering.layers[0].collectors[0].pattern,
            "src/app/**/*.rs"
        );
        assert_eq!(config.layering.layers[1].name, "domain");
        assert_eq!(config.layering.layers[1].collectors.len(), 2);
        assert_eq!(
            config.layering.layers[1].collectors[0].pattern,
            "src/domain/**/*.rs"
        );
        assert_eq!(
            config.layering.layers[1].collectors[1].pattern,
            "src/domain/**/*.py"
        );
        assert_eq!(config.layering.layers[2].name, "infrastructure");
        assert_eq!(config.layering.rules.len(), 2);
        assert_eq!(config.layering.rules[0].layer, "app");
        assert_eq!(config.layering.rules[0].allow, vec!["domain"]);
        assert_eq!(config.layering.rules[1].layer, "domain");
        assert_eq!(config.layering.rules[1].deny, vec!["app", "infrastructure"]);
        assert_eq!(
            config.layering.baseline_path.as_deref(),
            Some(".auditor/layering-baseline.yaml")
        );
    }

    #[test]
    fn layering_config_empty_by_default() {
        let config = AuditorConfig::parse("[detectors.foo]\nenabled = true\n");
        assert!(!config.layering.has_layers());
        assert!(config.layering.layers.is_empty());
        assert!(config.layering.rules.is_empty());
        assert!(config.layering.baseline_path.is_none());
    }
}

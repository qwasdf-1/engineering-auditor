//! Executable positive, negative, profile, and language capability contracts.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    analyze_repository_with_provider, parse_co_edit_journal, parse_repo_graph, AnalysisMethod,
    GraphInputs, ScanProfile, SourceDiscoveryOptions,
};

use super::test_support::TempRepo;

#[path = "capability_cases_correctness.rs"]
mod correctness_cases;
#[path = "capability_cases_performance.rs"]
mod performance_cases;
#[path = "capability_cases_structural.rs"]
mod structural_cases;

type Setup = fn(&TempRepo);

#[derive(Clone)]
pub(super) struct CapabilityCase {
    id: &'static str,
    profile: ScanProfile,
    method: AnalysisMethod,
    positive: Vec<(String, String)>,
    negative: Vec<(String, String)>,
    positive_setup: Option<Setup>,
    negative_setup: Option<Setup>,
    repo_graphs: Option<(&'static str, &'static str)>,
    co_edit_journals: Option<(&'static str, &'static str)>,
}

impl CapabilityCase {
    pub(super) fn new(
        id: &'static str,
        profile: ScanProfile,
        positive_path: impl Into<String>,
        positive: impl Into<String>,
        negative_path: impl Into<String>,
        negative: impl Into<String>,
    ) -> Self {
        Self {
            id,
            profile,
            method: AnalysisMethod::Auto,
            positive: vec![(positive_path.into(), positive.into())],
            negative: vec![(negative_path.into(), negative.into())],
            positive_setup: None,
            negative_setup: None,
            repo_graphs: None,
            co_edit_journals: None,
        }
    }

    #[cfg(feature = "graph-analysis")]
    pub(super) fn repo_graphs(mut self, positive: &'static str, negative: &'static str) -> Self {
        self.repo_graphs = Some((positive, negative));
        self
    }
    #[cfg(feature = "graph-analysis")]
    pub(super) fn co_edit_journals(
        mut self,
        positive: &'static str,
        negative: &'static str,
    ) -> Self {
        self.co_edit_journals = Some((positive, negative));
        self
    }

    pub(super) fn method(mut self, method: AnalysisMethod) -> Self {
        self.method = method;
        self
    }

    pub(super) fn positive_files(mut self, files: Vec<(String, String)>) -> Self {
        self.positive = files;
        self
    }

    pub(super) fn negative_files(mut self, files: Vec<(String, String)>) -> Self {
        self.negative = files;
        self
    }

    pub(super) fn positive_setup(mut self, setup: Setup) -> Self {
        self.positive_setup = Some(setup);
        self
    }
}

fn capability_cases() -> Vec<CapabilityCase> {
    let mut cases = structural_cases::default_cases();
    cases.extend(correctness_cases::default_cases());
    cases.extend(performance_cases::default_cases());
    #[cfg(feature = "graph-analysis")]
    cases.extend(structural_cases::feature_cases());
    cases
}

fn analyze_case(
    case: &CapabilityCase,
    positive: bool,
    profile: ScanProfile,
) -> crate::RepositoryAnalysis {
    analyze_transformed_case(case, positive, profile, |source| source.to_string())
}

fn analyze_transformed_case(
    case: &CapabilityCase,
    positive: bool,
    profile: ScanProfile,
    transform: fn(&str) -> String,
) -> crate::RepositoryAnalysis {
    let repository = TempRepo::new(case.id);
    let (files, setup) = if positive {
        (&case.positive, case.positive_setup)
    } else {
        (&case.negative, case.negative_setup)
    };
    for (path, source) in files {
        repository.write(path, transform(source));
    }
    if let Some(setup) = setup {
        setup(&repository);
    }
    let inputs = GraphInputs {
        repo_graph: case.repo_graphs.map(|graphs| {
            parse_repo_graph(&versioned_input(if positive { graphs.0 } else { graphs.1 })).unwrap()
        }),
        co_edit_journal: case.co_edit_journals.map(|journals| {
            parse_co_edit_journal(&versioned_input(if positive {
                journals.0
            } else {
                journals.1
            }))
            .unwrap()
        }),
    };
    analyze_repository_with_provider(
        repository.path(),
        SourceDiscoveryOptions {
            include_references: false,
            method: case.method,
        },
        profile,
        &inputs,
    )
    .unwrap_or_else(|error| panic!("{} repository analysis failed: {error}", case.id))
}

fn versioned_input(input: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(input).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("schema_version".to_string(), serde_json::json!(1));
    value.to_string()
}

fn detector_count(analysis: &crate::RepositoryAnalysis, detector: &str) -> usize {
    analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == detector)
        .count()
}

fn active_catalog() -> BTreeMap<&'static str, &'static str> {
    crate::registry::detector_registry(ScanProfile::Full)
        .into_iter()
        .map(|definition| (definition.id, definition.profile))
        .collect()
}

#[test]
fn active_catalog_has_executable_positive_and_negative_capabilities() {
    let catalog = active_catalog();
    let cases = capability_cases();
    let mut case_profiles = BTreeMap::new();
    let mut failures = Vec::new();

    for case in &cases {
        if case_profiles
            .insert(case.id, case.profile.as_str())
            .is_some()
        {
            failures.push(format!("duplicate capability case for {}", case.id));
        }
    }
    if case_profiles != catalog {
        let catalog_ids = catalog.keys().copied().collect::<BTreeSet<_>>();
        let case_ids = case_profiles.keys().copied().collect::<BTreeSet<_>>();
        failures.push(format!(
            "catalog/capability mismatch; missing={:?}, extra={:?}",
            catalog_ids.difference(&case_ids).collect::<Vec<_>>(),
            case_ids.difference(&catalog_ids).collect::<Vec<_>>()
        ));
        for (id, profile) in &case_profiles {
            if let Some(catalog_profile) = catalog.get(id) {
                if profile != catalog_profile {
                    failures.push(format!(
                        "{id} capability profile is {profile}, catalog profile is {catalog_profile}"
                    ));
                }
            }
        }
    }

    for case in &cases {
        // A language scoped to `full` runs its case in the full profile.
        let profile = language_tests::native_language(case).map_or(case.profile, |language| {
            language_profile(case.id, case.profile, language)
        });
        let positive = analyze_case(case, true, profile);
        let positive_count = detector_count(&positive, case.id);
        if positive_count == 0 {
            failures.push(format!(
                "{} did not fire for its positive repository; findings={:?}",
                case.id,
                positive
                    .findings
                    .iter()
                    .map(|finding| finding.detector.as_str())
                    .collect::<BTreeSet<_>>()
            ));
        } else {
            for finding in positive
                .findings
                .iter()
                .filter(|finding| finding.detector == case.id)
            {
                if finding.locations.is_empty() {
                    failures.push(format!("{} emitted a finding without a location", case.id));
                }
                if finding.recommendation.trim().is_empty() {
                    failures.push(format!(
                        "{} emitted a finding without a recommendation",
                        case.id
                    ));
                }
                if crate::registry::detector_spec(case.id).is_some_and(|spec| spec.layer == "L3") {
                    for key in [
                        "cost_expression",
                        "cost_terms",
                        "cost_composition",
                        "alternative_cost_expression",
                        "cost_confidence_boundary",
                    ] {
                        if !finding.evidence.contains_key(key) {
                            failures.push(format!(
                                "{} emitted a Layer 3 finding without `{key}` evidence",
                                case.id
                            ));
                        }
                    }
                }
            }
        }

        let negative = analyze_case(case, false, profile);
        let negative_count = detector_count(&negative, case.id);
        if negative_count != 0 {
            failures.push(format!(
                "{} fired {negative_count} time(s) for its semantic negative/boundary repository",
                case.id
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn full_only_capabilities_do_not_leak_into_gating() {
    let mut failures = Vec::new();
    for case in capability_cases()
        .into_iter()
        .filter(|case| case.profile == ScanProfile::Full)
    {
        let gating = analyze_case(&case, true, ScanProfile::Gating);
        let leaked = detector_count(&gating, case.id);
        if leaked != 0 {
            failures.push(format!(
                "full-only detector {} leaked {leaked} finding(s) into gating",
                case.id
            ));
        }
        if gating
            .detectors
            .iter()
            .any(|definition| definition.id == case.id)
        {
            failures.push(format!(
                "full-only detector {} appeared in the gating registry",
                case.id
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(feature = "graph-analysis")]
#[test]
fn feature_catalog_additions_are_explicit_and_executable() {
    let feature_ids = structural_cases::feature_cases()
        .into_iter()
        .map(|case| case.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        feature_ids,
        BTreeSet::from([
            "large_file_split_candidate",
            "repo_graph_high_fanout_source_file",
            "repo_graph_import_cycle",
            "repo_graph_isolated_source_file",
            "shotgun_surgery_cluster",
            "small_file_consolidation_candidate",
            "tooling_overlap",
        ])
    );
}

#[path = "capability_language_tests.rs"]
mod language_tests;

/// Insert blank lines before the first line, or after it when line one must
/// stay first (shebang, front matter).
fn prepend_blank_lines(source: &str) -> String {
    match source.split_once('\n') {
        Some((first, rest)) if first.starts_with("#!") || first == "---" => {
            format!("{first}\n\n\n\n{rest}")
        }
        _ => format!("\n\n\n{source}"),
    }
}

/// Double every existing blank line, shifting lines throughout the file
/// without touching any code line.
fn widen_blank_lines(source: &str) -> String {
    source.replace("\n\n", "\n\n\n")
}

fn finding_ids(analysis: &crate::RepositoryAnalysis, detector: &str) -> BTreeSet<String> {
    analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == detector)
        .map(|finding| finding.id.clone())
        .collect()
}

type SourcePerturbation = fn(&str) -> String;

#[test]
fn every_detector_keeps_finding_ids_when_unrelated_lines_shift() {
    let perturbations: [(&str, SourcePerturbation); 2] = [
        ("prepend_blank_lines", prepend_blank_lines),
        ("widen_blank_lines", widen_blank_lines),
    ];
    let mut failures = Vec::new();
    for case in capability_cases() {
        let before = finding_ids(&analyze_case(&case, true, case.profile), case.id);
        for (name, perturbation) in perturbations {
            let after = finding_ids(
                &analyze_transformed_case(&case, true, case.profile, perturbation),
                case.id,
            );
            if before != after {
                failures.push(format!(
                    "{} changed finding IDs under {name}: before={before:?} after={after:?}",
                    case.id
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Finding-ID v3 is frozen (docs/reference/stability.md): an unchanged
/// finding keeps its ID across releases. This golden file pins the ID of
/// every positive capability finding, so any identity change fails loudly.
/// It is an identity freeze, not detector capability evidence. Regenerate
/// with `EA_BLESS_FINDING_IDS=1` only for an intended, migrated ID change.
#[test]
fn finding_ids_match_the_frozen_v3_golden() {
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/finding-ids-v3.tsv");
    let cases = capability_cases();
    let definitions = crate::registry::detector_registry(ScanProfile::Full)
        .into_iter()
        .map(|definition| (definition.id, definition))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut actual = BTreeSet::new();
    for case in &cases {
        for id in finding_ids(&analyze_case(case, true, case.profile), case.id) {
            actual.insert(format!("{}\t{id}", case.id));
        }
        // Pin every supported language arm through the same retargeted
        // boundaries that back the catalog's language claims.
        for claim in &definitions[case.id].language_support {
            if !matches!(
                claim.mode,
                crate::LanguageSupportMode::Semantic
                    | crate::LanguageSupportMode::ExactLexical
                    | crate::LanguageSupportMode::Partial
            ) {
                continue;
            }
            let variant = language_tests::retarget_case(case, claim.language);
            let profile = language_profile(case.id, case.profile, claim.language);
            for id in finding_ids(&analyze_case(&variant, true, profile), case.id) {
                actual.insert(format!("{}\t{id}", case.id));
            }
        }
    }
    if std::env::var_os("EA_BLESS_FINDING_IDS").is_some() {
        let mut golden = actual.into_iter().collect::<Vec<_>>().join("\n");
        golden.push('\n');
        std::fs::write(&golden_path, golden).unwrap();
        return;
    }
    let ran = cases.iter().map(|case| case.id).collect::<BTreeSet<_>>();
    let expected = std::fs::read_to_string(&golden_path)
        .unwrap()
        .lines()
        .filter(|line| line.split('\t').next().is_some_and(|id| ran.contains(id)))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let missing = expected.difference(&actual).collect::<Vec<_>>();
    let added = actual.difference(&expected).collect::<Vec<_>>();
    assert!(
        missing.is_empty() && added.is_empty(),
        "finding IDs changed; missing={missing:?} added={added:?}"
    );
}

/// The profile that exercises `detector` in `language`: `full` where the
/// detector is scoped out of gating there (`full_profile_in`).
fn language_profile(detector: &str, profile: ScanProfile, language: &str) -> ScanProfile {
    match crate::registry::detector_spec(detector) {
        Some(spec) if !spec.reports_in(profile, language) => ScanProfile::Full,
        _ => profile,
    }
}

#[test]
fn production_only_detectors_do_not_emit_in_tooling() {
    let spec = crate::registry::detector_spec("duplicate_import").expect("registered");
    assert!(spec.emits_role("production"));
    assert!(!spec.emits_role("tooling"));
    assert!(!spec.emits_role("test"));
    // Repository-level findings carry no file role.
    assert!(spec.emits_role("repository"));
    let code = crate::registry::detector_spec("too_many_parameters").expect("registered");
    assert!(code.emits_role("tooling"));
}

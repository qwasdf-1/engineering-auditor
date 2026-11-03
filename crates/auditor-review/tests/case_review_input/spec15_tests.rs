use std::collections::BTreeSet;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_report::{ColorMode, NoReviewArtifactLoader};
use auditor_review::{AgendaPlanner, CaseInstantiationResult, CaseKind, CaseTemplateRegistry};
use auditor_scan::{run_scan, ScanFormat, ScanRequest};

use super::*;

/// A scanned fixture repository: `populate` writes its sources under `repo`.
fn scanned_fixture(name: &str, populate: impl FnOnce(&std::path::Path)) -> Fixture {
    static ID: AtomicU64 = AtomicU64::new(0);
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let id = ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("ea-case-review-input-{name}-{epoch}-{id}"));
    let repo = root.join("repo");
    populate(&repo);
    let artifacts = root.join("artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    let report = artifacts.join("audit.json");
    run_scan(
        ScanRequest {
            repo_root: repo.clone(),
            analysis: AnalysisRequest {
                discovery: SourceDiscoveryOptions {
                    include_references: false,
                    method: AnalysisMethod::Auto,
                },
                profile: ScanProfile::Full,
                scope: ScanScope::All,
                repository_map_output: Some(report.clone()),
                excluded_paths: BTreeSet::from([report.clone()]),
            },
            external_inputs: Vec::new(),
            baseline: None,
            format: ScanFormat::Json,
            output: Some(report.clone()),
            color: ColorMode::Plain,
            summary_only: false,
            review_loader: Box::new(NoReviewArtifactLoader),
        },
        &NoGraphProvider,
    )
    .unwrap();
    Fixture { root, repo, report }
}

fn walk_cognitive_fixture() -> Fixture {
    scanned_fixture("walk", |repo| {
        let generator = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tooling/review_eval/generate_walk_cognitive_repo.py");
        let status = Command::new("python3")
            .arg(generator)
            .arg(repo)
            .status()
            .unwrap();
        assert!(status.success());
    })
}

#[test]
fn walk_cognitive_generates_the_bound_multi_finding_shadow_case() {
    let fixture = walk_cognitive_fixture();
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.report).unwrap()).unwrap();
    let detectors = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["evidence"]["function"] == "walk_cognitive")
        .filter_map(|finding| finding["detector"].as_str())
        .collect::<BTreeSet<_>>();
    assert!([
        "cognitive_complexity",
        "function_complexity",
        "halstead_information_density",
        "too_many_parameters",
    ]
    .iter()
    .all(|detector| detectors.contains(detector)));

    let (templates, prompts, routes) = contracts();
    let input = verify(
        &fixture,
        Some(&templates),
        Some(&prompts),
        Some(&routes),
        &[],
        &[],
        None,
    )
    .unwrap();
    let agenda = AgendaPlanner.plan_with_foundation_policy(&input).unwrap();
    let registry = CaseTemplateRegistry::from_input(&input).unwrap();
    let cases = registry.instantiate_agenda(&agenda, &input).unwrap();
    let shared = cases
        .iter()
        .filter_map(|result| match result {
            CaseInstantiationResult::Instantiated(case)
                if case.case_kind == CaseKind::SharedRootCause =>
            {
                Some(case)
            }
            _ => None,
        })
        .find(|case| case.finding_ids.len() >= 4)
        .expect("walk_cognitive findings should form one shared-root case");
    assert_eq!(
        shared.template_status,
        auditor_review::TemplateStatus::Shadow
    );
}

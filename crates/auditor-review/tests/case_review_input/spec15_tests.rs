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

use std::sync::atomic::AtomicUsize;

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

/// Two findings on one function that share a recommendation.
fn overlapping_recommendation_fixture() -> Fixture {
    scanned_fixture("overlap", |repo| {
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/cache.py"),
            "def stage(payload):\n    with open(\"/tmp/stage-a\", \"w\") as first:\n        first.write(payload)\n    with open(\"/tmp/stage-b\", \"w\") as second:\n        second.write(payload)\n",
        )
        .unwrap();
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
    assert_eq!(shared.mandatory_probes.len(), 7);
}

/// A probe that blocks a claim slot, as the premise of an abstention.
type Premise = (auditor_review::StableId, auditor_review::ClaimSlot);

/// A template-agnostic scripted reviewer: it qualifies, answers every
/// offered probe `unknown`, abstains with a blocked premise and a
/// measurement recommendation, completes with the gap, and clears
/// challenges.
struct AbstainingTransport {
    /// Per case: its probes, and one that blocks a claim slot.
    premises: Vec<(BTreeSet<String>, Option<Premise>)>,
    case_turns: AtomicUsize,
}

impl AbstainingTransport {
    fn new(plan: &auditor_review::ReviewPlanOutput) -> Self {
        let premises = plan
            .cases
            .iter()
            .map(|planned| {
                let premise = planned.probe_definitions.iter().find_map(|definition| {
                    definition
                        .blocked_claim_slots
                        .first()
                        .map(|slot| (definition.probe_id.clone(), *slot))
                });
                let probes = planned
                    .probe_definitions
                    .iter()
                    .map(|definition| definition.probe_id.to_string())
                    .collect();
                (probes, premise)
            })
            .collect();
        Self {
            premises,
            case_turns: AtomicUsize::new(0),
        }
    }

    /// The blocked premise claim and the measurement recommendation that
    /// abstention requires, for the case whose probes the answer tool offers
    /// (cases over the same findings differ by their probes).
    fn measurement_calls(&self, answer: &Value, recommend: &Value) -> Vec<(&'static str, Value)> {
        let offered = answer["function"]["parameters"]["properties"]["probe_id"]["enum"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect::<BTreeSet<_>>();
        let Some((probe, slot)) = self
            .premises
            .iter()
            .find(|(probes, _)| offered.is_subset(probes))
            .and_then(|(_, premise)| premise.clone())
        else {
            return Vec::new();
        };
        let finding_ids = recommend["function"]["parameters"]["properties"]["affected_finding_ids"]
            ["items"]["enum"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        vec![
            (
                auditor_review::TOOL_RECORD_CASE_CLAIM,
                json!({
                    "claim_ref": "measurement-premise",
                    "slot": slot,
                    "kind": "recommendation_premise",
                    "proposition": "The unresolved boundary requires focused evidence before any combined change.",
                    "status": "blocked",
                    "supporting_probe_ids": [probe],
                    "counter_probe_ids": [],
                    "dependency_claim_refs": [],
                    "assumptions": [],
                    "confidence": "medium"
                }),
            ),
            (
                auditor_review::TOOL_RECORD_CASE_RECOMMENDATION,
                json!({
                    "recommendation_ref": "measure-before-change",
                    "family": "measurement_before_change",
                    "summary": "Resolve the missing evidence before combining remediation.",
                    "scope_ids": finding_ids,
                    "premise_claim_refs": ["measurement-premise"],
                    "affected_finding_ids": finding_ids,
                    "detector_advice_relationship": "narrows",
                    "preconditions": ["The missing behavior can be inspected or measured."],
                    "expected_benefits": [],
                    "risks": ["The measurement may show the findings are independent."],
                    "costs": ["Requires focused inspection."],
                    "alternatives": [],
                    "verification_steps": [{
                        "method": "Inspect the unresolved boundary.",
                        "expected_observation": "The evidence settles the blocked boundary.",
                        "evidence_probe_ids": [probe]
                    }],
                    "materiality": "low",
                    "materiality_basis": "Material impact is not established by the bounded evidence.",
                    "confidence": "medium"
                }),
            ),
        ]
    }
}

impl auditor_review::Transport for AbstainingTransport {
    fn post(
        &self,
        request: auditor_review::TransportRequest,
    ) -> Result<auditor_review::TransportResponse, String> {
        let body: Value =
            serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
        if let Some(response) = super::spec11_tests::scripted_qualification(&body) {
            return response;
        }
        let tools = body["tools"].as_array().cloned().unwrap_or_default();
        let tool = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["function"]["name"].as_str() == Some(name))
        };
        if tool(auditor_review::TOOL_COMPLETE_CASE_CHALLENGE).is_some() {
            return Ok(super::spec11_tests::tool_response(
                auditor_review::TOOL_COMPLETE_CASE_CHALLENGE,
                json!({}),
            ));
        }
        let Some(answer) = tool(auditor_review::TOOL_ANSWER_CASE_PROBE) else {
            return Err("unexpected stage".to_string());
        };
        let gap = json!({"unresolved_gaps": ["The bounded packet does not settle the case."]});
        self.case_turns.fetch_add(1, Ordering::Relaxed);
        // A conversation's first turn holds only the instructions and the
        // case context.
        let first_turn = body["messages"]
            .as_array()
            .is_some_and(|messages| messages.len() <= 2);
        let mut calls = Vec::new();
        if first_turn {
            for probe in answer["function"]["parameters"]["properties"]["probe_id"]["enum"]
                .as_array()
                .into_iter()
                .flatten()
            {
                calls.push((
                    auditor_review::TOOL_ANSWER_CASE_PROBE,
                    json!({
                        "probe_id": probe,
                        "answer": "unknown",
                        "evidence_ids": [],
                        "explanation": "The bounded packet does not establish this probe.",
                        "limitations": ["No admissible evidence resolves the probe."]
                    }),
                ));
            }
        }
        if let Some(recommend) = tool(auditor_review::TOOL_RECORD_CASE_RECOMMENDATION) {
            if first_turn {
                calls.extend(self.measurement_calls(answer, recommend));
            }
        }
        calls.push((auditor_review::TOOL_COMPLETE_CASE_REVIEW, gap));
        Ok(super::spec11_tests::tool_calls_response(calls))
    }
}

#[test]
fn shadow_templates_run_end_to_end_with_a_scripted_reviewer() {
    let fixture = overlapping_recommendation_fixture();
    let request = super::spec11_tests::request(&fixture);
    let plan = auditor_review::plan_engineering_review(&request).unwrap();
    let kinds = plan
        .cases
        .iter()
        .map(|planned| {
            assert_eq!(
                planned.case.template_status,
                auditor_review::TemplateStatus::Shadow
            );
            planned.case.case_kind
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            CaseKind::SharedRootCause,
            CaseKind::RecommendationConsolidation
        ])
    );

    let transport = AbstainingTransport::new(&plan);
    let output_path = fixture.report.with_file_name("reviewed-v2.json");
    let output = auditor_review::run_case_engineering_review(
        &auditor_review::ReviewRunRequest {
            plan: request,
            output: output_path.clone(),
            consent: auditor_review::HostedConsent {
                allow_hosted: true,
                allow_anonymous: true,
            },
        },
        &transport,
        &auditor_review::UncontrolledReview,
    )
    .unwrap();
    assert_eq!(output.case_count, 2);
    assert!(transport.case_turns.load(Ordering::Relaxed) >= 2);
    let artifact: Value =
        serde_json::from_slice(&std::fs::read(&output.review_artifact_path).unwrap()).unwrap();
    assert_eq!(artifact["coverage"]["terminal_case_count"], 2);
    for truth in artifact["execution_truth"].as_array().unwrap() {
        assert_eq!(
            truth["closure"]["final_state"],
            "blocked_by_missing_evidence"
        );
        assert!(truth["closure"]["failure"].is_null());
    }

    let verified =
        auditor_review::verify_case_engineering_review(&auditor_review::ReviewVerifyRequest {
            repo_root: fixture.repo.clone(),
            report: output_path,
        })
        .unwrap();
    assert!(verified.valid);
    assert_eq!(verified.artifact_digest, output.review_artifact_digest);
}

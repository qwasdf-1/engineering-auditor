#[test]
fn removed_review_commands_name_the_rebuild() {
    for command in ["plan", "inspect-case", "verify", "disposition", "gate"] {
        let error = super::review_command::run_review(&[command.to_string()])
            .expect_err("the v2 review commands are removed");
        assert!(
            error.contains("removed while the engineering review is rebuilt"),
            "{command}: {error}"
        );
    }
    let run = super::review_command::run_review(&["run".to_string()])
        .expect_err("review run needs the experimental flag");
    assert!(
        run.contains("removed while the engineering review is rebuilt")
            && run.contains("--experimental-reviewer")
    );
    let unknown = super::review_command::run_review(&["probe".to_string()])
        .expect_err("an unknown review command stays unknown");
    assert!(unknown.contains("unknown review command `probe`"));
}

#[test]
fn scan_review_is_refused_and_plain_scan_is_unaffected() {
    let error = super::dispatch_command("scan", &["--review".to_string()])
        .expect_err("scan --review is removed");
    assert!(error.contains("scan --review was removed"));
}

#[test]
fn kept_review_commands_still_validate_their_arguments() {
    let transcript = super::review_command::run_review(&["transcript".to_string()])
        .expect_err("transcript requires an input");
    assert!(transcript.contains("review transcript requires --input"));
    let provider =
        super::review_command::run_review(&["provider".to_string(), "discover".to_string()])
            .expect_err("provider discovery requires routes");
    assert!(provider.contains("review provider requires --routes"));
    let run = super::review_command::run_review(&strings(&["run", "--experimental-reviewer"]))
        .expect_err("review run requires a report");
    assert!(run.contains("review run requires --report"));
    let run = super::review_command::run_review(&strings(&[
        "run",
        "--experimental-reviewer",
        "--report",
        "audit.json",
    ]))
    .expect_err("review run requires routes");
    assert!(run.contains("review run requires --routes"));
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("ea-cli-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    root
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn review_history_writes_a_digest_of_head() {
    let root = temp_dir("history");
    std::fs::write(root.join("app/a.py"), "x = 1\n").unwrap();
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "a",
        ],
    ] {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let output = root.join("history.json");
    let printed = super::review_command::run_review(&strings(&[
        "history",
        "--repo-root",
        root.to_str().unwrap(),
        "--window-days",
        "30",
        "--output",
        output.to_str().unwrap(),
    ]))
    .unwrap();
    assert!(printed.is_empty());
    let digest =
        auditor_review::load_history_digest(&output, &root).expect("the digest is of HEAD");
    assert_eq!(digest.window_days, 30);
    assert_eq!(digest.files[0].path, "app/a.py");
    let error = super::review_command::run_review(&strings(&["history", "--window-days", "0"]))
        .unwrap_err();
    assert!(error.contains("at least one day"), "{error}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn review_check_patch_reports_a_verdict() {
    let root = temp_dir("check-patch");
    std::fs::write(
        root.join("app/store.py"),
        "def record(event):\n    try:\n        print(event)\n    except:\n        pass\n",
    )
    .unwrap();
    let analysis = auditor_detectors::analyze_repository(
        &root,
        auditor_detectors::SourceDiscoveryOptions::default(),
        auditor_detectors::ScanProfile::Full,
    )
    .unwrap();
    let swallowed = analysis
        .findings
        .iter()
        .find(|finding| finding.detector == "try_except_pass")
        .expect("the fixture swallows an exception")
        .id
        .clone();
    let scratch = temp_dir("check-patch-inputs");
    let patch = scratch.join("fix.diff");
    std::fs::write(
        &patch,
        "--- a/app/store.py\n+++ b/app/store.py\n@@ -4,2 +4,2 @@\n-    except:\n-        pass\n+    except OSError as error:\n+        raise RuntimeError(\"record failed\") from error\n",
    )
    .unwrap();
    let brief = scratch.join("brief.json");
    std::fs::write(
        &brief,
        r#"{"schema_version": 1, "goals": [{"id": "g", "text": "t", "priority": "high"}],
            "risk_posture": {"change_tolerance": "low", "operational_criticality": "low", "security_sensitivity": "low"},
            "test_command": {"argv": ["sh", "-c", "test -f app/store.py"], "timeout_seconds": 30}}"#,
    )
    .unwrap();
    let printed = super::review_command::run_review(&strings(&[
        "check-patch",
        "--repo-root",
        root.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "--resolves",
        &swallowed,
        "--brief",
        brief.to_str().unwrap(),
    ]))
    .unwrap();
    let check: serde_json::Value = serde_json::from_str(&printed).unwrap();
    assert_eq!(check["verdict"], "verified", "{check:#}");
    assert_eq!(check["resolved_findings"][0], swallowed.as_str());
    assert_eq!(check["tests"]["exit_code"], 0);
    let missing = super::review_command::run_review(&strings(&[
        "check-patch",
        "--patch",
        patch.to_str().unwrap(),
    ]))
    .unwrap_err();
    assert!(missing.contains("--resolves"), "{missing}");
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&scratch).unwrap();
}

/// A provider that refuses every request, so the review records no answer.
struct Refusing;

impl auditor_review::Transport for Refusing {
    fn post(
        &self,
        _request: auditor_review::TransportRequest,
    ) -> Result<auditor_review::TransportResponse, String> {
        Ok(auditor_review::TransportResponse {
            status: 403,
            body: br#"{"error":{"message":"refused"}}"#.to_vec(),
        })
    }
}

#[test]
fn review_qualify_fails_an_unanswered_run_offline_and_rubric_lists_no_items() {
    let root = temp_dir("qualify");
    std::fs::write(
        root.join("app/store.py"),
        "def record(event):\n    try:\n        print(event)\n    except:\n        pass\n",
    )
    .unwrap();
    std::fs::write(
        root.join("app/api.py"),
        "from app.store import record\n\ndef handle(event):\n    record(event)\n",
    )
    .unwrap();
    let scratch = temp_dir("qualify-artifacts");
    let report = scratch.join("audit.json");
    crate::scan_command::run_scan(&strings(&[
        "--repo-root",
        root.to_str().unwrap(),
        "--profile",
        "full",
        "--output",
        report.to_str().unwrap(),
    ]))
    .unwrap();
    let mut provider = auditor_review::ProviderConfig::openai_compatible(
        "fake",
        "https://fake.invalid/v1",
        None::<String>,
        vec!["fake-model".to_string()],
    );
    provider.model_protocols = std::collections::BTreeMap::from([(
        "fake-model".to_string(),
        auditor_review::ProviderProtocol::ChatCompletions,
    )]);
    let budget = auditor_review::RouteBudget {
        max_requests: 8,
        max_input_bytes: 1 << 22,
        max_output_tokens: 4096,
        timeout_ms: 1000,
        max_cost_microusd: 0,
        allow_unknown_cost: true,
    };
    let routes = auditor_review::ReviewRoutesV2 {
        schema_version: auditor_review::REVIEW_ROUTES_V2_SCHEMA_VERSION,
        providers: vec![provider],
        single_reviewer: auditor_review::V2ProviderChain {
            targets: vec![auditor_review::RouteTarget {
                provider_id: "fake".to_string(),
                model: "fake-model".to_string(),
            }],
        },
        review_budget: budget,
        question_budget: budget,
    };
    let routes_path = scratch.join("routes.json");
    std::fs::write(&routes_path, serde_json::to_string(&routes).unwrap()).unwrap();

    let text = std::fs::read_to_string(&report).unwrap();
    let reference = auditor_report::parse_repository_map_reference(&text).unwrap();
    let map = auditor_review::load_repository_map(&report, &reference).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let view = auditor_review::RepositoryView::new(&value, &map).unwrap();
    let review = auditor_review::run_engineering_review(
        &view,
        auditor_review::ReviewSetup {
            repo_root: &root,
            brief: None,
            templates: &auditor_review::QuestionTemplateBundle::checked_in().unwrap(),
            prompts: &auditor_review::ReviewPromptBundleV2::parse(crate::review_run::PROMPT_BUNDLE)
                .unwrap(),
            routes: &routes,
            consent: auditor_review::HostedConsent {
                allow_hosted: true,
                allow_anonymous: false,
            },
            transport: &Refusing,
            transcript: &auditor_review::TranscriptRecorder::null(),
            checker: None,
            max_patch_checks: 0,
            resume: None,
            checkpoint: None,
            progress: None,
        },
    )
    .unwrap();
    assert!(review.answered().next().is_none());
    let review_path = scratch.join("review.json");
    std::fs::write(&review_path, serde_json::to_string(&review).unwrap()).unwrap();
    // The harness reads only the recorded files.
    std::fs::remove_dir_all(&root).unwrap();

    let sheet = super::review_command::run_review(&strings(&[
        "rubric",
        "--review",
        review_path.to_str().unwrap(),
    ]))
    .unwrap();
    let sheet: auditor_review::UsefulnessSheet = serde_json::from_str(&sheet).unwrap();
    assert_eq!(sheet.review_digest, review.digest());
    assert!(sheet.items.is_empty());

    let output = scratch.join("qualification.json");
    let review_path = review_path.to_str().unwrap();
    let failure = super::review_command::run_review(&strings(&[
        "qualify",
        "--report",
        report.to_str().unwrap(),
        "--review",
        review_path,
        "--review",
        review_path,
        "--routes",
        routes_path.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]))
    .unwrap_err();
    let printed = failure
        .strip_prefix(crate::gate_command::POLICY_FAILURE)
        .expect("a failed gate exits as a policy failure");
    let written = std::fs::read_to_string(&output).unwrap();
    let qualification: auditor_review::ReviewQualification =
        serde_json::from_str(&written).unwrap();
    assert_eq!(
        serde_json::from_str::<auditor_review::ReviewQualification>(printed).unwrap(),
        qualification
    );
    let gates = &qualification.runs[0].gates;
    assert!(
        gates[0].passed,
        "the run is bound to these inputs: {gates:?}"
    );
    assert!(!gates[1].passed && gates[1].failures[0].contains("was not answered"));
    assert!(!qualification.passed);

    let error = super::review_command::run_review(&strings(&[
        "qualify",
        "--report",
        report.to_str().unwrap(),
        "--review",
        review_path,
        "--routes",
        routes_path.to_str().unwrap(),
        "--min-overlap",
        "2",
    ]))
    .unwrap_err();
    assert!(error.contains("--min-overlap takes a share"), "{error}");
    let error = super::review_command::run_review(&strings(&["qualify", "--report", "a.json"]))
        .unwrap_err();
    assert!(error.contains("requires --review"), "{error}");
    std::fs::remove_dir_all(&scratch).unwrap();
}

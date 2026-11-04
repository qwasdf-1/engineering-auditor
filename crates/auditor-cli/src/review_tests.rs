#[test]
fn removed_review_commands_name_the_rebuild() {
    for command in [
        "plan",
        "inspect-case",
        "run",
        "verify",
        "disposition",
        "gate",
    ] {
        let error = super::review_command::run_review(&[command.to_string()])
            .expect_err("the v2 review commands are removed");
        assert!(
            error.contains("removed while the engineering review is rebuilt"),
            "{command}: {error}"
        );
    }
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

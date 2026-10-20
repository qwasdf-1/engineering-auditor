use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ea-cli-review-{name}-{nonce}"));
    std::fs::create_dir_all(&root).expect("temporary directory");
    root
}

#[test]
fn v2_plan_and_case_cli_contracts_reject_missing_or_unknown_inputs() {
    let missing = super::review_command::run_review(&["plan".to_string()])
        .expect_err("review plan requires its complete bound input set");
    assert!(missing.contains("review plan requires --report"));

    let unknown = super::review_command::run_review(&[
        "plan".to_string(),
        "--protocol".to_string(),
        "v2".to_string(),
    ])
    .expect_err("review plan has no public protocol selector");
    assert!(unknown.contains("unknown review plan argument `--protocol`"));

    let malformed = super::review_command::run_review(&[
        "inspect-case".to_string(),
        "--plan".to_string(),
        "missing.json".to_string(),
        "--case-id".to_string(),
        "not-a-case".to_string(),
    ])
    .expect_err("inspect-case rejects malformed typed identities before I/O");
    assert!(malformed.contains("stable id"));
}

#[test]
fn review_run_and_scan_review_require_the_complete_v2_input_set() {
    let run = super::review_command::run_review(&[
        "run".to_string(),
        "--report".to_string(),
        "audit.json".to_string(),
        "--repo-root".to_string(),
        ".".to_string(),
        "--output".to_string(),
        "reviewed.json".to_string(),
        "--routes".to_string(),
        "routes.json".to_string(),
        "--prompt-bundle".to_string(),
        "prompts.json".to_string(),
    ])
    .expect_err("case templates are mandatory");
    assert!(run.contains("review run requires --case-templates"));

    let scan = super::review_command::run_scan_review(&[
        "--review".to_string(),
        "--output".to_string(),
        "reviewed.json".to_string(),
        "--routes".to_string(),
        "routes.json".to_string(),
        "--prompt-bundle".to_string(),
        "prompts.json".to_string(),
    ])
    .expect_err("case templates are mandatory before scan execution");
    assert!(scan.contains("scan --review requires --case-templates"));
}

#[test]
fn provider_qualification_is_case_bound_and_has_no_standalone_command() {
    let error = super::review_command::run_review(&[
        "provider".to_string(),
        "qualify".to_string(),
        "--routes".to_string(),
        "routes.json".to_string(),
    ])
    .expect_err("standalone qualification was removed with v1");
    assert!(error.contains("failed to read provider routes") || error.contains("unknown"));
}

#[test]
fn v1_review_artifacts_are_rejected_explicitly() {
    let root = temp_root("v1-rejection");
    let artifact = root.join("review.json");
    let ledger = root.join("ledger.json");
    std::fs::write(
        &artifact,
        r#"{"schema_version":1,"review_id":"review:old"}"#,
    )
    .expect("write old artifact");
    std::fs::write(
        &ledger,
        r#"{"schema_version":1,"review_id":"review:old","dispositions":[]}"#,
    )
    .expect("write ledger");
    let error = super::review_command::run_review(&[
        "gate".to_string(),
        "--review".to_string(),
        artifact.display().to_string(),
        "--dispositions".to_string(),
        ledger.display().to_string(),
    ])
    .expect_err("v1 artifacts are unsupported after cutover");
    assert!(error.contains("only v2 is accepted"));
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

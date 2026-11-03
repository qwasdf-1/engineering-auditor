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

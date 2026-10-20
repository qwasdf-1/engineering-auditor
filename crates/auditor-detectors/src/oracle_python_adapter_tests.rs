use super::*;

pub(super) fn run_ruff(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("ruff");
    verify_checkout_snapshot("ruff", &donor_root);
    let binary = provisioned_file(
        "EA_RUFF_BIN",
        "target/donor-binaries/ruff/bin/ruff",
        "Ruff binary",
    );
    verify_version_contains(&binary, &["--version"], "ruff 0.15.20", "Ruff");
    let mut command = Command::new(binary);
    command.current_dir(root).args([
        "check",
        "--no-cache",
        "--output-format",
        "json",
        "--select",
        case.donor_rule,
        "--config",
        "lint.mccabe.max-complexity=10",
        case.relative_path,
    ]);
    let output = run_command(&mut command, &[0, 1]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload
        .as_array()
        .expect("Ruff output must be an array")
        .iter()
        .filter(|finding| finding["code"].as_str() == Some(case.donor_rule))
        .map(|finding| {
            native_finding(
                root,
                case,
                finding["filename"]
                    .as_str()
                    .expect("Ruff finding must contain filename"),
                Some(
                    finding["location"]["row"]
                        .as_u64()
                        .expect("Ruff finding must contain location.row")
                        as usize,
                ),
            )
        })
        .collect()
}

pub(super) fn run_pylint(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("pylint");
    verify_checkout_snapshot("pylint", &donor_root);
    let binary = provisioned_file(
        "EA_PYLINT_BIN",
        "target/donor-binaries/pylint/bin/pylint",
        "Pylint binary",
    );
    verify_version_contains(&binary, &["--version"], "pylint 4.1.0-dev0", "Pylint");
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .env("PYLINTHOME", root.join(".pylint"))
        .args([
            "--output-format=json2",
            "--persistent=no",
            "--score=no",
            "--disable=all",
            "--enable",
            case.donor_rule,
            "--max-nested-blocks=5",
            case.relative_path,
        ]);
    let output = run_command(&mut command, &[0, 8]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload["messages"]
        .as_array()
        .expect("Pylint JSON2 output must contain messages")
        .iter()
        .filter(|finding| finding["symbol"].as_str() == Some(case.donor_rule))
        .map(|finding| {
            native_finding(
                root,
                case,
                finding["path"]
                    .as_str()
                    .expect("Pylint finding must contain path"),
                Some(
                    finding["line"]
                        .as_u64()
                        .expect("Pylint finding must contain line") as usize,
                ),
            )
        })
        .collect()
}

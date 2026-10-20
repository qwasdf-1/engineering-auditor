use super::*;

pub(super) fn run_import_linter(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let binary = architecture_binary(
        "import-linter",
        "EA_IMPORT_LINTER_BIN",
        "target/donor-binaries/import-linter/bin/lint-imports",
        "Import Linter binary",
        "2.13",
    );
    let mut command = Command::new(binary);
    command.current_dir(root).args([
        "--config",
        ".importlinter",
        "--contract",
        "ea-layers",
        "--no-cache",
    ]);
    let output = run_command(&mut command, &[0, 1]);
    let stdout = String::from_utf8(output.stdout).expect("Import Linter output must be UTF-8");
    let broken = stdout.contains("EA layers BROKEN");
    assert_eq!(
        output.status.code(),
        Some(if broken { 1 } else { 0 }),
        "Import Linter status disagrees with its native contract result"
    );
    if !broken {
        return Vec::new();
    }
    assert!(
        stdout.contains("domain.user -> app.service (l.1)"),
        "Import Linter did not report the expected native dependency edge: {stdout}"
    );
    vec![native_finding(root, case, case.relative_path, Some(1))]
}

pub(super) fn run_dependency_cruiser(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let binary = architecture_binary(
        "dependency-cruiser",
        "EA_DEPCRUISE_BIN",
        "target/donor-binaries/dependency-cruiser/node_modules/.bin/depcruise",
        "Dependency Cruiser binary",
        "18.1.0",
    );
    let mut command = Command::new(binary);
    command.current_dir(root).args([
        "--config",
        ".dependency-cruiser.cjs",
        "--output-type",
        "json",
        "a.js",
        "b.js",
    ]);
    let output = run_command(&mut command, &[0, 1]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload["summary"]["violations"]
        .as_array()
        .expect("Dependency Cruiser JSON must contain summary.violations")
        .iter()
        .filter(|violation| violation["rule"]["name"].as_str() == Some(case.donor_rule))
        .map(|violation| {
            native_finding(
                root,
                case,
                violation["from"]
                    .as_str()
                    .expect("Dependency Cruiser violation must contain from"),
                None,
            )
        })
        .collect()
}

fn architecture_binary(
    donor_id: &str,
    env_name: &str,
    default_binary: &str,
    label: &str,
    version: &str,
) -> PathBuf {
    verify_checkout_snapshot(donor_id, &donor_path(donor_id));
    let binary = provisioned_file(env_name, default_binary, label);
    verify_version_contains(&binary, &["--version"], version, label);
    binary
}

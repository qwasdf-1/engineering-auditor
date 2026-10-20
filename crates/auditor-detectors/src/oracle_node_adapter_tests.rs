use super::*;

struct NodeOracleSpec {
    donor_id: &'static str,
    env_name: &'static str,
    default_binary: &'static str,
    label: &'static str,
    version: &'static str,
}

pub(super) fn run_eslint(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let binary = provision_node_oracle(NodeOracleSpec {
        donor_id: "eslint",
        env_name: "EA_ESLINT_BIN",
        default_binary: "target/donor-binaries/eslint/node_modules/.bin/eslint",
        label: "ESLint",
        version: "10.6.0",
    });
    let mut command = Command::new(binary);
    command.current_dir(root).args([
        "--no-config-lookup",
        "--format",
        "json",
        "--rule",
        "no-duplicate-imports:error",
        case.relative_path,
    ]);
    let output = run_command(&mut command, &[0, 1]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload
        .as_array()
        .expect("ESLint output must be an array")
        .iter()
        .flat_map(|file| {
            file["messages"]
                .as_array()
                .expect("ESLint file result must contain messages")
                .iter()
                .filter(|message| message["ruleId"].as_str() == Some(case.donor_rule))
                .map(|message| {
                    native_finding(
                        root,
                        case,
                        file["filePath"]
                            .as_str()
                            .expect("ESLint file result must contain filePath"),
                        Some(
                            message["line"]
                                .as_u64()
                                .expect("ESLint message must contain line")
                                as usize,
                        ),
                    )
                })
        })
        .collect()
}

pub(super) fn run_jscpd(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let binary = provision_node_oracle(NodeOracleSpec {
        donor_id: "jscpd",
        env_name: "EA_JSCPD_BIN",
        default_binary: "target/donor-binaries/jscpd/node_modules/.bin/jscpd",
        label: "JSCPD",
        version: "4.2.5",
    });
    let report_dir = root.join(".jscpd-report");
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args(["--silent", "--reporters", "json", "--output"])
        .arg(&report_dir)
        .args([
            "--min-lines",
            "8",
            "--min-tokens",
            "50",
            "--exitCode",
            "0",
            ".",
        ]);
    run_command(&mut command, &[0]);
    let payload = read_json(&report_dir.join("jscpd-report.json"));
    payload["duplicates"]
        .as_array()
        .expect("JSCPD report must contain duplicates")
        .iter()
        .map(|duplicate| {
            let locations = [&duplicate["firstFile"], &duplicate["secondFile"]];
            let (path, line) = locations
                .into_iter()
                .map(|location| {
                    (
                        normalize_path(
                            root,
                            location["name"]
                                .as_str()
                                .expect("JSCPD clone location must contain name"),
                        ),
                        location["start"]
                            .as_u64()
                            .expect("JSCPD clone location must contain start")
                            as usize,
                    )
                })
                .min()
                .expect("JSCPD clone must contain two locations");
            native_finding(root, case, &path, Some(line))
        })
        .collect()
}

fn provision_node_oracle(spec: NodeOracleSpec) -> PathBuf {
    let donor_root = donor_path(spec.donor_id);
    verify_checkout_snapshot(spec.donor_id, &donor_root);
    let binary = provisioned_file(
        spec.env_name,
        spec.default_binary,
        &format!("{} binary", spec.label),
    );
    verify_binary_version(&binary, spec.version, spec.label);
    binary
}

fn verify_binary_version(binary: &Path, expected: &str, label: &str) {
    let mut command = Command::new(binary);
    command.arg("--version");
    let output = run_command(&mut command, &[0]);
    let actual = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{label} version output must be UTF-8: {error}"));
    assert_eq!(
        actual.trim().trim_start_matches('v'),
        expected,
        "{label} executable does not match the parity version"
    );
}

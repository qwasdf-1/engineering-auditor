use super::*;

pub(super) fn run_pmd(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("pmd");
    verify_checkout_snapshot("pmd", &donor_root);
    let java = provisioned_file(
        "EA_PMD_JAVA",
        "target/donor-runtimes/java-21-jdk/bin/java",
        "Java 21 runtime for PMD",
    );
    let classpath = env::var_os("EA_PMD_CLASSPATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target/donor-binaries/pmd/lib/*"));
    verify_pmd_version(&java, &classpath);

    let mut command = Command::new(java);
    command.current_dir(root).arg("-cp").arg(classpath).args([
        "net.sourceforge.pmd.cli.PmdCli",
        "check",
        "-d",
        case.relative_path,
        "-f",
        "json",
        "-R",
        &format!("category/java/codestyle.xml/{}", case.donor_rule),
        "--no-cache",
        "--no-fail-on-violation",
        "--no-progress",
    ]);
    let output = run_command(&mut command, &[0]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload["files"]
        .as_array()
        .expect("PMD JSON output must contain files")
        .iter()
        .flat_map(|file| {
            let filename = file["filename"]
                .as_str()
                .expect("PMD file result must contain filename");
            file["violations"]
                .as_array()
                .expect("PMD file result must contain violations")
                .iter()
                .filter(|finding| finding["rule"].as_str() == Some(case.donor_rule))
                .map(move |finding| {
                    native_finding(
                        root,
                        case,
                        filename,
                        Some(
                            finding["beginline"]
                                .as_u64()
                                .expect("PMD violation must contain beginline")
                                as usize,
                        ),
                    )
                })
        })
        .collect()
}

fn verify_pmd_version(java: &Path, classpath: &Path) {
    let mut command = Command::new(java);
    command
        .arg("-cp")
        .arg(classpath)
        .args(["net.sourceforge.pmd.cli.PmdCli", "--version"]);
    let output = run_command(&mut command, &[0]);
    let actual = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("PMD version output must be UTF-8: {error}"));
    assert!(
        actual.contains("PMD 7.27.0-SNAPSHOT")
            && actual.contains("e30a70186909232657aeb35f36cd400818f64e71"),
        "PMD executable does not match the locked parity snapshot: {actual}"
    );
}

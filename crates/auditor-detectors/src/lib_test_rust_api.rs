fn rust_findings(files: &[(&str, &str)]) -> Vec<(String, String, usize)> {
    let root = temp_root("rust_api");
    for (path, source) in files {
        write(&root, path, source);
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.detector.as_str(),
                "exit_in_production" | "console_output_in_production" | "debugger_in_production"
            )
        })
        .map(|finding| {
            (
                finding.detector.clone(),
                finding.primary_path().to_string(),
                finding.primary_line().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort();
    findings
}

#[test]
fn rust_prints_and_exits_belong_to_programs_not_libraries() {
    let library = "use std::process;\n\npub fn fail() {\n    println!(\"failed\");\n    process::exit(1);\n}\n";
    let tool = "pub fn usage() {\n    eprintln!(\"usage: tool\");\n    std::process::exit(2);\n    dbg!(1);\n}\n";
    assert_eq!(
        rust_findings(&[
            ("lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
            ("lib/src/lib.rs", "mod report;\n"),
            ("lib/src/report.rs", library),
            ("tool/Cargo.toml", "[package]\nname = \"tool\"\n"),
            ("tool/src/main.rs", "mod usage;\nfn main() {}\n"),
            ("tool/src/usage.rs", tool),
        ]),
        [
            (
                "console_output_in_production".to_string(),
                "lib/src/report.rs".to_string(),
                4
            ),
            ("debugger_in_production".to_string(), "tool/src/usage.rs".to_string(), 4),
            ("exit_in_production".to_string(), "lib/src/report.rs".to_string(), 5),
        ]
    );
}

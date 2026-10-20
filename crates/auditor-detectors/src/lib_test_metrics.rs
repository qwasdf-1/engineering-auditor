fn distinct_blocks_are_not_duplicates_case() {
    let root = temp_root("dup_distinct");
    write(
        &root,
        "src/lib.rs",
        r#"
fn read_config(path: &str) -> String {
    let content = std::fs::read_to_string(path).unwrap();
    content.trim().to_string()
}

fn compute_average(values: &[f64]) -> f64 {
    let sum: f64 = values.iter().sum();
    sum / values.len() as f64
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let dup_count = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "source_duplicate_group")
        .count();
    assert_eq!(
        dup_count, 0,
        "expected 0 duplicate groups for distinct blocks, got {dup_count}"
    );
}

#[test]
fn churn_window_is_anchored_on_head_not_the_wall_clock_or_other_refs() {
    let root = temp_root("churn-anchor");
    let git = |args: &[&str], date: &str| {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    };
    let commit = |path: &str, body: &str, date: &str| {
        write(&root, path, body);
        git(&["add", "src"], date);
        git(&["commit", "-m", path], date);
    };
    git(&["init"], "");
    git(&["config", "user.name", "test"], "");
    git(&["config", "user.email", "test@test"], "");
    // Outside the window ending at HEAD.
    commit("src/old.rs", "pub fn old() {}\n", "2019-06-01T00:00:00Z");
    commit("src/a.rs", "pub fn a() {}\n", "2020-01-01T00:00:00Z");
    // A newer commit on another ref is not part of the scanned history.
    git(&["checkout", "-b", "side"], "");
    commit("src/side.rs", "pub fn side() {}\n", "2020-01-04T00:00:00Z");
    git(&["checkout", "-"], "");
    commit("src/a.rs", "pub fn a() { let _ = 1; }\n", "2020-01-05T00:00:00Z");

    // The whole history is years older than the wall clock, yet the window
    // ending at HEAD still sees both in-window commits to `src/a.rs`.
    let churn = crate::churn::collect_file_churn(&root);
    assert_eq!(churn[Path::new("src/a.rs")].commits, 2);
    assert!(!churn.contains_key(Path::new("src/old.rs")));
    assert!(!churn.contains_key(Path::new("src/side.rs")));

    // Splitting `a.rs` moves its lines into `b.rs`: neither file grows, while
    // a genuine addition in the same commit still counts.
    let body = (0..40).map(|index| format!("pub fn f{index}() {{}}\n")).collect::<String>();
    commit("src/a.rs", &body, "2020-01-06T00:00:00Z");
    write(&root, "src/a.rs", "pub fn a() {}\n");
    write(&root, "src/b.rs", &body);
    write(&root, "src/0.rs", "pub fn c() {}\npub fn d() {}\n");
    // A large unrelated addition in another language does not absorb the move.
    write(&root, "src/data.json", &"[1]\n".repeat(200));
    commit("src/a.rs", "pub fn a() {}\n", "2020-01-07T00:00:00Z");
    let churn = crate::churn::collect_file_churn(&root);
    assert_eq!(churn[Path::new("src/b.rs")].loc_delta, 0);
    assert_eq!(churn[Path::new("src/0.rs")].loc_delta, 0);
    // Only edits after a file's creation grow it: shrinking `b.rs` does not.
    commit("src/b.rs", "pub fn f0() {}\n", "2020-01-08T00:00:00Z");
    commit(
        "src/0.rs",
        "pub fn c() {}\npub fn d() {}\npub fn e() {}\n",
        "2020-01-09T00:00:00Z",
    );
    let churn = crate::churn::collect_file_churn(&root);
    assert!(churn[Path::new("src/b.rs")].loc_delta < 0);
    assert_eq!(churn[Path::new("src/0.rs")].loc_delta, 1);
}

#[test]
fn churn_complexity_hotspot_detects_high_churn_high_complexity_function() {
    let root = temp_root("churn-hotspot");
    write(
        &root,
        "src/lib.rs",
        r#"
pub fn hotspot_fn(data: &[u8]) {
    if data.is_empty() { return; }
    if data.len() > 100 { return; }
    if data[0] == 0 { return; }
    if data[1] == 1 { return; }
    if data[2] == 2 { return; }
    if data[3] == 3 { return; }
    if data[4] == 4 { return; }
    if data[5] == 5 { return; }
    if data[6] == 6 { return; }
    if data[7] == 7 { return; }
    if data[8] == 8 { return; }
    if data[9] == 9 { return; }
    if data[10] == 10 { return; }
    if data[11] == 11 { return; }
    for a in data {
        if *a > 0 {
            for b in 0..*a {
                if b > 10 {
                    for c in 0..b {
                        if c > 5 {
                            let _ = c;
                        }
                    }
                }
            }
        }
    }
}
"#,
    );
    for index in 0..8 {
        write(
            &root,
            &format!("src/filler_{index}.rs"),
            &format!("pub fn filler_{index}() {{}}\n"),
        );
    }

    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init"]);
    git(&["config", "user.name", "test"]);
    git(&["config", "user.email", "test@test"]);
    git(&["add", "src"]);
    git(&["commit", "-m", "initial"]);

    for i in 0..12 {
        let extra = format!("// touch {i}\n");
        write(&root, "src/lib.rs", &format!(
            "{extra}pub fn hotspot_fn(data: &[u8]) {{\n    if data.is_empty() {{ return; }}\n    if data.len() > 100 {{ return; }}\n    if data[0] == 0 {{ return; }}\n    if data[1] == 1 {{ return; }}\n    if data[2] == 2 {{ return; }}\n    if data[3] == 3 {{ return; }}\n    if data[4] == 4 {{ return; }}\n    if data[5] == 5 {{ return; }}\n    if data[6] == 6 {{ return; }}\n    if data[7] == 7 {{ return; }}\n    if data[8] == 8 {{ return; }}\n    if data[9] == 9 {{ return; }}\n    if data[10] == 10 {{ return; }}\n    if data[11] == 11 {{ return; }}\n    for a in data {{\n        if *a > 0 {{\n            for b in 0..*a {{\n                if b > 10 {{\n                    for c in 0..b {{\n                        if c > 5 {{\n                            let _ = c;\n                        }}\n                    }}\n                }}\n            }}\n        }}\n    }}\n}}\n"
        ));
        if i == 0 {
            for index in 0..8 {
                write(
                    &root,
                    &format!("src/filler_{index}.rs"),
                    &format!("pub fn filler_{index}() {{ let value = {index}; }}\n"),
                );
            }
        }
        git(&["add", "src"]);
        git(&["commit", "-m", &format!("touch {i}")]);
    }

    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let detectors: BTreeSet<_> = analysis
        .findings
        .iter()
        .map(|finding| finding.detector.as_str())
        .collect();

    assert!(
        detectors.contains("churn_complexity_hotspot"),
        "expected churn_complexity_hotspot in findings, got {detectors:?}",
    );

    let hotspot_findings: Vec<_> = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector.as_str() == "churn_complexity_hotspot")
        .collect();
    assert!(!hotspot_findings.is_empty());
    let evidence = hotspot_findings
        .iter()
        .find(|finding| finding.primary_path() == "src/lib.rs")
        .expect("complex source file should be classified as a hotspot");
    assert_eq!(evidence.primary_path(), "src/lib.rs");
    assert_eq!(
        evidence.evidence.get("quadrant"),
        Some(&auditor_core::EvidenceValue::String("hotspot".to_string()))
    );
    assert!(evidence.evidence.contains_key("loc_delta"));
    assert!(evidence.evidence.contains_key("sum_ccx"));
}

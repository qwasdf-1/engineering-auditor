use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{analyze_repository, RepositoryAnalysis, ScanProfile, SourceDiscoveryOptions};

fn scan(name: &str, relative: &str, source: &str, profile: ScanProfile) -> RepositoryAnalysis {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root: PathBuf = std::env::temp_dir().join(format!(
        "ea-suppressions-{name}-{}-{nanos}",
        std::process::id()
    ));
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    let analysis = analyze_repository(&root, SourceDiscoveryOptions::default(), profile).unwrap();
    let _ = std::fs::remove_dir_all(root);
    analysis
}

/// `(detector, line)` of every finding.
fn found(analysis: &RepositoryAnalysis) -> Vec<(String, usize)> {
    analysis
        .findings
        .iter()
        .map(|finding| {
            (
                finding.detector.clone(),
                finding.primary_line().unwrap_or(0),
            )
        })
        .collect()
}

const SHELL: &str =
    "import subprocess\n\n\ndef run(cmd):\n    return subprocess.call(cmd, shell=True)\n";

#[test]
fn a_reasoned_suppression_removes_the_finding_and_keeps_it_in_the_report() {
    let unsuppressed = scan("bare", "app/run.py", SHELL, ScanProfile::Gating);
    assert_eq!(found(&unsuppressed), [("shell_execution".to_string(), 5)]);
    let trailing = "import subprocess\n\n\ndef run(cmd):\n    return subprocess.call(cmd, shell=True)  # auditor-ignore[shell_execution]: admin tool; cmd is a fixed list\n";
    let above = "import subprocess\n\n\ndef run(cmd):\n    # auditor-ignore[shell_execution]: admin tool\n    return subprocess.call(cmd, shell=True)\n";
    for (name, source, line, reason) in [
        ("trailing", trailing, 5, "admin tool; cmd is a fixed list"),
        ("above", above, 6, "admin tool"),
    ] {
        let analysis = scan(name, "app/run.py", source, ScanProfile::Gating);
        assert_eq!(found(&analysis), Vec::<(String, usize)>::new(), "{name}");
        assert_eq!(analysis.suppressed.len(), 1, "{name}");
        let record = &analysis.suppressed[0];
        assert_eq!(
            (
                record.detector.as_str(),
                record.path.as_str(),
                record.line,
                record.reason.as_str()
            ),
            ("shell_execution", "app/run.py", line, reason),
            "{name}"
        );
        assert!(record.id.starts_with("EA-"), "{name}");
    }
}

#[test]
fn invalid_suppressions_are_reported_and_suppress_nothing_without_a_reason() {
    // A missing reason keeps the finding; an unknown detector and a
    // suppression that matches nothing are reported where they stand.
    let source = "import subprocess\n\n\ndef run(cmd):\n    return subprocess.call(cmd, shell=True)  # auditor-ignore[shell_execution]\n\n\ndef later():\n    # auditor-ignore[no_such_detector]: typo\n    pass\n    # auditor-ignore[shell_execution]: nothing here runs a shell\n    return 1\n";
    let analysis = scan("invalid", "app/run.py", source, ScanProfile::Gating);
    let messages = analysis
        .findings
        .iter()
        .map(|finding| (finding.detector.as_str(), finding.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            (
                "invalid_suppression",
                "`auditor-ignore` at line 5 gives no reason, so it suppresses nothing."
            ),
            ("shell_execution", "Shell execution at line 5"),
            (
                "invalid_suppression",
                "`auditor-ignore` at line 9 names `no_such_detector`, which is not a detector."
            ),
            (
                "invalid_suppression",
                "`auditor-ignore` at line 11 matches no `shell_execution` finding."
            ),
        ]
    );
    assert!(analysis.suppressed.is_empty());
}

#[test]
fn suppressions_are_judged_only_where_their_detector_reports() {
    // `sql_injection` is full-only: a gating scan cannot call its
    // suppression unused, and a full scan honours it.
    let source = "def find(cur, name):\n    # auditor-ignore[sql_injection]: name is a validated enum value\n    cur.execute(\"SELECT * FROM t WHERE name = '%s'\" % name)\n";
    let gating = scan("gating", "app/find.py", source, ScanProfile::Gating);
    assert_eq!(found(&gating), Vec::<(String, usize)>::new());
    let full = scan("full", "app/find.py", source, ScanProfile::Full);
    assert!(
        !found(&full)
            .iter()
            .any(|(detector, _)| detector == "sql_injection" || detector == "invalid_suppression"),
        "{:?}",
        found(&full)
    );
    assert_eq!(full.suppressed.len(), 1);
}

#[test]
fn markers_outside_comments_or_in_documents_are_not_suppressions() {
    let code = "MARKER = \"# auditor-ignore[shell_execution]: quoted, not a comment\"\n";
    assert_eq!(
        found(&scan("literal", "app/marker.py", code, ScanProfile::Gating)),
        Vec::<(String, usize)>::new()
    );
    let doc = "# Guide\n\nWrite `# auditor-ignore[shell_execution]: why` above the call.\n";
    assert_eq!(
        found(&scan("doc", "docs/guide.md", doc, ScanProfile::Gating)),
        Vec::<(String, usize)>::new()
    );
}

#[test]
fn each_detector_a_suppression_names_must_match() {
    let source = "import subprocess\n\n\ndef run(cmd):\n    # auditor-ignore[shell_execution, eval_in_production]: admin tool\n    return subprocess.call(cmd, shell=True)\n";
    let analysis = scan("multiple", "app/run.py", source, ScanProfile::Gating);
    assert_eq!(
        analysis
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>(),
        ["`auditor-ignore` at line 5 matches no `eval_in_production` finding."]
    );
    assert_eq!(analysis.suppressed.len(), 1);
}

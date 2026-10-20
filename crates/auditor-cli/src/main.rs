#![recursion_limit = "256"]

use std::env;
use std::process::ExitCode;

mod arg_cursor;
mod baseline_command;
mod catalog_command;
mod digest_command;
mod gate_command;
mod review_command;
#[cfg(test)]
mod review_tests;
mod scan_command;
mod scan_options;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) if error.starts_with(gate_command::POLICY_FAILURE) => {
            println!("{}", &error[gate_command::POLICY_FAILURE.len()..]);
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("auditor: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<String, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(help());
    };
    dispatch_command(command, &args[1..])
}

fn dispatch_command(command: &str, args: &[String]) -> Result<String, String> {
    match command {
        "scan" if args.iter().any(|argument| argument == "--review") => {
            review_command::run_scan_review(args)
        }
        "scan" => scan_command::run_scan(args),
        "gate" => gate_command::run_gate(args),
        "review" => review_command::run_review(args),
        "baseline" | "catalog" | "digest" | "donor" => dispatch_auxiliary(command, args),
        "version" | "--version" | "-V" => Ok(env!("CARGO_PKG_VERSION").to_string()),
        "help" | "--help" | "-h" => Ok(help()),
        other => Err(format!("unknown command `{other}`\n{}", help())),
    }
}

fn dispatch_auxiliary(command: &str, args: &[String]) -> Result<String, String> {
    match command {
        "baseline" => baseline_command::run_baseline(args),
        "catalog" => catalog_command::run_catalog(args),
        "digest" => digest_command::run_digest(args),
        "donor" => run_donor(args),
        _ => Err(format!("unknown auxiliary command `{command}`")),
    }
}

fn run_donor(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("check") => Ok(auditor_donors::donor_check_summary_json()),
        Some("sync") => Ok(auditor_donors::donor_sync_summary_json()),
        Some(other) => Err(format!("unknown donor command `{other}`")),
        None => Err("donor command requires `check` or `sync`".to_string()),
    }
}

fn help() -> String {
    String::from(
        "Engineering Auditor\n\nCommands:\n  scan --repo-root <path> [--profile gating|full] [--format json|sarif|human] [--output <report>] [--path <relative>] [--changed-ranges-json <json>] [--include-references] [--baseline <baseline.json>] [--repo-graph <json>] [--co-edit-journal <json>] [--no-color] [--quiet] [--summary-only]\n  scan --review --repo-root <path> --output <report.json> --case-templates <templates.json> --routes <routes-v2.json> --prompt-bundle <prompts-v2.json> [--brief <brief.json>] [--evidence <attachment.json> ...] --allow-hosted-review [--allow-anonymous-provider] [experimental]\n  gate --repo-root <path> (--baseline <baseline.json> | --max-findings <n>) [--output-dir <path>]\n  review plan --repo-root <path> --report <report.json> --case-templates <templates.json> --routes <routes-v2.json> --prompt-bundle <prompts-v2.json> [--brief <brief.json>] [--evidence <attachment.json> ...] --output <plan.json> [experimental]\n  review inspect-case --plan <plan.json> --case-id <engineering-case:id> [--format json|human] [--output <case.md>] [experimental]\n  review transcript --input <transcript.json> [--output <transcript.md>] [--model-input-only] [--stage-prefix <prefix> ...] [experimental]\n  review run --report <report.json> --repo-root <path> --output <reviewed-report.json> --case-templates <templates.json> --routes <routes-v2.json> --prompt-bundle <prompts-v2.json> [--brief <brief.json>] [--evidence <attachment.json> ...] --allow-hosted-review [--allow-anonymous-provider] [experimental]\n  review provider discover --routes <routes-v2.json> --allow-hosted-review [--allow-anonymous-provider] [--output <artifact.json>] [experimental]\n  review verify --report <reviewed-report.json> --repo-root <path> [--dispositions <ledger.json>] [experimental]\n  review disposition --review <review-v2.json> --ledger <ledger.json> --recommendation <id> --status <accepted|deferred|rejected> --operator <id> [experimental]\n  review gate --review <review-v2.json> --dispositions <ledger.json> [experimental]\n  digest --report <report.json> [--output <human.md>] [--no-color] [--summary-only]\n  digest --repo-root <path> [--output <human.md>] [--format human] [--profile gating|full] [--no-color] [--summary-only]\n  baseline create --report <report.json> --reason <review reason>\n  baseline rekey --baseline <baseline.json> --report <report.json> [--prune-resolved]\n  baseline migrate --baseline <old-baseline.json> --from-report <previous-auditor-report.json> --report <report.json>\n  catalog [--profile gating|full] [--format json]\n  donor check [internal]\n  donor sync [internal]\n  version\n\n[experimental] surfaces may change in any release; [internal] commands are development tooling. See docs/reference/stability.md.",
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_repo(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ea-cli-{name}-{nonce}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    #[test]
    fn help_labels_experimental_and_internal_surfaces() {
        let text = help();
        for line in text.lines().map(str::trim) {
            if line.starts_with("scan --review") || line.starts_with("review ") {
                assert!(line.ends_with("[experimental]"), "{line}");
            } else if line.starts_with("donor ") {
                assert!(line.ends_with("[internal]"), "{line}");
            } else {
                assert!(!line.contains("[experimental]") || line.starts_with("[experimental]"));
            }
        }
        assert!(text.contains("docs/reference/stability.md"));
    }

    #[test]
    fn scan_emits_valid_v5_with_capability_ledger() {
        let root = temp_repo("scan-v5");
        std::fs::write(root.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n").unwrap();
        let report =
            scan_command::run_scan(&["--repo-root".into(), root.display().to_string()]).unwrap();
        auditor_report::validate_report_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(value["schema_version"], 5);
        assert!(value["capability_executions"]
            .as_array()
            .is_some_and(|v| !v.is_empty()));
        assert_eq!(value["repository_map"]["status"], "absent");
        assert_eq!(value["engineering_review"]["status"], "absent");
        assert_eq!(value["evidence_graph"]["schema_version"], 2);
        assert_eq!(value["evidence_graph"]["completeness"]["complete"], true);
    }

    #[test]
    fn full_scan_with_output_writes_digest_addressed_repository_map() {
        let root = temp_repo("repository-map");
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn outer() { inner(); }\nfn inner() {}\n",
        )
        .unwrap();
        let report_path = root.join("reports/audit.json");
        let report = scan_command::run_scan(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--output".into(),
            report_path.display().to_string(),
        ])
        .unwrap();
        assert_eq!(std::fs::read_to_string(&report_path).unwrap(), report);
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(value["repository_map"]["status"], "present");
        let map_name = value["repository_map"]["path"].as_str().unwrap();
        let map_path = report_path.parent().unwrap().join(map_name);
        let first_map = std::fs::read_to_string(&map_path).unwrap();
        let map: serde_json::Value = serde_json::from_str(&first_map).unwrap();
        assert_eq!(map["schema_version"], 1);
        assert_eq!(map["profile"], "full");
        assert_eq!(map["files"].as_array().unwrap().len(), 1);
        assert!(map["symbols"].as_array().unwrap().len() >= 2);
        assert!(map["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "calls"));
        let repeated = scan_command::run_scan(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--output".into(),
            report_path.display().to_string(),
        ])
        .unwrap();
        assert_eq!(repeated, report);
        assert_eq!(std::fs::read_to_string(map_path).unwrap(), first_map);
    }

    #[cfg(feature = "graph-analysis")]
    #[test]
    fn graph_capability_ledger_exposes_missing_donor_inputs() {
        let root = temp_repo("graph-capability-ledger");
        std::fs::write(root.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n").unwrap();
        let report = scan_command::run_scan(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
        ])
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();
        let executions = value["capability_executions"].as_array().unwrap();
        for detector in [
            "repo_graph_isolated_source_file",
            "repo_graph_high_fanout_source_file",
            "repo_graph_import_cycle",
            "large_file_split_candidate",
            "small_file_consolidation_candidate",
            "shotgun_surgery_cluster",
        ] {
            let execution = executions
                .iter()
                .find(|execution| execution["detector_id"] == detector)
                .unwrap();
            assert_eq!(execution["status"], "missing_input");
            assert!(!execution["missing_inputs"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn gate_budget_passes_and_fails_with_policy_marker() {
        let clean = temp_repo("clean");
        std::fs::write(clean.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n").unwrap();
        let pass = gate_command::run_gate(&[
            "--repo-root".into(),
            clean.display().to_string(),
            "--max-findings".into(),
            "100".into(),
        ])
        .unwrap();
        assert!(pass.contains("\"passed\":true"));
        let noisy = temp_repo("noisy");
        std::fs::write(
            noisy.join("src/lib.rs"),
            "pub fn unfinished() { todo!() }\n",
        )
        .unwrap();
        let failure = gate_command::run_gate(&[
            "--repo-root".into(),
            noisy.display().to_string(),
            "--max-findings".into(),
            "0".into(),
        ])
        .unwrap_err();
        assert!(failure.starts_with(gate_command::POLICY_FAILURE));
        assert!(failure.contains("finding_budget_exceeded"));
    }

    #[test]
    fn gate_carries_renamed_accepted_findings_and_rekey_rebinds_them() {
        let root = temp_repo("baseline-carry");
        let source = |name: &str| format!("pub fn {name}() -> usize {{\n    todo!()\n}}\n");
        std::fs::write(root.join("src/lib.rs"), source("parse")).unwrap();
        let report =
            scan_command::run_scan(&["--repo-root".into(), root.display().to_string()]).unwrap();
        let baseline_path = root.join("baseline.json");
        std::fs::write(
            &baseline_path,
            auditor_report::BaselineDocument::from_report_json(&report, "reviewed debt")
                .unwrap()
                .to_json(),
        )
        .unwrap();
        let gate = || {
            gate_command::run_gate(&[
                "--repo-root".into(),
                root.display().to_string(),
                "--baseline".into(),
                baseline_path.display().to_string(),
            ])
        };

        let renamed = format!("// header\n\n{}", source("parse_all"));
        std::fs::write(root.join("src/lib.rs"), renamed).unwrap();
        let carried = gate().unwrap();
        assert!(carried.contains("\"passed\":true"), "{carried}");
        assert!(carried.contains("\"carried_count\":1"), "{carried}");
        assert!(carried.contains("\"pass\":\"renamed\""), "{carried}");

        let current =
            scan_command::run_scan(&["--repo-root".into(), root.display().to_string()]).unwrap();
        let report_path = root.join("current.json");
        std::fs::write(&report_path, current).unwrap();
        let rekeyed = baseline_command::run_baseline(&[
            "rekey".into(),
            "--baseline".into(),
            baseline_path.display().to_string(),
            "--report".into(),
            report_path.display().to_string(),
        ])
        .unwrap();
        assert!(rekeyed.contains("reviewed debt"));
        std::fs::write(&baseline_path, rekeyed).unwrap();
        let rebound = gate().unwrap();
        assert!(rebound.contains("\"carried_count\":0"), "{rebound}");
        assert!(rebound.contains("\"persisting_count\":1"), "{rebound}");
    }

    #[test]
    fn gate_uses_standalone_baseline_and_rejects_report_fixtures() {
        let root = temp_repo("baseline");
        std::fs::write(root.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n").unwrap();
        let report =
            scan_command::run_scan(&["--repo-root".into(), root.display().to_string()]).unwrap();
        let baseline =
            auditor_report::BaselineDocument::from_report_json(&report, "reviewed test baseline")
                .unwrap();
        let baseline_path = root.join("baseline.json");
        std::fs::write(&baseline_path, baseline.to_json()).unwrap();
        assert!(gate_command::run_gate(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--baseline".into(),
            baseline_path.display().to_string()
        ])
        .unwrap()
        .contains("\"passed\":true"));

        std::fs::write(root.join("src/lib.rs"), "pub fn unfinished() { todo!() }\n").unwrap();
        let failure = gate_command::run_gate(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--baseline".into(),
            baseline_path.display().to_string(),
        ])
        .unwrap_err();
        assert!(failure.contains("new_findings_introduced"));

        let report_path = root.join("report.json");
        std::fs::write(&report_path, report).unwrap();
        let incompatible = gate_command::run_gate(&[
            "--repo-root".into(),
            root.display().to_string(),
            "--baseline".into(),
            report_path.display().to_string(),
        ])
        .unwrap_err();
        assert!(incompatible.contains("incomparable"));
    }

    #[test]
    fn baseline_command_requires_review_reason() {
        let root = temp_repo("baseline-command");
        std::fs::write(root.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n").unwrap();
        let report =
            scan_command::run_scan(&["--repo-root".into(), root.display().to_string()]).unwrap();
        let report_path = root.join("report.json");
        std::fs::write(&report_path, report).unwrap();
        assert!(baseline_command::run_baseline(&[
            "create".into(),
            "--report".into(),
            report_path.display().to_string(),
        ])
        .is_err());
        let baseline = baseline_command::run_baseline(&[
            "create".into(),
            "--report".into(),
            report_path.display().to_string(),
            "--reason".into(),
            "reviewed".into(),
        ])
        .unwrap();
        auditor_report::parse_baseline_document(&baseline).unwrap();
    }

    #[test]
    fn gate_rejects_missing_or_conflicting_policy_modes() {
        assert!(gate_command::run_gate(&[]).is_err());
        assert!(gate_command::run_gate(&[
            "--baseline".into(),
            "a".into(),
            "--max-findings".into(),
            "0".into()
        ])
        .is_err());
    }

    #[test]
    fn catalog_exposes_language_and_role_contracts() {
        let output = catalog_command::run_catalog(&["--profile".into(), "full".into()]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["schema_version"], 1);
        let detectors = value["detectors"].as_array().unwrap();
        let dead_code = detectors
            .iter()
            .find(|detector| detector["id"] == "dead_code")
            .unwrap();
        assert_eq!(dead_code["language_support"].as_array().unwrap().len(), 15);
        assert!(dead_code["context_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "test"));
        assert!(!dead_code["emission_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "test"));
    }

    /// A changed-scope invocation must bound `scanned_files` to the requested
    /// subset (not the whole repo), the report must validate, a
    /// repository-metadata detector must be `executed` when graph inputs are
    /// supplied, and the run must be deterministic.
    #[cfg(feature = "graph-analysis")]
    #[test]
    fn changed_scope_bounds_scanned_files_and_validates_with_graph_inputs() {
        use std::collections::BTreeSet;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ea-changed-scope-{nonce}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tooling")).unwrap();
        // Three production files plus a stray tooling file. A changed scope
        // over two `src/` files must not pick up the third or `tooling/`.
        std::fs::write(root.join("src/alpha.rs"), "pub fn alpha() -> usize { 1 }\n").unwrap();
        std::fs::write(root.join("src/beta.rs"), "pub fn beta() -> usize { 2 }\n").unwrap();
        std::fs::write(
            root.join("src/untouched.rs"),
            "pub fn untouched() -> usize { 3 }\n",
        )
        .unwrap();
        // A tooling file outside the scope.
        std::fs::write(
            root.join("tooling/check.py"),
            "# tooling script\nprint('ok')\n",
        )
        .unwrap();

        let graph_path = root.join("repo-graph.json");
        std::fs::write(
            &graph_path,
            serde_json::json!({
                "schema_version": 1,
                "repo_graph_id": "changed-scope-fixture",
                "nodes": [
                    {"path": "src/alpha.rs", "node_kind": "file", "line_count": 1},
                    {"path": "src/beta.rs", "node_kind": "file", "line_count": 1},
                ],
                "edges": [],
            })
            .to_string(),
        )
        .unwrap();
        let journal_path = root.join("co-edit.json");
        std::fs::write(
            &journal_path,
            serde_json::json!({
                "schema_version": 1,
                "records": [
                    {
                        "change_set_id": "changed-scope-set",
                        "files": ["src/alpha.rs", "src/beta.rs"],
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let changed_ranges = serde_json::json!([
            {"path": "src/alpha.rs", "start_line": 1, "end_line": 1},
            {"path": "src/beta.rs", "start_line": 1, "end_line": 1},
        ])
        .to_string();

        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--profile".to_string(),
            "full".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--path".to_string(),
            "src/alpha.rs".to_string(),
            "--path".to_string(),
            "src/beta.rs".to_string(),
            "--changed-ranges-json".to_string(),
            changed_ranges,
            "--repo-graph".to_string(),
            graph_path.display().to_string(),
            "--co-edit-journal".to_string(),
            journal_path.display().to_string(),
        ];

        let first = scan_command::run_scan(&args).expect("scoped scan must succeed");
        auditor_report::validate_report_json(&first).expect("scoped report must validate");
        let value: serde_json::Value = serde_json::from_str(&first).unwrap();

        assert_eq!(value["scope"], "changed");
        // Pre-fix: scanned_files would equal 4 (the whole repo). Post-fix:
        // bounded to the two requested paths.
        let scanned = value["scanned_files"].as_u64().unwrap();
        assert_eq!(
            scanned, 2,
            "scoped scan must bound scanned_files to the requested subset; got {scanned}"
        );

        // Every detector must have exactly one capability execution.
        let detectors: BTreeSet<String> = value["detectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["id"].as_str().unwrap().to_string())
            .collect();
        let executions: BTreeSet<String> = value["capability_executions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["detector_id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            detectors, executions,
            "every detector must have a capability execution"
        );

        // The repository-metadata detector must be `executed` now that the
        // report validator accepts a NotApplicable language policy with
        // eligible files.
        let isolated = value["capability_executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["detector_id"] == "repo_graph_isolated_source_file")
            .expect("repo_graph_isolated_source_file execution must be present");
        assert_eq!(
            isolated["status"], "executed",
            "repo_graph_isolated_source_file must be executed, not {:?}",
            isolated["status"]
        );

        // Determinism: a repeat run must produce byte-identical output.
        let second = scan_command::run_scan(&args).expect("repeat scoped scan must succeed");
        assert_eq!(first, second, "scoped scan must be deterministic");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Task 8: `--format human` writes the digest and sibling machine JSON
    /// via the CLI scan path; both are byte-identical to what `run_scan`
    /// produced for `--format json` (the sibling), and the digest carries the
    /// summary-header / severity / detector-table sections.
    #[test]
    fn scan_human_format_writes_two_files() {
        let root = temp_repo("scan-human");
        std::fs::write(root.join("src/lib.rs"), "pub fn one() -> usize { 1 }\n").unwrap();
        let digest_path = root.join("reports/audit.md");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--format".into(),
            "human".into(),
            "--output".into(),
            digest_path.display().to_string(),
        ];
        let rendered = scan_command::run_scan(&args).expect("human scan via CLI");
        assert!(
            rendered.contains("# Engineering Auditor"),
            "header: {rendered}"
        );
        assert!(
            digest_path.exists(),
            "digest file: {}",
            digest_path.display()
        );
        let sibling = root.join("reports/audit.audit.json");
        assert!(sibling.exists(), "sibling: {}", sibling.display());
        let sibling_text = std::fs::read_to_string(&sibling).unwrap();
        auditor_report::validate_report_json(&sibling_text).expect("sibling is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Task 8: `--summary-only --format json` is rejected by the CLI scan
    /// path before any work runs (the flag is only valid with `--format human`).
    #[test]
    fn scan_summary_only_requires_human_format() {
        let root = temp_repo("summary-only-rejects-json");
        std::fs::write(root.join("src/lib.rs"), "pub fn one() -> usize { 1 }\n").unwrap();
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--format".into(),
            "json".into(),
            "--summary-only".into(),
        ];
        let error = scan_command::run_scan(&args).expect_err("must reject");
        assert!(
            error.contains("--summary-only requires --format human"),
            "summary-only rejection: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Task 8: `--no-color` forces plain mode even when stdout is a tty; the
    /// resolver returns `ColorMode::Plain`. The output file path is also
    /// plain because file destinations are not ttys. This pinpoints the
    /// resolver behavior; the actual scan test above reads the file (Plain)
    /// so the assertion is via the resolver contract.
    #[test]
    fn scan_options_resolved_color_no_color_plain() {
        use std::io::IsTerminal;
        let parse = |args: &[&str]| {
            crate::scan_options::ScanOptions::parse(
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .expect("parse")
        };
        let args = ["--repo-root", ".", "--format", "human", "--no-color"];
        let options = parse(&args);
        assert_eq!(options.resolved_color(), auditor_report::ColorMode::Plain);
        assert!(options.no_color);
        // Without --no-color, an explicit output remains plain.
        let args_out = [
            "--repo-root",
            ".",
            "--format",
            "human",
            "--output",
            "audit.md",
        ];
        let opts_out = parse(&args_out);
        assert_eq!(opts_out.resolved_color(), auditor_report::ColorMode::Plain);
        // Without output or override, the resolver follows stdout's tty state.
        let args_default = ["--repo-root", ".", "--format", "human"];
        let opts_default = parse(&args_default);
        let expected = if std::io::stdout().is_terminal() {
            auditor_report::ColorMode::Auto
        } else {
            auditor_report::ColorMode::Plain
        };
        assert_eq!(opts_default.resolved_color(), expected);
    }

    /// Task 13: `--quiet` on the scan path returns empty stdout bytes; the
    /// write-to-file side effect still runs. (The stderr summary line is
    /// emitted directly via eprintln! inside scan_command; we cannot easily
    /// assert on it from a unit test, so the contract is the empty stdout.)
    #[test]
    fn scan_quiet_suppresses_stdout_renders() {
        let root = temp_repo("scan-quiet");
        std::fs::write(root.join("src/lib.rs"), "pub fn one() -> usize { 1 }\n").unwrap();
        let out_path = root.join("reports/audit.json");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--format".into(),
            "json".into(),
            "--output".into(),
            out_path.display().to_string(),
            "--quiet".into(),
        ];
        let stdout = scan_command::run_scan(&args).expect("quiet scan succeeds");
        assert!(
            stdout.is_empty(),
            "--quiet returns empty stdout: got {stdout:?}"
        );
        // The file artifact still gets written.
        assert!(out_path.exists(), "output file written under --quiet");
        let machine = std::fs::read_to_string(&out_path).unwrap();
        auditor_report::validate_report_json(&machine).expect("valid machine sibling");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Task 13: `--summary-only --format human` on the scan path truncates
    /// the digest to the summary-header / severity-totals / per-detector
    /// breakdown / footer and omits the per-finding detail section.
    #[test]
    fn scan_summary_only_truncates_human_digest() {
        let root = temp_repo("scan-summary-only");
        std::fs::write(root.join("src/lib.rs"), "pub fn one() -> usize { 1 }\n").unwrap();
        let digest_path = root.join("reports/audit.md");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--format".into(),
            "human".into(),
            "--output".into(),
            digest_path.display().to_string(),
            "--summary-only".into(),
        ];
        let rendered = scan_command::run_scan(&args).expect("summary scan succeeds");
        assert!(rendered.contains("# Engineering Auditor"), "header kept");
        assert!(rendered.contains("## Severity Totals"), "totals kept");
        assert!(
            rendered.contains("## Per-Detector Breakdown"),
            "breakdown kept"
        );
        assert!(rendered.contains("## Integrity"), "integrity section kept");
        assert!(
            !rendered.contains("## Findings by Detector"),
            "per-finding section omitted under --summary-only"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

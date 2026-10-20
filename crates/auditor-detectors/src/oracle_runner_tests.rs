use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{analyze_repository, Finding, ScanProfile, SourceDiscoveryOptions};
#[cfg(feature = "graph-analysis")]
use crate::{
    analyze_repository_with_provider, GraphInputs, RepoGraph, RepoGraphEdge, RepoGraphNode,
};

#[path = "oracle_architecture_adapter_tests.rs"]
mod architecture_adapters;
#[path = "oracle_java_adapter_tests.rs"]
mod java_adapters;
#[path = "oracle_node_adapter_tests.rs"]
mod node_adapters;
#[path = "oracle_python_adapter_tests.rs"]
mod python_adapters;

static TEMP_PROJECT_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
const ORACLE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const ORACLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug)]
pub(crate) enum OracleAdapter {
    AgentSlopLint,
    RustClippy,
    Bandit,
    Ruff,
    Pmd,
    Eslint,
    GolangCiLint,
    Jscpd,
    Detekt,
    Pylint,
    ImportLinter,
    DependencyCruiser,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ParityCase {
    pub donor_id: &'static str,
    pub donor_rule: &'static str,
    pub ea_detector: &'static str,
    pub relative_path: &'static str,
    pub source: &'static str,
    pub profile: ScanProfile,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MatchKey {
    detector: String,
    path: String,
    line: Option<usize>,
}

#[derive(Clone, Debug)]
struct OracleFinding {
    donor_rule: String,
    key: MatchKey,
}

#[derive(Debug)]
struct ParityComparison {
    oracle_count: usize,
    ea_count: usize,
    matched_count: usize,
    recall: f64,
    precision: f64,
    unmatched_oracle: Vec<OracleFinding>,
    unmatched_ea: Vec<MatchKey>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CapabilityStatus {
    LiveOracle {
        command: &'static str,
    },
    NotParity {
        reason: &'static str,
        eventual_command: &'static str,
        capability_alternative: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DonorCapabilityStatus {
    pub id: &'static str,
    pub status: CapabilityStatus,
}

pub(crate) const DONOR_CAPABILITY_STATUSES: &[DonorCapabilityStatus] = &[
    DonorCapabilityStatus {
        id: "agent-slop-lint",
        status: CapabilityStatus::LiveOracle {
            command: "EA_AGENT_SLOP_PYTHON=<provisioned-python> PYTHONPATH=references/agent-slop-lint/src/cli <python> -c <adapter> <rule> <repo>",
        },
    },
    DonorCapabilityStatus {
        id: "rust-clippy",
        status: CapabilityStatus::LiveOracle {
            command: "CARGO_NET_OFFLINE=true cargo clippy --manifest-path <repo>/Cargo.toml --message-format=json -- -W <rule>",
        },
    },
    DonorCapabilityStatus {
        id: "ruff",
        status: CapabilityStatus::LiveOracle {
            command: "EA_RUFF_BIN=<ruff-0.15.20> <binary> check --no-cache --output-format json --select <rule> <file>",
        },
    },
    DonorCapabilityStatus {
        id: "eslint",
        status: CapabilityStatus::LiveOracle {
            command: "EA_ESLINT_BIN=<eslint-10.6.0> <binary> --no-config-lookup --format json --rule no-duplicate-imports:error <file>",
        },
    },
    DonorCapabilityStatus {
        id: "semgrep",
        status: CapabilityStatus::NotParity {
            reason: "EA has no general custom-rule engine, so running ad hoc Semgrep rules would only restate EA lexical tests",
            eventual_command: "semgrep --json --metrics=off --disable-version-check --config <rules.yml> <repo>",
            capability_alternative: "unit and integration tests for EA's eval and shell-execution detectors until a custom-rule engine exists",
        },
    },
    DonorCapabilityStatus {
        id: "pmd",
        status: CapabilityStatus::LiveOracle {
            command: "EA_PMD_JAVA=<java-21> EA_PMD_CLASSPATH=<pmd-libs> <java> -cp <classpath> net.sourceforge.pmd.cli.PmdCli check -d <file> -f json -R <rule>",
        },
    },
    DonorCapabilityStatus {
        id: "detekt",
        status: CapabilityStatus::LiveOracle {
            command: "EA_DETEKT_JAR=<detekt-cli-all.jar> java -jar <jar> --input <repo> --all-rules --report sarif:<report>",
        },
    },
    DonorCapabilityStatus {
        id: "golangci-lint",
        status: CapabilityStatus::LiveOracle {
            command: "EA_GOLANGCI_BIN=<golangci-lint> <binary> run --no-config --enable-only <linter> --output.json.path stdout <repo>",
        },
    },
    DonorCapabilityStatus {
        id: "pylint",
        status: CapabilityStatus::LiveOracle {
            command: "EA_PYLINT_BIN=<pylint-4.1.0-dev0> <binary> --output-format=json2 --disable=all --enable=<rule> <file>",
        },
    },
    DonorCapabilityStatus {
        id: "jscpd",
        status: CapabilityStatus::LiveOracle {
            command: "EA_JSCPD_BIN=<jscpd-4.2.5> <binary> --silent --reporters json --min-lines 8 --min-tokens 50 <repo>",
        },
    },
    DonorCapabilityStatus {
        id: "deptrac",
        status: CapabilityStatus::NotParity {
            reason: "Deptrac is PHP-specific and PHP is outside EA's declared language roadmap",
            eventual_command: "php references/deptrac/deptrac.phar analyse --config-file=<deptrac.yaml> --formatter=json",
            capability_alternative: "resolved local-topology capability tests and live Import Linter/Dependency Cruiser parity cover EA's declared architecture languages",
        },
    },
    DonorCapabilityStatus {
        id: "bandit",
        status: CapabilityStatus::LiveOracle {
            command: "EA_BANDIT_PYTHON=<provisioned-python> PYTHONPATH=references/bandit <python> -m bandit -q -f json -t <rule> -r <repo>",
        },
    },
    DonorCapabilityStatus {
        id: "import-linter",
        status: CapabilityStatus::LiveOracle {
            command: "EA_IMPORT_LINTER_BIN=<import-linter-2.13> <binary> --config <importlinter.ini> --contract ea-layers --no-cache",
        },
    },
    DonorCapabilityStatus {
        id: "dependency-cruiser",
        status: CapabilityStatus::LiveOracle {
            command: "EA_DEPCRUISE_BIN=<dependency-cruiser-18.1.0> <binary> --config <config.cjs> --output-type json <files>",
        },
    },
];

pub(crate) fn assert_live_case(adapter: OracleAdapter, case: ParityCase) {
    let project = TempProject::new(case.donor_id);
    prepare_project(&project, adapter, case, case.source);

    let positive_ea = ea_findings(&project.root, case);
    let oracle_findings = run_oracle(adapter, &project.root, case);

    assert!(
        !oracle_findings.is_empty(),
        "{} {} produced no selected donor findings",
        case.donor_id,
        case.donor_rule
    );
    assert!(
        oracle_findings
            .iter()
            .all(|finding| finding.donor_rule == case.donor_rule),
        "{} returned a rule outside the selected capability: {oracle_findings:#?}",
        case.donor_id
    );
    assert!(
        !positive_ea.is_empty(),
        "EA detector {} produced no findings for {} {}",
        case.ea_detector,
        case.donor_id,
        case.donor_rule
    );

    let comparison = compare_one_to_one(&oracle_findings, &positive_ea);
    assert_eq!(
        comparison.recall, 1.0,
        "{} {} recall was not exact: {comparison:#?}",
        case.donor_id, case.donor_rule
    );
    assert_eq!(
        comparison.precision, 1.0,
        "{} {} precision was not exact: {comparison:#?}",
        case.donor_id, case.donor_rule
    );
    assert_eq!(comparison.matched_count, comparison.oracle_count);
    assert_eq!(comparison.matched_count, comparison.ea_count);
    assert!(comparison.unmatched_oracle.is_empty());
    assert!(comparison.unmatched_ea.is_empty());

    let clean = TempProject::new(&format!("{}-clean", case.donor_id));
    prepare_project(&clean, adapter, case, clean_source(adapter));
    let clean_ea = ea_findings(&clean.root, case);
    let clean_oracle = run_oracle(adapter, &clean.root, case);
    assert!(
        clean_oracle.is_empty(),
        "{} {} produced donor findings for its clean case: {clean_oracle:#?}",
        case.donor_id,
        case.donor_rule
    );
    assert!(
        clean_ea.is_empty(),
        "EA detector {} produced findings for the {} {} clean case: {clean_ea:#?}",
        case.ea_detector,
        case.donor_id,
        case.donor_rule
    );
}

pub(crate) fn assert_precision_extension_noise(adapter: OracleAdapter, case: ParityCase) {
    let project = TempProject::new(&format!("{}-precision", case.donor_id));
    prepare_project(&project, adapter, case, case.source);
    let ea = ea_findings(&project.root, case);
    let donor = run_oracle(adapter, &project.root, case);
    assert!(
        !donor.is_empty(),
        "{} {} did not reproduce the documented donor-noise case",
        case.donor_id,
        case.donor_rule
    );
    assert!(
        ea.is_empty(),
        "EA detector {} did not preserve its precision extension: {ea:#?}",
        case.ea_detector
    );
}

fn prepare_project(project: &TempProject, adapter: OracleAdapter, case: ParityCase, source: &str) {
    project.write(case.relative_path, source);
    if case.donor_rule == "structural.deps" {
        project.write(
            "b.py",
            if source.contains("import b") {
                "import a\n"
            } else {
                "def clean(): return 1\n"
            },
        );
    }
    if case.donor_rule == "structural.packages" && source.contains("class Model") {
        project.write("core/__init__.py", "");
        for index in 1..=3 {
            project.write(&format!("client{index}/__init__.py"), "");
            project.write(
                &format!("client{index}/use.py"),
                &format!("from core.model import Model\nclass Use{index}:\n    def use(self, value: Model): return value\n"),
            );
        }
    }
    if case.donor_rule == "structural.hotspots" {
        let positive = source.contains("def hot");
        if positive {
            for index in 0..8 {
                project.write(
                    &format!("filler{index}.py"),
                    &format!("def filler{index}():\n    pass\n"),
                );
            }
        }
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "EA Test"],
            vec!["config", "user.email", "ea@example.com"],
            vec!["add", "."],
            vec!["commit", "-q", "-m", "initial"],
        ] {
            let output = Command::new("git")
                .args(args)
                .current_dir(&project.root)
                .output()
                .expect("run git for hotspot parity");
            assert!(
                output.status.success(),
                "hotspot parity git setup failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if positive {
            project.write(case.relative_path, &format!("{source}\n# growth\n"));
            for index in 0..8 {
                project.write(
                    &format!("filler{index}.py"),
                    &format!("def filler{index}():\n    return {index}\n"),
                );
            }
            for args in [vec!["add", "."], vec!["commit", "-q", "-m", "growth"]] {
                let output = Command::new("git")
                    .args(args)
                    .current_dir(&project.root)
                    .output()
                    .expect("commit hotspot parity history");
                assert!(
                    output.status.success(),
                    "hotspot parity git commit failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
    if matches!(adapter, OracleAdapter::RustClippy) {
        let dependencies = if case.donor_rule == "clippy::regex_creation_in_loops" {
            "\n[dependencies]\nregex = \"1\"\n"
        } else {
            ""
        };
        project.write(
            "Cargo.toml",
            &format!(
                "[package]\nname = \"ea-donor-parity\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n{dependencies}"
            ),
        );
    }
    if matches!(adapter, OracleAdapter::GolangCiLint) {
        project.write("go.mod", "module example.com/ea-donor-parity\n\ngo 1.25\n");
    }
    if matches!(adapter, OracleAdapter::ImportLinter) {
        project.write("domain/__init__.py", "");
        project.write("domain/model.py", "class User:\n    pass\n");
        project.write("app/__init__.py", "");
        project.write("app/service.py", "class UserService:\n    pass\n");
        project.write(
            ".importlinter",
            "[importlinter]\nroot_packages =\n    domain\n    app\n\n[importlinter:contract:ea-layers]\nname = EA layers\ntype = forbidden\nsource_modules = domain\nforbidden_modules = app\n",
        );
        project.write(
            "auditor.toml",
            "[layering.layer.domain]\nglob = domain/**/*.py\n\n[layering.layer.app]\nglob = app/**/*.py\n\n[layering.rule.domain]\ndeny = app\n",
        );
    }
    if matches!(adapter, OracleAdapter::DependencyCruiser) {
        project.write(
            "b.js",
            if source.contains("donor-cycle") {
                "import { a } from './a.js';\nexport const b = a;\n"
            } else {
                "export const b = 1;\n"
            },
        );
        project.write(
            ".dependency-cruiser.cjs",
            "module.exports = { forbidden: [{ name: 'no-circular', severity: 'error', from: {}, to: { circular: true } }], options: { doNotFollow: { path: 'node_modules' } } };\n",
        );
    }
}

fn run_oracle(adapter: OracleAdapter, root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    match adapter {
        OracleAdapter::AgentSlopLint => run_agent_slop(root, case),
        OracleAdapter::RustClippy => run_clippy(root, case),
        OracleAdapter::Bandit => run_bandit(root, case),
        OracleAdapter::Ruff => python_adapters::run_ruff(root, case),
        OracleAdapter::Pmd => java_adapters::run_pmd(root, case),
        OracleAdapter::Eslint => node_adapters::run_eslint(root, case),
        OracleAdapter::GolangCiLint => run_golangci_lint(root, case),
        OracleAdapter::Jscpd => node_adapters::run_jscpd(root, case),
        OracleAdapter::Detekt => run_detekt(root, case),
        OracleAdapter::Pylint => python_adapters::run_pylint(root, case),
        OracleAdapter::ImportLinter => architecture_adapters::run_import_linter(root, case),
        OracleAdapter::DependencyCruiser => {
            architecture_adapters::run_dependency_cruiser(root, case)
        }
    }
}

fn clean_source(adapter: OracleAdapter) -> &'static str {
    match adapter {
        OracleAdapter::RustClippy => "pub fn clean(value: i32) -> i32 { value }\n",
        OracleAdapter::GolangCiLint => {
            "package parity\n\nfunc clean(value int) int { return value }\n"
        }
        OracleAdapter::Eslint => "function clean(value) { return value; }\n",
        OracleAdapter::Jscpd => {
            "def clean(items):\n    total = items[0]\n    total += items[1]\n    total *= items[2]\n    total -= items[3]\n    total //= items[4]\n    total %= items[5]\n    total **= items[6]\n    total &= items[7]\n    return total\n"
        }
        OracleAdapter::Detekt => "fun clean(value: Int): Int = value\n",
        OracleAdapter::Pylint => {
            "def nested(a, b, c, d, e):\n    if a:\n        if b:\n            if c:\n                if d:\n                    if e:\n                        return 1\n    return 0\n"
        }
        OracleAdapter::Pmd => {
            "class Sample { void update(int value) { consume(value); } }\n"
        }
        OracleAdapter::ImportLinter => {
            "from domain.model import User\n\ndef check():\n    return User\n"
        }
        OracleAdapter::DependencyCruiser => {
            "import { b } from './b.js';\nexport const a = b;\n"
        }
        _ => "def clean(value):\n    return value\n",
    }
}

pub(crate) fn manifest_donor_ids() -> BTreeSet<String> {
    let manifest = read_json(&repo_root().join("auditor-donors.lock.json"));
    manifest["donors"]
        .as_array()
        .expect("auditor-donors.lock.json donors must be an array")
        .iter()
        .map(|donor| {
            donor["id"]
                .as_str()
                .expect("donor id must be a string")
                .to_string()
        })
        .collect()
}

fn ea_findings(root: &Path, case: ParityCase) -> Vec<MatchKey> {
    #[cfg(feature = "graph-analysis")]
    let analysis = if case.ea_detector == "repo_graph_import_cycle" {
        let inputs = GraphInputs {
            repo_graph: Some(RepoGraph {
                repo_graph_id: Some("parity".into()),
                nodes: vec!["a.py", "b.py"]
                    .into_iter()
                    .map(|path| RepoGraphNode {
                        path: path.into(),
                        node_kind: "file".into(),
                        ..RepoGraphNode::default()
                    })
                    .collect(),
                edges: if std::fs::read_to_string(root.join("a.py"))
                    .unwrap_or_default()
                    .contains("import b")
                {
                    vec![
                        RepoGraphEdge {
                            from_path: "a.py".into(),
                            to_path: "b.py".into(),
                            edge_kind: "imports".into(),
                        },
                        RepoGraphEdge {
                            from_path: "b.py".into(),
                            to_path: "a.py".into(),
                            edge_kind: "imports".into(),
                        },
                    ]
                } else {
                    vec![]
                },
            }),
            co_edit_journal: None,
        };
        analyze_repository_with_provider(
            root,
            SourceDiscoveryOptions::default(),
            case.profile,
            &inputs,
        )
    } else {
        analyze_repository(root, SourceDiscoveryOptions::default(), case.profile)
    }
    .unwrap_or_else(|error| panic!("EA analysis failed for {}: {error}", root.display()));
    #[cfg(not(feature = "graph-analysis"))]
    let analysis = analyze_repository(root, SourceDiscoveryOptions::default(), case.profile)
        .unwrap_or_else(|error| panic!("EA analysis failed for {}: {error}", root.display()));
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == case.ea_detector)
        .flat_map(|finding| finding_match_keys(finding, root))
        .collect::<Vec<_>>();
    findings.sort();
    findings
}

fn finding_match_keys(finding: &Finding, root: &Path) -> Vec<MatchKey> {
    assert!(
        !finding.locations.is_empty(),
        "EA detector {} emitted evidence without a location",
        finding.detector
    );
    finding
        .locations
        .iter()
        .map(|location| {
            assert!(
                location.line.is_some()
                    || matches!(
                        finding.detector.as_str(),
                        "god_module"
                            | "escape_hatch_type_density"
                            | "repo_graph_import_cycle"
                            | "package_coupling_metrics"
                            | "churn_complexity_hotspot"
                    ),
                "EA detector {} unexpectedly omitted its line",
                finding.detector
            );
            MatchKey {
                detector: finding.detector.clone(),
                path: normalize_path(root, &location.path),
                line: location.line,
            }
        })
        .collect()
}

fn compare_one_to_one(
    oracle_findings: &[OracleFinding],
    ea_findings: &[MatchKey],
) -> ParityComparison {
    let mut unmatched_ea = ea_findings.to_vec();
    let mut unmatched_oracle = Vec::new();
    let mut matched_count = 0;

    for oracle in oracle_findings {
        if let Some(index) = unmatched_ea
            .iter()
            .position(|ea| match_keys_at_available_location(&oracle.key, ea))
        {
            unmatched_ea.remove(index);
            matched_count += 1;
        } else {
            unmatched_oracle.push(oracle.clone());
        }
    }

    let oracle_count = oracle_findings.len();
    let ea_count = ea_findings.len();
    ParityComparison {
        oracle_count,
        ea_count,
        matched_count,
        recall: matched_count as f64 / oracle_count as f64,
        precision: matched_count as f64 / ea_count as f64,
        unmatched_oracle,
        unmatched_ea,
    }
}

fn match_keys_at_available_location(oracle: &MatchKey, ea: &MatchKey) -> bool {
    oracle.detector == ea.detector
        && oracle.path == ea.path
        && (oracle.line == ea.line || oracle.line.is_none() || ea.line.is_none())
}

fn run_bandit(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("bandit");
    verify_checkout_snapshot("bandit", &donor_root);
    let python = provisioned_python("EA_BANDIT_PYTHON", "bandit");
    let mut command = Command::new(python);
    command
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONPATH", &donor_root)
        .args([
            "-m",
            "bandit",
            "-q",
            "-f",
            "json",
            "-t",
            case.donor_rule,
            "-r",
        ])
        .arg(root);
    let output = run_command(&mut command, &[0, 1]);
    let payload = parse_stdout_json(&output, case.donor_id);
    findings_from_json_array(
        root,
        case,
        payload["results"]
            .as_array()
            .expect("Bandit output must contain results"),
        "test_id",
        "filename",
        "line_number",
    )
}

fn run_agent_slop(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("agent-slop-lint");
    verify_checkout_snapshot("agent-slop-lint", &donor_root);
    let python = provisioned_python("EA_AGENT_SLOP_PYTHON", "agent-slop-lint");
    let python_path = donor_root.join("cli");
    let adapter = "import sys; from slop.cli import main; raise SystemExit(main(['check', sys.argv[1], '--root', sys.argv[2], '--output', 'json', '--max-violations', '0']))";
    let mut command = Command::new(python);
    command
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONPATH", python_path)
        .args(["-c", adapter, case.donor_rule])
        .arg(root);
    let output = run_command(&mut command, &[0, 1]);
    let payload = parse_stdout_json(&output, case.donor_id);
    let violations = payload["rules"][case.donor_rule]["violations"]
        .as_array()
        .unwrap_or_else(|| panic!("agent-slop output missing {} violations", case.donor_rule));
    let selected = violations
        .iter()
        .filter(|finding| {
            case.donor_rule != "structural.duplication" || finding["file"].as_str() != Some(".")
        })
        .cloned()
        .collect::<Vec<_>>();
    findings_from_json_array(root, case, &selected, "rule", "file", "line")
}

fn run_clippy(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let target_dir = root.join("target");
    let mut command = Command::new("cargo");
    command
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", target_dir)
        .args(["clippy", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .args([
            "--message-format=json",
            "--quiet",
            "--",
            "-W",
            case.donor_rule,
        ]);
    let output = run_command(&mut command, &[0]);
    let stdout = String::from_utf8(output.stdout).expect("Clippy output must be UTF-8");
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-message")
        .filter(|message| message["message"]["code"]["code"] == case.donor_rule)
        .filter_map(|message| {
            let span = message["message"]["spans"]
                .as_array()?
                .iter()
                .find(|span| span["is_primary"].as_bool() == Some(true))?;
            Some(OracleFinding {
                donor_rule: case.donor_rule.to_string(),
                key: MatchKey {
                    detector: case.ea_detector.to_string(),
                    path: normalize_path(root, span["file_name"].as_str()?),
                    line: Some(span["line_start"].as_u64()? as usize),
                },
            })
        })
        .collect()
}

fn run_golangci_lint(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("golangci-lint");
    verify_checkout_snapshot("golangci-lint", &donor_root);
    let binary = provisioned_file(
        "EA_GOLANGCI_BIN",
        "target/donor-oracles/golangci-lint/golangci-lint",
        "GolangCI-Lint binary",
    );
    let cache = root.join(".golangci-cache");
    let go_cache = root.join(".go-cache");
    fs::create_dir_all(&cache).expect("GolangCI-Lint cache directory must be creatable");
    fs::create_dir_all(&go_cache).expect("Go cache directory must be creatable");
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .env("GOLANGCI_LINT_CACHE", cache)
        .env("GOCACHE", go_cache)
        .args([
            "run",
            "--no-config",
            "--enable-only",
            case.donor_rule,
            "--tests=false",
            "--show-stats=false",
            "--issues-exit-code=0",
            "--modules-download-mode=readonly",
            "--output.text.path",
            "/dev/null",
            "--output.json.path",
            "stdout",
            "./...",
        ]);
    let output = run_command(&mut command, &[0]);
    let payload = parse_stdout_json(&output, case.donor_id);
    payload["Issues"]
        .as_array()
        .expect("GolangCI-Lint output must contain Issues")
        .iter()
        .filter(|issue| issue["FromLinter"].as_str() == Some(case.donor_rule))
        .map(|issue| OracleFinding {
            donor_rule: case.donor_rule.to_string(),
            key: MatchKey {
                detector: case.ea_detector.to_string(),
                path: normalize_path(
                    root,
                    issue["Pos"]["Filename"]
                        .as_str()
                        .expect("GolangCI-Lint issue must contain Pos.Filename"),
                ),
                line: Some(
                    issue["Pos"]["Line"]
                        .as_u64()
                        .expect("GolangCI-Lint issue must contain Pos.Line")
                        as usize,
                ),
            },
        })
        .collect()
}

fn run_detekt(root: &Path, case: ParityCase) -> Vec<OracleFinding> {
    let donor_root = donor_path("detekt");
    verify_checkout_snapshot("detekt", &donor_root);
    let jar = provisioned_file(
        "EA_DETEKT_JAR",
        "target/donor-oracles/detekt/detekt-cli-2.0.0-alpha.5-all.jar",
        "Detekt CLI jar",
    );
    let report = root.join("detekt.sarif");
    let mut command = Command::new("java");
    command
        .args(["-jar"])
        .arg(jar)
        .args(["--input"])
        .arg(root)
        .args(["--all-rules", "--fail-on-severity", "Never", "--base-path"])
        .arg(root)
        .args(["--report"])
        .arg(format!("sarif:{}", report.display()));
    if matches!(
        case.donor_rule,
        "detekt.potential-bugs.UnreachableCode" | "detekt.performance.SpreadOperator"
    ) {
        command.args(["--analysis-mode", "full", "--jvm-target", "17"]);
    }
    run_command(&mut command, &[0]);
    let payload = read_json(&report);
    payload["runs"]
        .as_array()
        .expect("Detekt SARIF must contain runs")
        .iter()
        .flat_map(|run| {
            run["results"]
                .as_array()
                .expect("Detekt SARIF run must contain results")
        })
        .filter(|result| result["ruleId"].as_str() == Some(case.donor_rule))
        .map(|result| {
            let location = &result["locations"][0]["physicalLocation"];
            OracleFinding {
                donor_rule: case.donor_rule.to_string(),
                key: MatchKey {
                    detector: case.ea_detector.to_string(),
                    path: normalize_path(
                        root,
                        location["artifactLocation"]["uri"]
                            .as_str()
                            .expect("Detekt location must contain an artifact URI"),
                    ),
                    line: Some(
                        location["region"]["startLine"]
                            .as_u64()
                            .expect("Detekt location must contain a start line")
                            as usize,
                    ),
                },
            }
        })
        .collect()
}

fn findings_from_json_array(
    root: &Path,
    case: ParityCase,
    findings: &[serde_json::Value],
    rule_field: &str,
    path_field: &str,
    line_field: &str,
) -> Vec<OracleFinding> {
    findings
        .iter()
        .filter(|finding| finding[rule_field].as_str() == Some(case.donor_rule))
        .map(|finding| {
            let line = finding[line_field].as_u64().map(|line| line as usize);
            assert!(
                line.is_some()
                    || matches!(
                        case.donor_rule,
                        "structural.god_module"
                            | "structural.types.escape_hatches"
                            | "structural.deps"
                            | "structural.packages"
                            | "structural.hotspots"
                    ),
                "{} {} finding unexpectedly lacks a line",
                case.donor_id,
                case.donor_rule
            );
            OracleFinding {
                donor_rule: case.donor_rule.to_string(),
                key: MatchKey {
                    detector: case.ea_detector.to_string(),
                    path: normalize_path(
                        root,
                        finding[path_field]
                            .as_str()
                            .unwrap_or_else(|| panic!("{} finding lacks a path", case.donor_id)),
                    ),
                    line,
                },
            }
        })
        .collect()
}

fn run_command(command: &mut Command, accepted_exit_codes: &[i32]) -> Output {
    let rendered = format!("{command:?}");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("failed to execute {rendered}: {error}"));
    let deadline = Instant::now() + ORACLE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(ORACLE_POLL_INTERVAL);
            }
            Ok(None) => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .unwrap_or_else(|error| panic!("failed to reap {rendered}: {error}"));
                panic!(
                    "command {rendered} exceeded 120 seconds\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(error) => panic!("failed to poll {rendered}: {error}"),
        }
    }
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("failed to collect {rendered}: {error}"));
    assert!(
        output.stdout.len() <= MAX_OUTPUT_BYTES && output.stderr.len() <= MAX_OUTPUT_BYTES,
        "command {rendered} exceeded the 10 MiB output bound"
    );
    let code = output.status.code();
    assert!(
        code.is_some_and(|value| accepted_exit_codes.contains(&value)),
        "command {rendered} exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn parse_stdout_json(output: &Output, donor_id: &str) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{donor_id} emitted invalid JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn native_finding(
    root: &Path,
    case: ParityCase,
    raw_path: &str,
    line: Option<usize>,
) -> OracleFinding {
    OracleFinding {
        donor_rule: case.donor_rule.to_string(),
        key: MatchKey {
            detector: case.ea_detector.to_string(),
            path: normalize_path(root, raw_path),
            line,
        },
    }
}

fn verify_version_contains(binary: &Path, args: &[&str], expected: &str, label: &str) {
    let mut command = Command::new(binary);
    command.args(args);
    let output = run_command(&mut command, &[0]);
    let actual = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{label} version output must be UTF-8: {error}"));
    assert!(
        actual.contains(expected),
        "{label} executable does not match parity version {expected}: {actual}"
    );
}

fn provisioned_python(env_name: &str, tool: &str) -> PathBuf {
    if let Some(path) = env::var_os(env_name).map(PathBuf::from) {
        assert!(
            path.is_file(),
            "{env_name} does not name a Python executable"
        );
        return path;
    }
    let path = PathBuf::from(
        env::var_os("HOME").expect("HOME is required to resolve provisioned donor Python"),
    )
    .join(".local/share/uv/tools")
    .join(tool)
    .join("bin/python");
    assert!(
        path.is_file(),
        "{tool} donor environment is not provisioned; set {env_name} to its Python executable"
    );
    path
}

fn provisioned_file(env_name: &str, default_relative: &str, label: &str) -> PathBuf {
    let path = env::var_os(env_name)
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join(default_relative));
    assert!(
        path.is_file(),
        "{label} is not provisioned at {}; set {env_name}",
        path.display()
    );
    path
}

fn verify_checkout_snapshot(donor_id: &str, donor_root: &Path) {
    let manifest = read_json(&repo_root().join("auditor-donors.lock.json"));
    let donor = manifest["donors"]
        .as_array()
        .expect("donors must be an array")
        .iter()
        .find(|donor| donor["id"] == donor_id)
        .unwrap_or_else(|| panic!("{donor_id} is missing from auditor-donors.lock.json"));
    let snapshot = donor["snapshot"]
        .as_str()
        .expect("donor snapshot must be a string");
    let expected = snapshot.rsplit(':').next().unwrap_or(snapshot);
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(donor_root)
        .args(["rev-parse", "HEAD"]);
    let output = run_command(&mut command, &[0]);
    let actual = String::from_utf8(output.stdout)
        .expect("git output must be UTF-8")
        .trim()
        .to_string();
    assert!(
        actual.starts_with(expected),
        "{donor_id} checkout {actual} does not match locked snapshot {snapshot}"
    );
}

fn donor_path(donor_id: &str) -> PathBuf {
    let manifest = read_json(&repo_root().join("auditor-donors.lock.json"));
    let donor = manifest["donors"]
        .as_array()
        .expect("donors must be an array")
        .iter()
        .find(|donor| donor["id"] == donor_id)
        .unwrap_or_else(|| panic!("{donor_id} is missing from auditor-donors.lock.json"));
    let local_path = donor["local_path"]
        .as_str()
        .unwrap_or_else(|| panic!("{donor_id} has no local_path"));
    let path = repo_root().join(local_path);
    assert!(
        path.is_dir(),
        "{donor_id} checkout is missing at {}",
        path.display()
    );
    path
}

fn read_json(path: &Path) -> serde_json::Value {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn normalize_path(root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path
    };
    relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            std::path::Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root must resolve")
}

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(label: &str) -> Self {
        let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock precedes Unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "engineering-auditor-donor-{label}-{}-{nanos}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir_all(&root)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", root.display()));
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
        }
        fs::write(&path, contents)
            .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

use std::path::PathBuf;

use auditor_report::{
    match_baseline, parse_baseline_document, parse_report_findings, parse_report_identity,
    BaselineEntry, CarriedFinding, ParsedFinding,
};

use crate::arg_cursor::ArgCursor;
use crate::scan_command;

pub(crate) const POLICY_FAILURE: &str = "__AUDITOR_POLICY_FAILURE__";

pub(crate) fn run_gate(args: &[String]) -> Result<String, String> {
    let options = GateOptions::parse(args)?;
    let report_json = scan_command::run_scan(&options.scan_args)?;
    let current_identity = parse_report_identity(&report_json)?;
    let current = parse_report_findings(&report_json)?;
    let result = if let Some(path) = options.baseline {
        compare_baseline(&path, &current_identity, &current)?
    } else {
        compare_budget(
            options.max_findings.expect("validated policy mode"),
            &current,
        )
    };
    let summary = result.to_json(current.len());
    if let Some(dir) = options.output_dir {
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("failed to create gate output directory: {error}"))?;
        std::fs::write(dir.join("gate.json"), &summary)
            .map_err(|error| format!("failed to write gate.json: {error}"))?;
        std::fs::write(dir.join("findings.json"), &report_json)
            .map_err(|error| format!("failed to write findings.json: {error}"))?;
    }
    if result.passed {
        Ok(summary)
    } else {
        Err(format!("{POLICY_FAILURE}{summary}"))
    }
}

struct GateOptions {
    scan_args: Vec<String>,
    baseline: Option<PathBuf>,
    max_findings: Option<usize>,
    output_dir: Option<PathBuf>,
}

impl GateOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            scan_args: Vec::new(),
            baseline: None,
            max_findings: None,
            output_dir: None,
        };
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            options.consume(argument, &mut cursor)?;
        }
        options.validate_policy()?;
        Ok(options)
    }

    fn consume(&mut self, argument: &str, cursor: &mut ArgCursor<'_>) -> Result<(), String> {
        match argument {
            "--baseline" => self.baseline = Some(PathBuf::from(cursor.required(argument)?)),
            "--max-findings" => {
                self.max_findings = Some(
                    cursor
                        .required(argument)?
                        .parse()
                        .map_err(|_| "--max-findings requires an integer".to_string())?,
                );
            }
            "--output-dir" => {
                self.output_dir = Some(PathBuf::from(cursor.required(argument)?));
            }
            "--format" if cursor.required(argument)? != "json" => {
                return Err("gate output format is always json".to_string());
            }
            "--format" => {}
            value => self.forward_scan_argument(value, cursor)?,
        }
        Ok(())
    }

    fn forward_scan_argument(
        &mut self,
        argument: &str,
        cursor: &mut ArgCursor<'_>,
    ) -> Result<(), String> {
        self.scan_args.push(argument.to_string());
        if scan_option_takes_value(argument) {
            self.scan_args.push(cursor.required(argument)?.to_string());
        }
        Ok(())
    }

    fn validate_policy(&self) -> Result<(), String> {
        if self.baseline.is_some() == self.max_findings.is_some() {
            return Err("gate requires exactly one of --baseline or --max-findings".to_string());
        }
        Ok(())
    }
}

fn scan_option_takes_value(value: &str) -> bool {
    matches!(
        value,
        "--repo-root"
            | "--profile"
            | "--method"
            | "--repo-graph"
            | "--co-edit-journal"
            | "--path"
            | "--changed-ranges-json"
    )
}

struct GateResult {
    passed: bool,
    reason: &'static str,
    baseline_status: &'static str,
    new_findings: Vec<auditor_report::ParsedFinding>,
    carried: Vec<CarriedFinding>,
    resolved: usize,
    persisting: usize,
}

impl GateResult {
    fn to_json(&self, total: usize) -> String {
        let findings = self
            .new_findings
            .iter()
            .map(|finding| {
                serde_json::json!({
                    "detector": finding.detector,
                    "id": finding.id,
                    "line": finding.line,
                    "message": finding.message,
                    "path": finding.path,
                    "priority": priority(&finding.severity),
                    "recommendation": finding.recommendation,
                    "severity": finding.severity,
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&serde_json::json!({
            "baseline_status": self.baseline_status,
            "carried_count": self.carried.len(),
            "carried_findings": self
                .carried
                .iter()
                .map(|carried| serde_json::json!({
                    "baseline_id": carried.baseline_id,
                    "current_id": carried.current_id,
                    "pass": carried.pass.as_str(),
                }))
                .collect::<Vec<_>>(),
            "gate": "bloat_slop_findings",
            "new_count": findings.len(),
            "new_findings": findings,
            "passed": self.passed,
            "persisting_count": self.persisting,
            "reason": self.reason,
            "resolved_count": self.resolved,
            "schema_version": 2,
            "total_count": total,
        }))
        .expect("gate values are serializable")
    }
}

fn compare_baseline(
    path: &PathBuf,
    current_identity: &auditor_report::ReportIdentity,
    current: &[auditor_report::ParsedFinding],
) -> Result<GateResult, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|_| policy_error("baseline_required", "missing", current.len()))?;
    let baseline = parse_baseline_document(&text)
        .map_err(|_| policy_error("baseline_required", "incomparable", current.len()))?;
    if &baseline.identity() != current_identity {
        return Err(policy_error(
            "baseline_required",
            "incomparable",
            current.len(),
        ));
    }
    let accepted = baseline
        .entries
        .iter()
        .map(BaselineEntry::match_subject)
        .collect::<Vec<_>>();
    let subjects = current
        .iter()
        .map(ParsedFinding::match_subject)
        .collect::<Vec<_>>();
    let matched = match_baseline(&accepted, &subjects);
    let new_findings = current
        .iter()
        .filter(|finding| matched.new.contains(&finding.id))
        .cloned()
        .collect::<Vec<_>>();
    Ok(GateResult {
        passed: new_findings.is_empty(),
        reason: if new_findings.is_empty() {
            "clean"
        } else {
            "new_findings_introduced"
        },
        baseline_status: "comparable",
        new_findings,
        resolved: matched.resolved.len(),
        persisting: matched.unchanged.len() + matched.carried.len(),
        carried: matched.carried,
    })
}

fn compare_budget(maximum: usize, current: &[auditor_report::ParsedFinding]) -> GateResult {
    let passed = current.len() <= maximum;
    GateResult {
        passed,
        reason: if passed {
            "clean"
        } else {
            "finding_budget_exceeded"
        },
        baseline_status: "not_applicable",
        new_findings: current.to_vec(),
        carried: Vec::new(),
        resolved: 0,
        persisting: current.len(),
    }
}

fn policy_error(reason: &'static str, baseline_status: &'static str, total: usize) -> String {
    let result = GateResult {
        passed: false,
        reason,
        baseline_status,
        new_findings: Vec::new(),
        carried: Vec::new(),
        resolved: 0,
        persisting: total,
    };
    format!("{POLICY_FAILURE}{}", result.to_json(total))
}

fn priority(severity: &str) -> &'static str {
    match severity {
        "critical" | "high" => "P0",
        "medium" => "P1",
        _ => "P2",
    }
}

use auditor_core::{Confidence, Finding, Severity};

use crate::{util, AuditedFile, AuditorConfig};

pub(crate) use crate::security_api_rules::{
    bind_all_interfaces_findings, random_for_security_findings, request_no_cert_verify_findings,
    ssh_no_host_key_verification_findings, ssl_with_bad_defaults_findings,
    unsafe_deserialization_findings, world_writable_file_findings, xml_xxe_vulnerable_findings,
};
pub(crate) use crate::security_literal_rules::{
    hardcoded_credential_findings, hardcoded_tmp_directory_findings,
};

#[derive(Clone, Copy)]
pub(crate) struct Rule {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) severity: Severity,
    pub(crate) confidence: Confidence,
    pub(crate) recommendation: &'static str,
    pub(crate) cwe: &'static str,
}

const TRY_EXCEPT_PASS: Rule = Rule {
    id: "try_except_pass",
    title: "Bare except:pass clause (Bandit B110)",
    severity: Severity::Medium,
    confidence: Confidence::High,
    recommendation: "Handle the exception or raise a more specific exception.",
    cwe: "CWE-703",
};
const FLASK_DEBUG_TRUE: Rule = Rule {
    id: "flask_debug_true",
    title: "Flask app in debug mode (Bandit B201)",
    severity: Severity::High,
    confidence: Confidence::High,
    recommendation: "Disable debug mode in production.",
    cwe: "CWE-94",
};
pub(crate) fn rule_finding(rule: Rule, rel: &str, line_number: usize, pattern: &str) -> Finding {
    auditor_core::finding!(
        rule.id,
        "correctness",
        rule.title,
        format!("{} at line {}", rule.title, line_number),
        rule.severity,
        rule.confidence,
        rel,
        Some(line_number),
    )
    .with_recommendation(rule.recommendation)
    .with_evidence("pattern", pattern)
    .with_evidence("cwe", rule.cwe)
    .with_identity("pattern", pattern)
}

fn scan_source_files(
    files: &[AuditedFile],
    cache: &util::FileCache,
    detector: impl Fn(&str, &str) -> Vec<Finding>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files
        .iter()
        .filter(|file| file.language.is_source() && !util::is_test_source_path(&file.relative))
    {
        let rel = util::path_slash(&file.relative);
        findings.extend(detector(&rel, cache.text(file)));
    }
    findings
}

fn matching_lines(
    rel: &str,
    text: &str,
    rule: Rule,
    matches: impl Fn(&str, &str) -> bool,
) -> Vec<Finding> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let lower = util::strip_comments(line).to_ascii_lowercase();
            let literal_lower = util::strip_comments_preserve_literals(line).to_ascii_lowercase();
            matches(&lower, &literal_lower).then(|| rule_finding(rule, rel, index + 1, line.trim()))
        })
        .collect()
}

fn try_except_pass_per_file(rel: &str, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut bare_except_line = None;
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let code = util::strip_comments(line);
        let lower = code.to_ascii_lowercase();
        let is_bare_except =
            lower.contains("except:") && !lower.contains("except ") && !lower.contains("except\t");
        if is_bare_except {
            if lower.contains("pass") {
                findings.push(rule_finding(TRY_EXCEPT_PASS, rel, line_number, line.trim()));
            } else {
                bare_except_line = Some(line_number);
            }
            continue;
        }
        if let Some(except_line) = bare_except_line {
            let stripped = code.trim();
            if stripped == "pass" {
                findings.push(rule_finding(TRY_EXCEPT_PASS, rel, except_line, line.trim()));
                bare_except_line = None;
            } else if !stripped.is_empty() {
                bare_except_line = None;
            }
        }
    }
    findings
}

pub(crate) fn try_except_pass_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    scan_source_files(files, cache, try_except_pass_per_file)
}

pub(crate) fn flask_debug_true_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    scan_source_files(files, cache, |rel, text| {
        matching_lines(rel, text, FLASK_DEBUG_TRUE, |lower, _| {
            (lower.contains("debug=true") || lower.contains("debug = true"))
                && (lower.contains("flask")
                    || lower.contains("app.run")
                    || lower.contains("flask("))
        })
    })
}

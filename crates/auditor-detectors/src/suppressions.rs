//! Inline suppressions: `auditor-ignore[detector, ..]: reason` in a comment on
//! a finding's line, or on a comment-only line directly above it, removes
//! that finding and keeps it, with the reason, in the report. A suppression
//! without a reason suppresses nothing. `invalid_suppression` reports one
//! without a reason, one that names an unknown detector, and one that
//! matches no finding of a detector that reports in its file.

use auditor_core::{Confidence, Finding, Severity, SuppressedFinding};

use crate::registry;
use crate::{util, AuditedFile, Language};

const MARKER: &str = "auditor-ignore[";

/// Text that opens a comment in the scanned languages.
const COMMENT_OPENERS: &[&str] = &["//", "#", "--", "/*"];

struct Suppression {
    path: String,
    line: usize,
    /// The comment is alone on its line, so it covers the next line.
    own_line: bool,
    detectors: Vec<String>,
    reason: Option<String>,
    text: String,
}

/// A suppression that does not do its job, and why.
pub(crate) struct InvalidSuppression {
    path: String,
    line: usize,
    text: String,
    kind: &'static str,
    issue: String,
    recommendation: &'static str,
}

pub(crate) struct SuppressionOutcome {
    pub(crate) suppressed: Vec<SuppressedFinding>,
    pub(crate) invalid: Vec<InvalidSuppression>,
}

/// Removes the findings valid suppressions in `files` cover. `reports` says
/// whether a detector reports in a file at all; a suppression naming one
/// that does not cannot be judged unused.
pub(crate) fn apply(
    findings: &mut Vec<Finding>,
    files: &[AuditedFile],
    cache: &util::FileCache,
    reports: impl Fn(&str, &AuditedFile) -> bool,
) -> SuppressionOutcome {
    let mut outcome = SuppressionOutcome {
        suppressed: Vec::new(),
        invalid: Vec::new(),
    };
    for file in files.iter().filter(|file| has_comments(&file.language)) {
        let text = cache.text(file);
        if !text.contains(MARKER) {
            continue;
        }
        let path = util::path_slash(&file.relative);
        for suppression in parse(&path, text) {
            judge(&suppression, file, findings, &reports, &mut outcome);
        }
    }
    outcome.suppressed.sort_by(|left, right| {
        (&left.path, left.line, &left.detector, &left.id).cmp(&(
            &right.path,
            right.line,
            &right.detector,
            &right.id,
        ))
    });
    outcome
}

/// Removes the findings `suppression` covers into `suppressed`, or records
/// why it is invalid.
fn judge(
    suppression: &Suppression,
    file: &AuditedFile,
    findings: &mut Vec<Finding>,
    reports: &impl Fn(&str, &AuditedFile) -> bool,
    outcome: &mut SuppressionOutcome,
) {
    let SuppressionOutcome {
        suppressed,
        invalid,
    } = outcome;
    let Some(reason) = &suppression.reason else {
        invalid.push(problem(
            suppression,
            "missing_reason",
            "gives no reason, so it suppresses nothing".to_string(),
            "Add the reason after a colon: `auditor-ignore[detector]: why the finding does not apply`.",
        ));
        return;
    };
    for detector in &suppression.detectors {
        if registry::detector_spec(detector).is_none() {
            invalid.push(problem(
                suppression,
                "unknown_detector",
                format!("names `{detector}`, which is not a detector"),
                "Use a detector ID that `auditor catalog --profile full` lists.",
            ));
            continue;
        }
        let before = suppressed.len();
        findings.retain(|finding| {
            let covered = covers(suppression, detector, finding);
            if covered {
                suppressed.push(SuppressedFinding {
                    id: finding.id.clone(),
                    detector: finding.detector.clone(),
                    path: suppression.path.clone(),
                    line: finding.primary_line().unwrap_or(suppression.line),
                    reason: reason.clone(),
                });
            }
            !covered
        });
        if suppressed.len() == before && reports(detector, file) {
            invalid.push(problem(
                suppression,
                "unused",
                format!("matches no `{detector}` finding"),
                "Remove the suppression: the finding it covered is gone.",
            ));
        }
    }
}

/// Source and script languages: markup and data files carry no code to
/// suppress findings in.
fn has_comments(language: &Language) -> bool {
    !matches!(
        language,
        Language::Markdown | Language::Json | Language::Toml | Language::Yaml | Language::Other(_)
    )
}

fn covers(suppression: &Suppression, detector: &str, finding: &Finding) -> bool {
    finding.detector == detector
        && finding.primary_path() == suppression.path
        && finding.primary_line().is_some_and(|line| {
            line == suppression.line || (suppression.own_line && line == suppression.line + 1)
        })
}

fn parse(path: &str, text: &str) -> Vec<Suppression> {
    text.lines()
        .enumerate()
        .filter_map(|(row, line)| {
            // String literals are code: a marker inside one suppresses nothing.
            let code = util::strip_string_literals(line);
            let start = code.find(MARKER)?;
            let before = &code[..start];
            let opener = COMMENT_OPENERS
                .iter()
                .filter_map(|opener| before.find(opener))
                .min()?;
            let (names, rest) = code[start + MARKER.len()..].split_once(']')?;
            let detectors = names
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            let reason = rest
                .trim_start()
                .strip_prefix(':')
                .map(|reason| {
                    reason
                        .trim()
                        .trim_end_matches("*/")
                        .trim_end_matches("-->")
                        .trim()
                })
                .filter(|reason| !reason.is_empty())
                .map(str::to_string);
            Some(Suppression {
                path: path.to_string(),
                line: row + 1,
                own_line: before[..opener].trim().is_empty(),
                detectors,
                reason,
                text: code[start..].trim().to_string(),
            })
        })
        .collect()
}

fn problem(
    suppression: &Suppression,
    kind: &'static str,
    issue: String,
    recommendation: &'static str,
) -> InvalidSuppression {
    InvalidSuppression {
        path: suppression.path.clone(),
        line: suppression.line,
        text: suppression.text.clone(),
        kind,
        issue,
        recommendation,
    }
}

/// `invalid_suppression`: one finding per suppression that [`apply`] found
/// invalid, at the comment's line.
pub(crate) fn invalid_suppression_findings(invalid: Vec<InvalidSuppression>) -> Vec<Finding> {
    invalid
        .into_iter()
        .map(|problem| {
            auditor_core::finding!(
                "invalid_suppression",
                "lexical",
                "Invalid inline suppression",
                format!(
                    "`auditor-ignore` at line {} {}.",
                    problem.line, problem.issue
                ),
                Severity::Low,
                Confidence::High,
                &problem.path,
                Some(problem.line),
            )
            .with_recommendation(problem.recommendation)
            .with_evidence("problem", problem.kind)
            .with_evidence("suppression", problem.text)
        })
        .collect()
}

#[cfg(test)]
#[path = "suppressions_tests.rs"]
mod tests;

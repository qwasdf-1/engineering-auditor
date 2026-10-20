use std::collections::BTreeSet;
use std::path::Path;

use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::{AuditedFile, FunctionSpan, Language, COMPLEXITY_THRESHOLD};

#[cfg(feature = "graph-analysis")]
use std::collections::BTreeMap;
#[cfg(feature = "graph-analysis")]
use std::fs;

#[cfg(feature = "graph-analysis")]
pub(crate) fn tooling_overlap_findings(files: &[AuditedFile]) -> Vec<Finding> {
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for file in files {
        if let Some((group, role)) = tooling_role(&file.relative, &file.absolute) {
            grouped
                .entry((group, role))
                .or_default()
                .insert(util::path_slash(&file.relative));
        }
    }
    grouped
        .into_iter()
        .filter_map(|((group, role), paths)| {
            let paths_vec: Vec<String> = paths.into_iter().collect();
            if paths_vec.len() < 2 {
                return None;
            }
            Some(
                auditor_core::finding!(
                    "tooling_overlap",
                    "tooling",
                    "Overlapping tooling entry points",
                    format!("{group} has {} active entry points for role `{role}`.", paths_vec.len()),
                    Severity::Medium,
                    Confidence::High,
                    paths_vec[0].clone(),
                    None,
                )
                .with_recommendation(
                    "Audit call sites and consolidate or retire duplicate entry points for the same role.",
                )
                .with_evidence("group", group)
                .with_evidence("role", role)
                .with_evidence("files", paths_vec),
            )
        })
        .collect()
}

#[cfg(feature = "graph-analysis")]
fn tooling_role(relative: &Path, absolute: &Path) -> Option<(String, String)> {
    let rel = util::path_slash(relative);
    if !rel.starts_with("tooling/") {
        return None;
    }
    if relative.extension().and_then(|ext| ext.to_str()) != Some("sh") || !has_shebang(absolute) {
        return None;
    }
    let stem = relative.file_stem()?.to_str()?;
    let role = stem.strip_prefix("run-")?.split('-').next()?;
    let group = relative
        .parent()
        .map(util::path_slash)
        .unwrap_or_else(|| "tooling".to_string());
    Some((group, role.to_string()))
}

#[cfg(feature = "graph-analysis")]
fn has_shebang(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| text.starts_with("#!"))
        .unwrap_or(false)
}

fn source_line_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
    detector: fn(&str, &Language, &[&str]) -> Vec<Finding>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|file| file.language.is_source()) {
        let rel = util::path_slash(&file.relative);
        findings.extend(detector(&rel, &file.language, &cache.lines(file)));
    }
    findings
}

pub(crate) fn function_complexity_finding(
    title: &'static str,
    message: impl Into<String>,
    severity: Severity,
    confidence: Confidence,
    path: impl Into<String>,
    line: Option<usize>,
) -> Finding {
    auditor_core::finding!(
        "function_complexity",
        "structural",
        title,
        message,
        severity,
        confidence,
        path,
        line,
    )
}

pub(crate) fn halstead_information_density_finding(
    title: &'static str,
    message: impl Into<String>,
    severity: Severity,
    confidence: Confidence,
    path: impl Into<String>,
    line: Option<usize>,
) -> Finding {
    auditor_core::finding!(
        "halstead_information_density",
        "information",
        title,
        message,
        severity,
        confidence,
        path,
        line,
    )
}

/// AST findings, except in token mode.
fn ast_findings(
    method: crate::AnalysisMethod,
    build: impl FnOnce() -> Vec<Finding>,
) -> Vec<Finding> {
    if method == crate::AnalysisMethod::Token {
        Vec::new()
    } else {
        build()
    }
}

/// Source files measured by the token fallback: all of them in token mode,
/// otherwise those without an AST grammar.
fn token_fallback_files(
    files: &[AuditedFile],
    method: crate::AnalysisMethod,
) -> impl Iterator<Item = &AuditedFile> {
    files.iter().filter(move |file| {
        file.language.is_source()
            && (method == crate::AnalysisMethod::Token
                || util::map_language_to_ast(&file.language).is_none())
    })
}

pub(crate) fn function_complexity_findings(
    files: &[AuditedFile],
    method: crate::AnalysisMethod,
    cache: &util::FileCache,
    complexity: &crate::cognitive::ComplexityIndex,
) -> Vec<Finding> {
    let mut findings = ast_findings(method, || {
        crate::cognitive::function_complexity_ast_findings(complexity)
    });
    for file in token_fallback_files(files, method) {
        let rel = util::path_slash(&file.relative);
        findings.extend(function_complexity_token_per_file(
            &rel,
            &file.language,
            &cache.lines(file),
        ));
    }
    findings
}

pub(crate) fn cognitive_complexity_findings(
    method: crate::AnalysisMethod,
    complexity: &crate::cognitive::ComplexityIndex,
) -> Vec<Finding> {
    ast_findings(method, || {
        crate::cognitive::cognitive_complexity_ast_findings(complexity)
    })
}

pub(crate) fn npath_complexity_findings(
    method: crate::AnalysisMethod,
    complexity: &crate::cognitive::ComplexityIndex,
) -> Vec<Finding> {
    ast_findings(method, || {
        crate::cognitive::npath_complexity_ast_findings(complexity)
    })
}

pub(crate) fn halstead_information_density_findings(
    files: &[AuditedFile],
    method: crate::AnalysisMethod,
    cache: &util::FileCache,
    complexity: &crate::cognitive::ComplexityIndex,
) -> Vec<Finding> {
    let mut findings = ast_findings(method, || {
        crate::cognitive::halstead_information_density_ast_findings(complexity)
    });
    for file in token_fallback_files(files, method) {
        let rel = util::path_slash(&file.relative);
        let lines = cache.lines(file).to_vec();
        let spans = util::cached_function_spans_for(cache, file, &lines);
        findings.extend(halstead_density_token_per_file(
            &rel,
            &file.language,
            &lines,
            &spans,
        ));
    }
    findings
}

pub(crate) fn escape_hatch_type_density_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    source_line_findings(files, cache, escape_hatch_type_density_per_file)
}

fn function_complexity_token_per_file(
    rel: &str,
    language: &Language,
    lines: &[&str],
) -> Vec<Finding> {
    let spans = util::function_spans_for(language, lines, rel);
    spans
        .iter()
        .filter_map(|span| {
            let body = util::function_body_lines(lines, span, &spans);
            let complexity = 1 + body.iter().map(|(_, line)| util::branch_decision_count(line)).sum::<usize>();
            if complexity <= COMPLEXITY_THRESHOLD {
                return None;
            }
            Some(
                function_complexity_finding(
                    "Function branch complexity exceeds donor-calibrated budget",
                    format!(
                        "Function `{}` has rough branch complexity {} (> {}).",
                        span.name, complexity, COMPLEXITY_THRESHOLD
                    ),
                    Severity::Medium,
                    Confidence::Medium,
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation(
                    "Reduce branches or split cohesive behavior only when the net implementation shrinks.",
                )
                .with_evidence("function", span.name.clone())
                .with_evidence("rough_complexity", complexity)
                .with_evidence("threshold", COMPLEXITY_THRESHOLD)
                .with_evidence("start_line", span.start_line)
                .with_evidence("end_line", span.end_line),
            )
        })
        .collect()
}

/// TypeScript `any` used as a value's type. Inside generic brackets
/// (`Map<any, any>`, `R extends HandlerResponse<any> = any`) or as a
/// constraint (`T extends any[]`) it is the idiomatic placeholder of a
/// variadic generic, not an escape hatch (operator decision, 2026-10-18).
/// `depth` carries open generic brackets across lines.
/// Updates the open generic-bracket count for `ch`: `Foo<`, or a line opening
/// a type-parameter list (`<T`), opens one; a spaced `a < b` is a comparison,
/// `=>` an arrow, and `;` ends the statement.
fn track_generic_depth(ch: char, raw: char, previous: char, line_start: bool, depth: &mut usize) {
    match ch {
        '<' if raw.is_ascii_alphanumeric() || raw == '_' || line_start => *depth += 1,
        '>' if previous != '=' && *depth > 0 => *depth -= 1,
        ';' => *depth = 0,
        _ => {}
    }
}

fn typescript_escape_anys(code: &str, depth: &mut usize) -> usize {
    let mut count = 0;
    // The character just before (`raw`) and the last non-space one.
    let (mut raw, mut previous) = (' ', ' ');
    let mut last_word = String::new();
    let mut word = String::new();
    for ch in code.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            word.push(ch);
            (raw, previous) = (ch, ch);
            continue;
        }
        if !word.is_empty() {
            if word == "any" && *depth == 0 && last_word != "extends" {
                count += 1;
            }
            last_word = std::mem::take(&mut word);
        }
        track_generic_depth(ch, raw, previous, last_word.is_empty(), depth);
        raw = ch;
        if !ch.is_whitespace() {
            previous = ch;
        }
    }
    count
}

fn escape_hatch_type_density_per_file(
    rel: &str,
    language: &Language,
    lines: &[&str],
) -> Vec<Finding> {
    if util::is_test_source_path(Path::new(rel))
        || !matches!(language, Language::Python | Language::TypeScript)
    {
        return Vec::new();
    }
    let mut annotations = 0usize;
    let mut escape = 0usize;
    let mut generic_depth = 0usize;
    for line in lines {
        let code = util::strip_comments(line);
        annotations += code.matches(':').count() + code.matches("->").count();
        escape += match language {
            Language::Python => util::count_word(&code, "Any"),
            Language::TypeScript => typescript_escape_anys(&code, &mut generic_depth),
            _ => 0,
        };
    }
    if annotations < 5 {
        return Vec::new();
    }
    let density = escape as f64 / annotations as f64;
    if density <= 0.30 {
        return Vec::new();
    }
    vec![auditor_core::finding!(
        "escape_hatch_type_density",
        "structural",
        "Escape-hatch types dominate annotations",
        format!("File has escape-hatch type density {density:.2} (> 0.30)."),
        Severity::Medium,
        Confidence::Medium,
        rel,
        None,
    )
    .with_recommendation(
        "Replace broad escape-hatch types with narrower domain types where practical.",
    )
    .with_evidence("annotations", annotations)
    .with_evidence("escape_hatch_count", escape)
    .with_evidence("density", density)]
}

fn halstead_density_token_per_file(
    rel: &str,
    language: &Language,
    lines: &[&str],
    spans: &[FunctionSpan],
) -> Vec<Finding> {
    spans
        .iter()
        .filter_map(|span| {
            let mut operators = BTreeSet::new();
            let mut operands = BTreeSet::new();
            let mut total_ops = 0usize;
            let mut total_operands = 0usize;
            for (_, line) in util::function_body_lines(lines, span, spans) {
                let code = util::strip_string_literals(&util::strip_comments(line));
                for token in util::identifiers(&code) {
                    if util::keywords(language).contains(token.as_str()) {
                        operators.insert(token);
                        total_ops += 1;
                    } else {
                        operands.insert(token);
                        total_operands += 1;
                    }
                }
                for op in ["==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "="] {
                    let count = code.matches(op).count();
                    if count > 0 {
                        operators.insert(op.to_string());
                        total_ops += count;
                    }
                }
            }
            let vocabulary = operators.len() + operands.len();
            let length = total_ops + total_operands;
            let volume = if vocabulary > 1 {
                length as f64 * (vocabulary as f64).log2()
            } else {
                0.0
            };
            let difficulty = if !operands.is_empty() {
                operators.len() as f64 / 2.0 * (total_operands as f64 / operands.len() as f64)
            } else {
                0.0
            };
            if volume <= 1500.0 && difficulty <= 30.0 {
                return None;
            }
            Some(
                halstead_information_density_finding(
                    "Function information density exceeds full-profile threshold",
                    format!("Function `{}` has estimated Halstead volume {volume:.1}, difficulty {difficulty:.1}.", span.name),
                    Severity::Medium,
                    Confidence::Medium,
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation("Reduce distinct symbols per function; extract typed helpers when net size drops.")
                .with_evidence("function", span.name.clone())
                .with_evidence("volume", volume)
                .with_evidence("difficulty", difficulty),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    include!("structural_tests.rs");
}

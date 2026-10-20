use std::collections::{BTreeMap, BTreeSet};

use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::{FunctionSpan, Language};

fn section_divider_with_spans(
    rel: &str,
    language: &Language,
    lines: &[&str],
    spans: &[FunctionSpan],
) -> Vec<Finding> {
    spans
        .iter()
        .filter_map(|span| {
            let body = util::function_body_lines(lines, span, spans);
            let divider_lines: Vec<_> = body
                .iter()
                .filter_map(|(line_number, line)| {
                    is_section_divider(language, line).then_some(*line_number)
                })
                .collect();
            if divider_lines.len() <= crate::SECTION_DIVIDER_THRESHOLD {
                return None;
            }
            Some(
                auditor_core::finding!(
                    "section_divider_comment",
                    "lexical",
                    "Function uses repeated section-divider comments",
                    format!(
                        "Function `{}` contains {} section-divider comments (> {}).",
                        span.name,
                        divider_lines.len(),
                        crate::SECTION_DIVIDER_THRESHOLD
                    ),
                    Severity::Medium,
                    Confidence::High,
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation(
                    "Extract typed helpers or delete decorative sectioning after confirming the function is doing too much.",
                )
                .with_evidence("function", span.name.clone())
                .with_evidence("count", divider_lines.len())
                .with_evidence("threshold", crate::SECTION_DIVIDER_THRESHOLD)
                .with_evidence(
                    "lines",
                    divider_lines
                        .iter()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>(),
                ),
            )
        })
        .collect()
}

/// A comment holding a rule of at least three repeated `-`, `=`, `*`, or `_`.
/// Scattered punctuation in prose (`-e/--regexp`, snake_case) is not a rule,
/// a border framed by `+` or `|` (`+---+---+`) draws a table or byte-layout
/// diagram, and `#` starts a comment only in hash-comment languages (not Rust
/// attributes or C preprocessor lines).
fn is_section_divider(language: &Language, line: &str) -> bool {
    let trimmed = line.trim();
    let hash_comments = matches!(
        language,
        Language::Python | Language::Ruby | Language::Julia | Language::Shell
    );
    let Some(content) = trimmed
        .strip_prefix("//")
        .or_else(|| trimmed.strip_prefix("/*"))
        .or_else(|| hash_comments.then(|| trimmed.strip_prefix('#')).flatten())
    else {
        return false;
    };
    // Diagram borders and number lines (`+---+`, `<--- delta --->`,
    // `---[---+---]---`) are drawings, not dividers.
    if content.trim_start().starts_with(['+', '|'])
        || content.contains(['<', '>', '[', ']', '+', '|', '^'])
    {
        return false;
    }
    let chars: Vec<char> = content.chars().collect();
    chars
        .windows(3)
        .any(|run| matches!(run[0], '-' | '=' | '*' | '_') && run[1] == run[0] && run[2] == run[0])
}

/// First line of a trailing `#[cfg(test)] mod` block; everything after it
/// is test code compiled only under `cargo test`.
fn rust_test_module_start(lines: &[&str]) -> Option<usize> {
    lines.iter().enumerate().find_map(|(index, line)| {
        (line.trim() == "#[cfg(test)]"
            && lines[index + 1..]
                .iter()
                .map(|next| next.trim())
                .find(|next| !next.is_empty() && !next.starts_with("#["))
                .is_some_and(|next| next.starts_with("mod ") || next.starts_with("pub mod ")))
        .then_some(index + 1)
    })
}

fn is_rust_test_function(lines: &[&str], span: &FunctionSpan, test_module: Option<usize>) -> bool {
    test_module.is_some_and(|start| span.start_line > start)
        || lines[span.start_line.saturating_sub(4)..span.start_line.saturating_sub(1)]
            .iter()
            .any(|line| line.trim_start().starts_with("#[test]") || line.contains("::test]"))
}

fn magic_literal_with_spans(
    rel: &str,
    language: &Language,
    lines: &[&str],
    spans: &[FunctionSpan],
) -> Vec<Finding> {
    let test_module = (*language == Language::Rust)
        .then(|| rust_test_module_start(lines))
        .flatten();
    if util::is_locale_path(rel) {
        return Vec::new();
    }
    let masked = crate::block_strings::mask_block_strings(language, lines);
    let code: Vec<&str> = masked.iter().map(String::as_str).collect();
    spans
        .iter()
        .filter(|span| {
            *language != Language::Rust || !is_rust_test_function(lines, span, test_module)
        })
        // Hash mixing constants are the algorithm (Checkstyle MagicNumber's
        // `ignoreHashCodeMethod`).
        .filter(|span| !matches!(span.name.as_str(), "hashCode" | "__hash__" | "GetHashCode"))
        .filter_map(|span| {
            // Keyed by value, so `0x10` and `16` count once, under the first spelling.
            let mut literals: BTreeMap<String, String> = BTreeMap::new();
            // Signature lines hold keyword defaults, which name themselves.
            for (_, line) in util::function_executable_lines(language, &code, span, spans) {
                for literal in util::numeric_literals(
                    &crate::magic_literal_code::magic_candidate_code(language, line),
                ) {
                    if !util::is_trivial_numeric_literal(&literal) {
                        let value = util::numeric_value(&literal)
                            .map_or_else(|| literal.clone(), |value| value.to_string());
                        literals.entry(value).or_insert(literal);
                    }
                }
            }
            if literals.len() <= crate::MAGIC_LITERAL_THRESHOLD {
                return None;
            }
            let mut literal_names: Vec<_> = literals.into_values().collect();
            literal_names.sort();
            Some(
                auditor_core::finding!(
                    "magic_literal_cluster",
                    "lexical",
                    "Function clusters non-trivial numeric literals",
                    format!(
                        "Function `{}` contains {} distinct non-trivial numeric literals (> {}).",
                        span.name,
                        literal_names.len(),
                        crate::MAGIC_LITERAL_THRESHOLD
                    ),
                    Severity::Medium,
                    Confidence::Medium,
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation(
                    "Name numeric constants or move domain configuration out of the function.",
                )
                .with_evidence("function", span.name.clone())
                .with_evidence("distinct_count", literal_names.len())
                .with_evidence("threshold", crate::MAGIC_LITERAL_THRESHOLD)
                .with_evidence("literals", literal_names),
            )
        })
        .collect()
}

fn identifier_verbosity_per_file(
    rel: &str,
    language: &Language,
    lines: &[&str],
    spans: &[FunctionSpan],
) -> Vec<Finding> {
    const MIN_OCCURRENCES: usize = 5;
    const MIN_WORDS_PER_NAME: usize = 1;
    const VERBOSITY_THRESHOLD: f64 = 3.0;
    spans
        .iter()
        .filter_map(|span| {
            let occurrences = util::identifier_occurrences(language, lines, span, spans);
            if occurrences.len() < MIN_OCCURRENCES {
                return None;
            }
            let total_words: usize = occurrences
                .iter()
                .map(|(_, name)| util::split_identifier(name).len().max(MIN_WORDS_PER_NAME))
                .sum();
            let mean = total_words as f64 / occurrences.len() as f64;
            if mean <= VERBOSITY_THRESHOLD {
                return None;
            }
            Some(
                lexical_finding(
                    "identifier_verbosity",
                    "Function identifiers are overly verbose",
                    format!(
                        "Function `{}` averages {mean:.1} words per identifier (> {VERBOSITY_THRESHOLD}).",
                        span.name
                    ),
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation(
                    "Shorten locally obvious identifiers while preserving domain meaning.",
                )
                .with_evidence("function", span.name.clone())
                .with_evidence("identifier_count", occurrences.len())
                .with_evidence("mean_words", mean)
                .with_evidence("threshold", VERBOSITY_THRESHOLD as usize),
            )
        })
        .collect()
}

fn identifier_tersity_per_file(
    rel: &str,
    language: &Language,
    lines: &[&str],
    spans: &[FunctionSpan],
) -> Vec<Finding> {
    let allow: BTreeSet<&str> = [
        "a", "b", "c", "ch", "db", "f", "fs", "id", "i", "j", "k", "x", "y", "z", "ok", "n",
    ]
    .into_iter()
    .collect();
    spans
        .iter()
        .filter_map(|span| {
            let occurrences = util::identifier_occurrences(language, lines, span, spans);
            if occurrences.len() < 5 {
                return None;
            }
            let terse = occurrences
                .iter()
                .filter(|(_, name)| name.len() <= 2 && !allow.contains(name.as_str()))
                .count();
            let density = terse as f64 / occurrences.len() as f64;
            if density <= 0.50 {
                return None;
            }
            Some(
                lexical_finding(
                    "identifier_tersity",
                    "Function identifiers are cryptically terse",
                    format!(
                        "Function `{}` has terse identifier density {density:.2} (> 0.50).",
                        span.name
                    ),
                    rel,
                    Some(span.start_line),
                )
                .with_recommendation("Rename cryptic locals that carry domain meaning.")
                .with_evidence("function", span.name.clone())
                .with_evidence("identifier_count", occurrences.len())
                .with_evidence("terse_count", terse)
                .with_evidence("density", density),
            )
        })
        .collect()
}

fn lexical_finding(
    detector: &str,
    title: &'static str,
    message: String,
    path: &str,
    line: Option<usize>,
) -> Finding {
    auditor_core::finding!(
        detector,
        "lexical",
        title,
        message,
        Severity::Medium,
        Confidence::Medium,
        path,
        line,
    )
}

fn source_findings(
    files: &[crate::AuditedFile],
    cache: &crate::util::FileCache,
    detector: fn(&str, &Language, &[&str], &[crate::FunctionSpan]) -> Vec<Finding>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|file| file.language.is_source()) {
        let rel = crate::util::path_slash(&file.relative);
        let lines = cache.lines(file);
        let spans = crate::util::cached_function_spans_for(cache, file, &lines);
        findings.extend(detector(&rel, &file.language, &lines, &spans));
    }
    findings
}

pub(crate) fn section_divider_comment_findings(
    files: &[crate::AuditedFile],
    cache: &crate::util::FileCache,
) -> Vec<Finding> {
    source_findings(files, cache, section_divider_with_spans)
}

pub(crate) fn magic_literal_cluster_findings(
    files: &[crate::AuditedFile],
    cache: &crate::util::FileCache,
) -> Vec<Finding> {
    source_findings(files, cache, magic_literal_with_spans)
}

pub(crate) fn identifier_verbosity_findings(
    files: &[crate::AuditedFile],
    cache: &crate::util::FileCache,
) -> Vec<Finding> {
    source_findings(files, cache, identifier_verbosity_per_file)
}

pub(crate) fn identifier_tersity_findings(
    files: &[crate::AuditedFile],
    cache: &crate::util::FileCache,
) -> Vec<Finding> {
    source_findings(files, cache, identifier_tersity_per_file)
}

#[cfg(test)]
fn section_divider_per_file(rel: &str, language: &Language, lines: &[&str]) -> Vec<Finding> {
    let spans = util::function_spans_for(language, lines, rel);
    section_divider_with_spans(rel, language, lines, &spans)
}

#[cfg(test)]
fn magic_literal_per_file(rel: &str, language: &Language, lines: &[&str]) -> Vec<Finding> {
    let spans = util::function_spans_for(language, lines, rel);
    magic_literal_with_spans(rel, language, lines, &spans)
}

#[cfg(test)]
include!("lexical_tests.rs");

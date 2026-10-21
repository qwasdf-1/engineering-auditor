//! `catastrophic_backtracking_regex`: a regex literal whose pattern alone
//! admits exponential backtracking (ReDoS). Patterns come from each
//! backtracking engine's literals and compile calls: JavaScript regex
//! literals and `RegExp(..)`, Python `re`/`regex` calls, Java
//! `Pattern.compile` and the `String` regex methods, and .NET `Regex`
//! constructors, static methods, and `[GeneratedRegex]` (unless
//! `RegexOptions.NonBacktracking`), and C++ `std::regex` objects. Go's
//! `regexp` and Rust's `regex` are linear-time automata and never backtrack.

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::algorithmic_facts::{CostExpression, CostTermKind};
use crate::ast_scan::{self, AstScan};
use crate::js_api_identity::{js_arguments, js_string};
use crate::python_api_identity::{positional_arguments, py_string, PyScope};
use crate::regex_ambiguity::ambiguous_repetition;
use crate::{util, Language};

#[path = "regex_sites_tier_two.rs"]
mod tier_two;

pub(crate) fn catastrophic_backtracking_regex_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, backtracking_engine, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        for (site, pattern) in regex_literals(tree.root_node(), &file.language) {
            let Some(repetition) = ambiguous_repetition(&pattern) else {
                continue;
            };
            let line = site.start_position().0 + 1;
            let observed =
                CostExpression::term(CostTermKind::Exponential, repetition.as_str(), None);
            let alternative = CostExpression::term(CostTermKind::Linear, "input", None);
            findings.push(
                    auditor_core::finding!(
                        "catastrophic_backtracking_regex",
                        "algorithmic",
                        "Regex can backtrack exponentially",
                        format!(
                            "Pattern at line {line} repeats `{repetition}`, whose body can match the same text in more than one way; a near-miss input makes matching exponential"
                        ),
                        Severity::Medium,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Make each repetition match its text one way only (drop the inner quantifier, add a mandatory separator, or use a possessive quantifier or atomic group), or use a linear-time engine for untrusted input.",
                    )
                    .with_evidence("pattern", pattern.as_str())
                    .with_evidence("repetition", repetition.as_str())
                    .with_evidence("cost_model", "O(2^n) -> O(n)")
                    .with_evidence("cost_expression", observed.normalized())
                    .with_evidence("cost_terms", observed.terms())
                    .with_evidence("cost_composition", observed.composition())
                    .with_evidence("alternative_cost_expression", alternative.normalized())
                    .with_evidence("cost_confidence_boundary", "backtracking_engine_and_failing_input"),
                );
        }
    });
    findings
}

/// Languages whose standard regex engine backtracks.
fn backtracking_engine(language: &Language) -> bool {
    matches!(
        language,
        Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Java
            | Language::CSharp
            | Language::Cpp
    )
}

/// `(site, pattern)` of every regex the file writes as a literal.
fn regex_literals<'a>(root: AstNode<'a>, language: &Language) -> Vec<(AstNode<'a>, String)> {
    match language {
        Language::JavaScript | Language::TypeScript => js_regexes(root),
        Language::Python => python_regexes(root),
        Language::Java => java_regexes(root),
        Language::CSharp => tier_two::csharp_regexes(root),
        Language::Cpp => tier_two::cpp_regexes(root),
        _ => Vec::new(),
    }
}

fn js_regexes(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    let mut regexes = ast_scan::nodes_of_kind(root, &["regex"])
        .into_iter()
        .filter_map(|regex| {
            let pattern = regex.child_by_field_name("pattern")?;
            Some((regex, pattern.text().to_string()))
        })
        .collect::<Vec<_>>();
    for call in ast_scan::nodes_of_kind(root, &["call_expression", "new_expression"]) {
        let callee = call
            .child_by_field_name("function")
            .or_else(|| call.child_by_field_name("constructor"));
        if callee.is_none_or(|callee| callee.text() != "RegExp") {
            continue;
        }
        if let Some(pattern) = js_arguments(call)
            .first()
            .and_then(|first| js_string(*first))
        {
            regexes.push((call, unescape_string(&pattern)));
        }
    }
    regexes
}

const PYTHON_REGEX_FUNCTIONS: &[&str] = &[
    "compile",
    "match",
    "search",
    "fullmatch",
    "findall",
    "finditer",
    "sub",
    "subn",
    "split",
];

fn python_regexes(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    PyScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            let (module, function) = api.split_once('.')?;
            if !matches!(module, "re" | "regex") || !PYTHON_REGEX_FUNCTIONS.contains(&function) {
                return None;
            }
            let literal = *positional_arguments(call).first()?;
            let content = py_string(literal)?;
            // In a raw string the backslashes are the pattern's own.
            let raw = literal
                .text()
                .split(['"', '\''])
                .next()
                .is_some_and(|prefix| prefix.to_ascii_lowercase().contains('r'));
            Some((
                call,
                if raw {
                    content
                } else {
                    unescape_string(&content)
                },
            ))
        })
        .collect()
}

const JAVA_STRING_REGEX_METHODS: &[&str] = &["matches", "replaceAll", "replaceFirst", "split"];

fn java_regexes(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    ast_scan::nodes_of_kind(root, &["method_invocation"])
        .into_iter()
        .filter_map(|call| {
            let name = call.child_by_field_name("name")?.text();
            let pattern_compile = matches!(name, "compile" | "matches")
                && call
                    .child_by_field_name("object")
                    .is_some_and(|object| object.text().ends_with("Pattern"));
            if !pattern_compile && !JAVA_STRING_REGEX_METHODS.contains(&name) {
                return None;
            }
            let first = call.child_by_field_name("arguments")?.named_child(0)?;
            let text = first.text();
            if first.kind() != "string_literal" || text.starts_with("\"\"\"") {
                return None;
            }
            let content = text.get(1..text.len().checked_sub(1)?)?;
            Some((call, unescape_string(content)))
        })
        .collect()
}

/// A string literal's regex: `\\` is one backslash and `\"` a quote; other
/// escapes are left for the regex.
pub(super) fn unescape_string(content: &str) -> String {
    let mut pattern = String::with_capacity(content.len());
    let mut characters = content.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            pattern.push(character);
            continue;
        }
        match characters.next() {
            Some(quote @ ('"' | '\'')) => pattern.push(quote),
            Some('\\') => pattern.push('\\'),
            Some(other) => {
                pattern.push('\\');
                pattern.push(other);
            }
            None => pattern.push('\\'),
        }
    }
    pattern
}

//! Kotlin naming rules from the Kotlin coding conventions
//! (kotlinlang.org/docs/coding-conventions.html#naming-rules): constants in
//! SCREAMING_SNAKE_CASE, enum entries in PascalCase or SCREAMING_SNAKE_CASE,
//! camelCase constructor parameters, and a file holding a single class or
//! interface named after it.

use std::path::Path;

use super::*;

pub(super) fn findings(text: &str, relative: &str, min_len: usize, max_len: usize) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let code = util::strip_comments(line);
        let trimmed = code.trim();
        findings.extend(constant_findings(trimmed, relative, line_number));
        findings.extend(variable_findings(
            trimmed,
            relative,
            line_number,
            min_len,
            max_len,
        ));
        findings.extend(constructor_parameter_findings(
            trimmed,
            relative,
            line_number,
        ));
        findings.extend(enum_entry_findings(trimmed, relative, line_number));
    }
    findings.extend(declaration_file_findings(text, relative));
    findings
}

fn constant_findings(trimmed: &str, relative: &str, line_number: usize) -> Vec<Finding> {
    let Some(declaration) = trimmed.strip_prefix("const val ") else {
        return Vec::new();
    };
    let name = declaration.split([' ', '=']).next().unwrap_or("");
    if name.is_empty() || is_upper_snake(name) {
        return Vec::new();
    }
    vec![naming_finding(
        NamingKind::Constant,
        "UPPER_SNAKE_CASE",
        name,
        relative,
        line_number,
    )]
}

fn variable_findings(
    trimmed: &str,
    relative: &str,
    line_number: usize,
    min_len: usize,
    max_len: usize,
) -> Vec<Finding> {
    let Some(name) = variable_name(trimmed) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    if name.len() < min_len {
        findings.push(variable_length_finding(
            &name,
            relative,
            line_number,
            LengthLimit::Minimum(min_len),
        ));
    }
    if name.len() > max_len {
        findings.push(variable_length_finding(
            &name,
            relative,
            line_number,
            LengthLimit::Maximum(max_len),
        ));
    }
    findings
}

fn variable_name(trimmed: &str) -> Option<String> {
    ["val ", "var "].iter().find_map(|prefix| {
        let declaration = trimmed.strip_prefix(prefix)?;
        let name = declaration.split([' ', ':', '=', '(', ')', ',']).next()?;
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn constructor_parameter_findings(
    trimmed: &str,
    relative: &str,
    line_number: usize,
) -> Vec<Finding> {
    let Some(parameters) = class_parameters(trimmed) else {
        return Vec::new();
    };
    parameters
        .split(',')
        .filter_map(|parameter| {
            let name = parameter.split(':').next()?.trim();
            (!name.is_empty()
                && !name.starts_with("var ")
                && !name.starts_with("val ")
                && !starts_lowercase(name))
            .then(|| {
                naming_finding(
                    NamingKind::ConstructorParameter,
                    "camelCase",
                    name,
                    relative,
                    line_number,
                )
            })
        })
        .collect()
}

/// The primary constructor's parameter list (`class Name<T>(..)`,
/// `class Name private constructor(..)`), not a superclass call after `:`.
fn class_parameters(trimmed: &str) -> Option<&str> {
    let declaration = trimmed.strip_prefix("class ")?;
    let start = declaration.find('(')?;
    let head = &declaration[..start];
    if head.contains(':') || head.contains('=') {
        return None;
    }
    let tail = &declaration[start + 1..];
    let end = tail.find(')')?;
    Some(&tail[..end])
}

fn enum_entry_findings(trimmed: &str, relative: &str, line_number: usize) -> Vec<Finding> {
    if !trimmed.starts_with("enum class ") {
        return Vec::new();
    }
    enum_entries(trimmed)
        .into_iter()
        .filter(|entry| !starts_uppercase(entry) && !is_upper_snake(entry))
        .map(|entry| {
            naming_finding(
                NamingKind::EnumEntry,
                "PascalCase or UPPER_SNAKE_CASE",
                entry,
                relative,
                line_number,
            )
        })
        .collect()
}

fn enum_entries(line: &str) -> Vec<&str> {
    let Some(start) = line.find('{') else {
        return Vec::new();
    };
    let body = &line[start + 1..];
    let body = body.rsplit_once('}').map_or(body, |(body, _)| body);
    body.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn declaration_file_findings(text: &str, relative: &str) -> Vec<Finding> {
    let file_stem = Path::new(relative)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if file_stem.is_empty() {
        return Vec::new();
    }
    let declarations = text
        .lines()
        .enumerate()
        .filter_map(|(line, source)| top_level_declaration(source).map(|name| (line + 1, name)))
        .collect::<Vec<_>>();
    let [(line, declaration)] = declarations.as_slice() else {
        return Vec::new();
    };
    // Top-level functions, properties, or type aliases beside the class make
    // a file that is named for what it contains.
    if text.lines().any(declares_top_level_member) {
        return Vec::new();
    }
    // A leading `-` (`-DnsMessageReader.kt`) hides the file's JVM class from
    // Java callers, a convention okhttp uses; the declaration still names it.
    if declaration == file_stem.trim_start_matches('-') {
        return Vec::new();
    }
    vec![declaration_file_mismatch_finding(
        declaration,
        file_stem,
        relative,
        Some(*line),
    )]
}

const MODIFIERS: &[&str] = &[
    "public",
    "internal",
    "private",
    "expect",
    "actual",
    "data",
    "sealed",
    "abstract",
    "open",
    "inner",
    "value",
    "annotation",
    "fun",
    "enum",
    "const",
    "inline",
    "suspend",
    "operator",
    "infix",
    "tailrec",
    "external",
];

/// A top-level function, property, object, or type alias line.
fn declares_top_level_member(source: &str) -> bool {
    if source.starts_with(char::is_whitespace) {
        return false;
    }
    let code = util::strip_comments(source);
    let mut words = code
        .split_whitespace()
        .skip_while(|word| MODIFIERS.contains(word) && *word != "fun");
    matches!(
        words.next(),
        Some("fun" | "val" | "var" | "typealias" | "object")
    )
}

/// The class or interface a top-level line declares, past its modifiers
/// (`expect class Buffer`, `internal data class Point<T>`); a nested or
/// indented declaration is not top level.
fn top_level_declaration(source: &str) -> Option<String> {
    if source.starts_with(char::is_whitespace) {
        return None;
    }
    let code = util::strip_comments(source);
    let mut words = code
        .split_whitespace()
        .skip_while(|word| MODIFIERS.contains(word));
    if !matches!(words.next(), Some("class" | "interface")) {
        return None;
    }
    let name = words
        .next()?
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .next()?;
    (!name.is_empty()).then(|| name.to_string())
}

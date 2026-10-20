use auditor_ast::SymbolKind;
use auditor_core::{Confidence, Finding, Severity};

use crate::config::AuditorConfig;
use crate::structure_index::{StructureFile, StructureIndex, StructureSymbol};
use crate::{AuditedFile, Language};

use super::util;

#[path = "naming_java.rs"]
mod java;
#[path = "naming_kotlin.rs"]
mod kotlin;
#[path = "naming_rust_allows.rs"]
mod rust_allows;

pub(crate) fn naming_convention_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
    config: &AuditorConfig,
    structure: &StructureIndex,
) -> Vec<Finding> {
    let min_len = config.min_length_for("naming_convention", 1);
    let max_len = config.max_length_for("naming_convention", 64);
    let allowed = rust_allows::allowed_naming_lints(files, cache);
    let mut findings = semantic_naming_declaration_findings(structure, &allowed);
    for file in files {
        let relative = util::path_slash(&file.relative);
        match file.language {
            Language::Java => findings.extend(java::findings(cache.text(file), &relative)),
            Language::Kotlin => {
                findings.extend(kotlin::findings(
                    cache.text(file),
                    &relative,
                    min_len,
                    max_len,
                ));
            }
            _ => {}
        }
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    findings
}

fn semantic_naming_declaration_findings(
    index: &StructureIndex,
    allowed: &std::collections::BTreeMap<String, std::collections::BTreeSet<&'static str>>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in index.files().iter().filter(|file| file.emittable) {
        for symbol in &file.symbols {
            if symbol.in_test || symbol.name.is_empty() {
                continue;
            }
            let Some((kind, expected)) = naming_violation(file, symbol) else {
                continue;
            };
            // A Rust module may allow the naming lint (`#![allow(non_camel_case_types)]`).
            if rust_allows::lint_for(expected).is_some_and(|lint| {
                allowed
                    .get(&file.path)
                    .is_some_and(|lints| lints.contains(lint))
            }) {
                continue;
            }
            findings.push(naming_finding(
                kind,
                expected,
                &symbol.name,
                &file.path,
                symbol.line.saturating_sub(1),
            ));
        }
    }
    findings
}

fn naming_violation(
    file: &StructureFile,
    symbol: &StructureSymbol,
) -> Option<(NamingKind, &'static str)> {
    match file.language {
        Language::Java => java_naming_violation(symbol),
        Language::Kotlin => kotlin_naming_violation(symbol),
        Language::CSharp => csharp_naming_violation(symbol),
        Language::Go => go_naming_violation(symbol),
        Language::Rust => rust_naming_violation(symbol),
        Language::Python => python_naming_violation(file, symbol),
        Language::JavaScript | Language::TypeScript => js_naming_violation(symbol),
        _ => None,
    }
}

/// RFC 430: types and traits in UpperCamelCase; functions, methods, and
/// modules in snake_case; constants and statics in SCREAMING_SNAKE_CASE.
/// A trait impl's items take the trait's names and are not judged.
fn rust_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    let name = symbol.name.trim_start_matches("r#");
    if symbol.implicitly_used {
        return None;
    }
    match symbol.kind {
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Trait
        | SymbolKind::TypeAlias
        | SymbolKind::Union => {
            (!is_upper_camel(name)).then_some((NamingKind::Type, "UpperCamelCase"))
        }
        SymbolKind::Function | SymbolKind::Method => {
            (!is_lower_snake(name)).then_some((NamingKind::Function, "snake_case"))
        }
        SymbolKind::Module => (!is_lower_snake(name)).then_some((NamingKind::Type, "snake_case")),
        SymbolKind::Constant | SymbolKind::Static => (name.chars().any(char::is_lowercase))
            .then_some((NamingKind::Constant, "SCREAMING_SNAKE_CASE")),
        _ => None,
    }
}

/// PEP 8 (pep8-naming N801, N802): classes in CapWords, functions and
/// methods in lowercase. A method of a class with a base may implement an
/// external protocol (`setUp`, `visit_Name`, `do_GET`) and is not judged,
/// nor is a `ctypes`/`cffi` binding module, whose names mirror the C API
/// (PEP 8's consistency with an external API).
fn python_naming_violation(
    file: &StructureFile,
    symbol: &StructureSymbol,
) -> Option<(NamingKind, &'static str)> {
    let binding_module = file.imports.iter().any(|import| {
        [import.module.as_deref(), import.canonical.as_deref()]
            .into_iter()
            .flatten()
            .any(|module| matches!(module.split('.').next(), Some("ctypes" | "cffi")))
    });
    if binding_module {
        return None;
    }
    let name = symbol.name.trim_start_matches('_');
    match symbol.kind {
        SymbolKind::Class => (!is_upper_camel(name)).then_some((NamingKind::Type, "CapWords")),
        SymbolKind::Function | SymbolKind::Method => {
            let protocol_method = symbol
                .parent
                .and_then(|parent| file.symbols.get(parent as usize))
                .is_some_and(|parent| {
                    parent.kind == SymbolKind::Class
                        && parent.base.as_deref().is_some_and(|base| base != "object")
                });
            (!protocol_method && !symbol.name.starts_with("__") && !is_lower_snake(name))
                .then_some((NamingKind::Function, "lowercase_with_underscores"))
        }
        _ => None,
    }
}

/// Common JavaScript and TypeScript style (Airbnb, Google, typescript-eslint
/// `naming-convention` defaults): classes in PascalCase, and no snake_case
/// functions or methods. A PascalCase function is a component or
/// constructor, and a leading `_` or `$` marks a private or internal name.
fn js_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    // A leading `$` or `_` marks an internal or private name.
    let name = symbol.name.trim_matches(['_', '$']);
    match symbol.kind {
        // An interface may describe a callable hook (`useQuery`) or an
        // internal marker type, so only classes are judged.
        SymbolKind::Class => (!starts_uppercase(name)).then_some((NamingKind::Type, "PascalCase")),
        SymbolKind::Function | SymbolKind::Method => {
            // React-style stability prefixes are part of the API name.
            let name = ["unstable_", "experimental_", "UNSAFE_"]
                .iter()
                .find_map(|prefix| name.strip_prefix(prefix))
                .unwrap_or(name);
            let snake = name.contains('_')
                && name.chars().any(char::is_lowercase)
                && name
                    .split('_')
                    .all(|part| !part.chars().any(char::is_uppercase));
            snake.then_some((NamingKind::Function, "camelCase"))
        }
        _ => None,
    }
}

fn is_upper_camel(name: &str) -> bool {
    starts_uppercase(name) && !name.contains('_')
}

fn is_lower_snake(name: &str) -> bool {
    !name.chars().any(char::is_uppercase)
}

fn java_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    match symbol.kind {
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum => {
            (!starts_uppercase(&symbol.name)).then_some((NamingKind::Type, "PascalCase"))
        }
        SymbolKind::Method => {
            (!starts_lowercase(&symbol.name)).then_some((NamingKind::Method, "camelCase"))
        }
        _ => None,
    }
}

fn kotlin_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    match symbol.kind {
        SymbolKind::Class | SymbolKind::Object | SymbolKind::Interface | SymbolKind::Enum => {
            (!starts_uppercase(&symbol.name)).then_some((NamingKind::Type, "PascalCase"))
        }
        // Kotlin's conventions let a factory function share its type's
        // PascalCase name (Compose's `@Composable` functions do too), so only
        // underscores break camelCase.
        // An `external` function binds a foreign symbol's own name.
        SymbolKind::Function | SymbolKind::Method => (symbol.name.contains('_')
            && !symbol
                .declaration
                .as_deref()
                .is_some_and(|header| header.split_whitespace().any(|word| word == "external")))
        .then_some((NamingKind::Function, "camelCase")),
        _ => None,
    }
}

/// The .NET Framework Design Guidelines: types and methods in PascalCase,
/// interfaces prefixed with `I`. Generated event handlers (`button1_Click`)
/// keep their underscore.
fn csharp_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    match symbol.kind {
        SymbolKind::Interface => {
            let mut letters = symbol.name.chars();
            (!(letters.next() == Some('I') && letters.next().is_some_and(char::is_uppercase)))
                .then_some((NamingKind::Type, "IPascalCase"))
        }
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum => {
            (!starts_uppercase(&symbol.name)).then_some((NamingKind::Type, "PascalCase"))
        }
        SymbolKind::Method | SymbolKind::Function => {
            (!starts_uppercase(&symbol.name)).then_some((NamingKind::Method, "PascalCase"))
        }
        _ => None,
    }
}

fn go_naming_violation(symbol: &StructureSymbol) -> Option<(NamingKind, &'static str)> {
    matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
        .then(|| symbol.name.contains('_'))
        .filter(|violation| *violation)
        .map(|_| (NamingKind::Function, "camelCase or PascalCase"))
}

fn starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn starts_lowercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

fn is_upper_snake(name: &str) -> bool {
    name.chars()
        .all(|character| character.is_uppercase() || character.is_ascii_digit() || character == '_')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NamingKind {
    Type,
    Method,
    Constant,
    Function,
    ConstructorParameter,
    EnumEntry,
}

impl std::fmt::Display for NamingKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Type => "Type",
            Self::Method => "Method",
            Self::Constant => "Constant",
            Self::Function => "Function",
            Self::ConstructorParameter => "ConstructorParameter",
            Self::EnumEntry => "EnumEntry",
        };
        formatter.write_str(label)
    }
}

fn naming_finding(
    kind: NamingKind,
    expected: &str,
    name: &str,
    relative: &str,
    zero_based_line: usize,
) -> Finding {
    auditor_core::finding!(
        "naming_convention",
        "lexical",
        "Name violates naming convention",
        format!(
            "{kind} `{name}` at line {} should be {expected}",
            zero_based_line + 1
        ),
        Severity::Low,
        Confidence::High,
        relative,
        Some(zero_based_line + 1),
    )
    .with_recommendation(format!("Rename {kind} to use {expected}."))
    .with_evidence("name", name.to_string())
    .with_identity("kind", kind.to_string())
    .with_identity("name", name)
}

#[derive(Clone, Copy)]
enum LengthLimit {
    Minimum(usize),
    Maximum(usize),
}

fn variable_length_finding(
    name: &str,
    relative: &str,
    zero_based_line: usize,
    limit: LengthLimit,
) -> Finding {
    let (direction, limit, recommendation) = match limit {
        LengthLimit::Minimum(limit) => (
            "short",
            limit,
            format!("Rename to at least {limit} characters."),
        ),
        LengthLimit::Maximum(limit) => (
            "long",
            limit,
            format!("Rename to at most {limit} characters."),
        ),
    };
    auditor_core::finding!(
        "naming_convention",
        "lexical",
        "Variable name length is outside configured limits",
        format!(
            "Variable `{name}` at line {} is {} chars ({direction} limit {limit})",
            zero_based_line + 1,
            name.len()
        ),
        Severity::Low,
        Confidence::Medium,
        relative,
        Some(zero_based_line + 1),
    )
    .with_recommendation(recommendation)
    .with_evidence("name", name.to_string())
    .with_evidence("limit", limit)
    .with_identity("direction", direction)
    .with_identity("name", name)
}

fn declaration_file_mismatch_finding(
    declaration: &str,
    file_stem: &str,
    relative: &str,
    line: Option<usize>,
) -> Finding {
    auditor_core::finding!(
        "naming_convention",
        "lexical",
        "File name does not match top-level declaration",
        format!(
            "File `{relative}` contains declaration `{declaration}` but is named `{file_stem}.kt`"
        ),
        Severity::Low,
        Confidence::Medium,
        relative,
        line,
    )
    .with_recommendation(format!(
        "Rename the file to `{declaration}.kt` or rename the declaration to `{file_stem}`."
    ))
    .with_evidence("declaration", declaration.to_string())
    .with_identity("declaration", declaration)
    .with_evidence("filename", file_stem.to_string())
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;

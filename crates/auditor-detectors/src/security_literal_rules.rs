//! Literal-based security rules: credential-named targets bound to string
//! literals (with entropy evidence) and hardcoded temp directories, across
//! Python, Rust, JavaScript/TypeScript, Go, Java, Kotlin, C#, C, and C++.

use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::security_rules::{rule_finding, Rule};
use crate::{util, AuditorConfig, Language};

#[path = "security_literal_bindings.rs"]
mod bindings;
use bindings::{bound_string, defines_temp_directory, rust_assigned_string, string_value};

const HARDCODED_CREDENTIAL: Rule = Rule {
    id: "hardcoded_credential",
    title: "Potential hardcoded credential",
    severity: Severity::Medium,
    confidence: Confidence::Medium,
    recommendation: "Move credentials to environment variables or a secrets manager.",
    cwe: "CWE-259",
};

const HARDCODED_TMP_DIRECTORY: Rule = Rule {
    id: "hardcoded_tmp_directory",
    title: "Hardcoded temp directory (Bandit B108)",
    severity: Severity::Medium,
    confidence: Confidence::Medium,
    recommendation: "Use tempfile.mkstemp() or a dedicated temp directory.",
    cwe: "CWE-377",
};

pub(crate) fn shannon_entropy(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    let mut frequencies = [0usize; 256];
    for byte in value.bytes() {
        frequencies[byte as usize] += 1;
    }
    let length = value.len() as f64;
    frequencies
        .into_iter()
        .filter(|count| *count > 0)
        .map(|count| {
            let probability = count as f64 / length;
            -probability * probability.log2()
        })
        .sum()
}

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        if !matches!(character, '"' | '\'' | '`') {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for inner in chars.by_ref() {
            if escaped {
                escaped = false;
                value.push(inner);
            } else if inner == '\\' {
                escaped = true;
            } else if inner == character {
                break;
            } else {
                value.push(inner);
            }
        }
        if !value.is_empty() {
            literals.push(value);
        }
    }
    literals
}

fn credential_context(target: &str) -> bool {
    let normalized = target
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "password",
        "passwd",
        "secret",
        "apikey",
        "accesstoken",
        "authtoken",
        "credential",
        "clientsecret",
        "privatekey",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

fn assignment_separator(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut quotes = util::QuoteState::default();
    for (index, ch) in code.char_indices() {
        if quotes.consumes(ch) || ch != '=' {
            continue;
        }
        let before = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let after = bytes.get(index + 1).copied();
        if !matches!(before, Some(b'=' | b'!' | b'>' | b'<')) && !matches!(after, Some(b'=' | b'>'))
        {
            return Some(index);
        }
    }
    None
}

fn assignment_target(lhs: &str) -> Option<String> {
    lhs.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .rfind(|part| !part.is_empty())
        .map(str::to_string)
}

fn assigned_string(line: &str) -> Option<(String, String)> {
    let code = util::strip_comments_preserve_literals(line);
    let (lhs, rhs) = if let Some(index) = assignment_separator(&code) {
        (&code[..index], &code[index + 1..])
    } else {
        code.split_once(':')?
    };
    let target = assignment_target(lhs)?;
    let rhs = rhs.trim_start();
    let literal_start = rhs
        .char_indices()
        .find(|(_, ch)| matches!(ch, '\'' | '"' | '`'))?;
    if literal_start.0 > 2
        || !rhs[..literal_start.0]
            .chars()
            .all(|ch| matches!(ch, 'r' | 'b' | 'f' | 'u' | 'R' | 'B' | 'F' | 'U'))
    {
        return None;
    }
    let value = string_literals(rhs).into_iter().next()?;
    credential_literal(target, value)
}

/// `(target, value)` when a credential-named target holds a literal that is
/// not a placeholder.
fn credential_literal(target: String, value: String) -> Option<(String, String)> {
    if !credential_context(&target) || names_a_credential(&target, &value) {
        return None;
    }
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() < 4
        || value.contains('$')
        || [
            "password",
            "secret",
            "changeme",
            "example",
            "placeholder",
            "test",
            "xxx",
            "none",
            "provider",
            "warning",
            "api_key=",
        ]
        .contains(&normalized.as_str())
        || ["paste your", "enter your", "api key here", "token here"]
            .iter()
            .any(|marker| normalized.contains(marker))
    {
        return None;
    }
    Some((target, value))
}

/// A target or literal that names where a credential lives rather than
/// holding one: `API_KEY_ENV = "SERVICE_API_KEY"`, `TOKEN_HEADER = ".."`,
/// a property key (`SOCKS_PASSWORD = "java.net.socks.password"`), or a
/// character set with no letter or digit (`PASSWORD_ENCODE_SET = " \"':;"`).
fn names_a_credential(target: &str, value: &str) -> bool {
    let target = target.to_ascii_lowercase();
    let environment_name = value.contains('_')
        && value.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        });
    let property_key = value.contains('.')
        && credential_context(value)
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || ".-_".contains(character)
        });
    let symbols_only = !value.chars().any(|character| character.is_alphanumeric());
    environment_name
        || property_key
        || symbols_only
        || ["_env", "_var", "_name", "_header", "_field", "_param"]
            .iter()
            .any(|suffix| target.ends_with(suffix))
}

/// A plain or high-entropy credential finding for `target = value`.
fn credential_finding(
    rel: &str,
    line_number: usize,
    (target, value): (String, String),
    entropy_threshold: f64,
) -> Finding {
    let entropy = shannon_entropy(&value);
    let high_entropy = value.len() >= 8 && entropy >= entropy_threshold;
    let finding = if high_entropy {
        auditor_core::finding!(
            HARDCODED_CREDENTIAL.id,
            "correctness",
            "Potential hardcoded credential (high-entropy string)",
            format!(
                "High-entropy string on credential-related line {} may be a secret",
                line_number
            ),
            HARDCODED_CREDENTIAL.severity,
            HARDCODED_CREDENTIAL.confidence,
            rel,
            Some(line_number),
        )
        .with_evidence("entropy", format!("{entropy:.2}"))
        .with_evidence("value_length", value.len())
    } else {
        auditor_core::finding!(
            HARDCODED_CREDENTIAL.id,
            "correctness",
            HARDCODED_CREDENTIAL.title,
            format!(
                "Credential-like variable `{target}` is assigned a literal at line {line_number}"
            ),
            HARDCODED_CREDENTIAL.severity,
            HARDCODED_CREDENTIAL.confidence,
            rel,
            Some(line_number),
        )
    };
    finding
        .with_recommendation(HARDCODED_CREDENTIAL.recommendation)
        .with_evidence("target", target.clone())
        .with_evidence("cwe", HARDCODED_CREDENTIAL.cwe)
        // Credential lines must never be content-hashed into identity.
        .with_identity("target", target)
}

pub(crate) fn hardcoded_credential_findings(
    scan: AstScan<'_, '_>,
    config: &AuditorConfig,
) -> Vec<Finding> {
    let threshold = config.entropy_threshold_for(HARDCODED_CREDENTIAL.id, 4.5);
    let mut findings = Vec::new();
    visit_table_nodes(scan, CREDENTIAL_ASSIGNMENTS, |language, rel, node| {
        let credential = match language {
            Language::Python => assigned_string(node.text()),
            Language::Rust => rust_assigned_string(node),
            _ => bound_string(node),
        };
        if let Some(credential) = credential {
            let line = node.start_position().0 + 1;
            findings.push(credential_finding(rel, line, credential, threshold));
        }
    });
    findings
}

/// Visits each node of the kinds `table` lists for its file's language,
/// outside Rust tests, with the file's language and slash path.
fn visit_table_nodes(
    scan: AstScan<'_, '_>,
    table: &[(Language, &[&str])],
    mut visit: impl FnMut(&Language, &str, auditor_ast::AstNode<'_>),
) {
    ast_scan::visit_ast_files(
        scan,
        |language| table.iter().any(|(owner, _)| owner == language),
        |file, tree, _| {
            let Some((_, kinds)) = table.iter().find(|(owner, _)| *owner == file.language) else {
                return;
            };
            let rel = util::path_slash(&file.relative);
            for node in ast_scan::nodes_of_kind(tree.root_node(), kinds)
                .into_iter()
                .filter(|node| !ast_scan::is_inside_rust_test(*node))
            {
                visit(&file.language, &rel, node);
            }
        },
    );
}

/// The assignment-like node kinds each language binds a value with.
const CREDENTIAL_ASSIGNMENTS: &[(Language, &[&str])] = &[
    (Language::Python, &["assignment"]),
    (
        Language::Rust,
        &[
            "let_declaration",
            "const_item",
            "static_item",
            "assignment_expression",
            "field_initializer",
        ],
    ),
    (
        Language::JavaScript,
        &[
            "variable_declarator",
            "assignment_expression",
            "pair",
            "public_field_definition",
        ],
    ),
    (
        Language::TypeScript,
        &[
            "variable_declarator",
            "assignment_expression",
            "pair",
            "public_field_definition",
        ],
    ),
    (
        Language::Go,
        &[
            "short_var_declaration",
            "assignment_statement",
            "const_spec",
            "var_spec",
            "keyed_element",
        ],
    ),
    (
        Language::Java,
        &["variable_declarator", "assignment_expression"],
    ),
    (Language::Kotlin, &["property_declaration", "assignment"]),
    (
        Language::CSharp,
        &["variable_declarator", "assignment_expression"],
    ),
    (Language::C, &["init_declarator", "assignment_expression"]),
    (Language::Cpp, &["init_declarator", "assignment_expression"]),
];

/// The string literal kinds of each language.
const STRING_LITERALS: &[(Language, &[&str])] = &[
    (Language::Python, &["string"]),
    (Language::Rust, &["string_literal", "raw_string_literal"]),
    (Language::JavaScript, &["string", "template_string"]),
    (Language::TypeScript, &["string", "template_string"]),
    (
        Language::Go,
        &["interpreted_string_literal", "raw_string_literal"],
    ),
    (Language::Java, &["string_literal"]),
    (Language::Kotlin, &["string_literal"]),
    (
        Language::CSharp,
        &[
            "string_literal",
            "verbatim_string_literal",
            "raw_string_literal",
        ],
    ),
    (Language::C, &["string_literal"]),
    (Language::Cpp, &["string_literal", "raw_string_literal"]),
];

pub(crate) fn hardcoded_tmp_directory_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    visit_table_nodes(scan, STRING_LITERALS, |language, rel, node| {
        let value = match language {
            Language::Python => string_literals(node.text()).into_iter().next(),
            _ => string_value(node),
        };
        // The directory or a path under it; `/tmp:rw,..` is a mount spec.
        // Code that defines the temp directory names it by necessity.
        if !defines_temp_directory(node)
            && value.is_some_and(|value| {
                ["/tmp", "/var/tmp", "/dev/shm"].iter().any(|directory| {
                    value
                        .strip_prefix(directory)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                })
            })
        {
            findings.push(rule_finding(
                HARDCODED_TMP_DIRECTORY,
                rel,
                node.start_position().0 + 1,
                node.text(),
            ));
        }
    });
    findings
}

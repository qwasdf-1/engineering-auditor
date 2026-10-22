//! `eager_log_formatting`: a debug or trace log call whose message is
//! formatted before the call (an f-string, `%` or `.format`, `String.format`,
//! `fmt.Sprintf`, a template literal, or concatenation with a value). The
//! logger checks the level inside the call, so when debug logging is off,
//! as it usually is in production, the string is built and thrown away on
//! every call. Passing the values as arguments (`logger.debug("x=%s", x)`,
//! SLF4J `{}` placeholders, `Debugf`, structured fields) defers formatting
//! until the level is known to be enabled.
//!
//! The receiver must name a logger (`logger`, `log`, `LOG`, `self._log`,
//! `logging`, `slog`, `requestLogger`); `console` is not one, since browsers
//! and Node format its output whatever the level. A call inside an
//! `isDebugEnabled()`-style guard is already lazy. Rust's `log` and
//! `tracing` macros check the level before evaluating their arguments.

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, descendants, nodes_of_kind, AstScan};
use crate::{util, Language};

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
    )
}

pub(crate) fn eager_log_formatting_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, supported, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let kinds: &[&str] = match file.language {
            Language::Python => &["call"],
            Language::Java => &["method_invocation"],
            _ => &["call_expression"],
        };
        findings.extend(
            nodes_of_kind(tree.root_node(), kinds)
                .into_iter()
                .filter_map(|call| eager_debug_log(call, &file.language))
                .map(|(call, method, message)| finding(call, method, message, &relative)),
        );
    });
    findings
}

/// The call, its level method, and its message, when `call` logs at debug
/// or trace level a message formatted before the call.
fn eager_debug_log<'a>(
    call: AstNode<'a>,
    language: &Language,
) -> Option<(AstNode<'a>, &'a str, AstNode<'a>)> {
    let (receiver, method) = match language {
        Language::Java => (
            call.child_by_field_name("object")?,
            call.child_by_field_name("name")?,
        ),
        _ => {
            let function = call.child_by_field_name("function")?;
            let (object, field) = match function.kind() {
                "attribute" => ("object", "attribute"),
                "member_expression" => ("object", "property"),
                "selector_expression" => ("operand", "field"),
                _ => return None,
            };
            (
                function.child_by_field_name(object)?,
                function.child_by_field_name(field)?,
            )
        }
    };
    let level = method.text().to_ascii_lowercase();
    if !matches!(level.as_str(), "debug" | "trace") || !names_logger(receiver.text()) {
        return None;
    }
    let message = call
        .child_by_field_name("arguments")?
        .named_children()
        .find(|argument| argument.kind() != "comment")?;
    (eager(message, language) && !guarded(call)).then_some((call, method.text(), message))
}

/// Whether a receiver names a logger: its last segment is `log`, `logger`,
/// `logging`, `slog`, or `logrus`, or its last word is `log` or `logger`.
fn names_logger(receiver: &str) -> bool {
    let last = receiver
        .rsplit(['.', ':', '>'])
        .find(|part| !part.is_empty())
        .unwrap_or(receiver);
    let words = util::split_identifier(last);
    matches!(
        last.trim_matches('_').to_ascii_lowercase().as_str(),
        "logging" | "slog" | "logrus"
    ) || words
        .last()
        .is_some_and(|word| word == "log" || word == "logger")
}

/// Whether `message` builds a string from values at the call site.
fn eager(message: AstNode<'_>, language: &Language) -> bool {
    match message.kind() {
        "parenthesized_expression" => message
            .named_child(0)
            .is_some_and(|inner| eager(inner, language)),
        // Python f-strings and JS/TS template literals with a substitution.
        "string" => descendants(message).any(|child| child.kind() == "interpolation"),
        "template_string" => {
            descendants(message).any(|child| child.kind() == "template_substitution")
        }
        "binary_operator" | "binary_expression" => {
            let Some(operator) = message.child_by_field_name("operator") else {
                return false;
            };
            let left = message.child_by_field_name("left");
            let right = message.child_by_field_name("right");
            match operator.kind() {
                "%" => *language == Language::Python && left.is_some_and(is_string),
                "+" => {
                    let operands = [left, right];
                    operands.iter().flatten().any(|operand| is_string(*operand))
                        && operands
                            .iter()
                            .flatten()
                            .any(|operand| !is_literal(*operand))
                }
                _ => false,
            }
        }
        "call" | "call_expression" | "method_invocation" => formats(message, language),
        _ => false,
    }
}

/// `"..".format(..)`, `String.format(..)`, `"..".formatted(..)`, or
/// `fmt.Sprintf(..)`.
fn formats(call: AstNode<'_>, language: &Language) -> bool {
    let (receiver, method) = match language {
        Language::Java => (
            call.child_by_field_name("object"),
            call.child_by_field_name("name"),
        ),
        _ => {
            let Some(function) = call.child_by_field_name("function") else {
                return false;
            };
            (
                function
                    .child_by_field_name("object")
                    .or_else(|| function.child_by_field_name("operand")),
                function
                    .child_by_field_name("attribute")
                    .or_else(|| function.child_by_field_name("field"))
                    .or_else(|| function.child_by_field_name("property")),
            )
        }
    };
    let (Some(receiver), Some(method)) = (receiver, method) else {
        return false;
    };
    match (language, method.text()) {
        (Language::Python, "format") => is_string(receiver),
        (Language::Java, "format") => receiver.text() == "String",
        (Language::Java, "formatted") => is_string(receiver),
        (Language::Go, "Sprintf") => receiver.text() == "fmt",
        _ => false,
    }
}

fn is_string(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "string"
            | "string_literal"
            | "template_string"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "concatenated_string"
    ) || (matches!(node.kind(), "binary_operator" | "binary_expression")
        && node.child_by_field_name("left").is_some_and(is_string))
}

/// A literal, or an operation over literals only.
fn is_literal(node: AstNode<'_>) -> bool {
    match node.kind() {
        "string" => !descendants(node).any(|child| child.kind() == "interpolation"),
        "template_string" => {
            !descendants(node).any(|child| child.kind() == "template_substitution")
        }
        "binary_operator" | "binary_expression" => ["left", "right"]
            .iter()
            .all(|field| node.child_by_field_name(field).is_some_and(is_literal)),
        kind => {
            is_string(node)
                || kind.contains("integer")
                || kind.contains("float")
                || matches!(kind, "number" | "true" | "false")
        }
    }
}

/// Whether an enclosing `if` checks that the level is enabled
/// (`isDebugEnabled()`, `isEnabledFor(DEBUG)`, `Enabled(ctx, slog.LevelDebug)`).
fn guarded(call: AstNode<'_>) -> bool {
    std::iter::successors(call.parent(), |node| node.parent())
        .filter(|node| matches!(node.kind(), "if_statement" | "if_expression"))
        .filter_map(|node| node.child_by_field_name("condition"))
        .any(|condition| condition.text().to_ascii_lowercase().contains("enabled"))
}

fn finding(call: AstNode<'_>, method: &str, message: AstNode<'_>, relative: &str) -> Finding {
    let line = call.start_position().0 + 1;
    let summary = ast_scan::source_summary(message);
    auditor_core::finding!(
        "eager_log_formatting",
        "optimization",
        "Eagerly formatted debug log",
        format!(
            "The `{method}` call at line {line} formats its message before the logger checks the level, so the string is built even when {} logging is off",
            method.to_ascii_lowercase()
        ),
        Severity::Low,
        Confidence::High,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Pass the values as arguments so the logger formats only enabled messages: `logger.debug(\"x=%s\", x)`, SLF4J `{}` placeholders, `Debugf` or structured fields.",
    )
    .with_identity("message", summary.as_str())
}

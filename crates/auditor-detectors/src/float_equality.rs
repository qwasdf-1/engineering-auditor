//! `float_equality_comparison`: `==` or `!=` between the result of float
//! arithmetic and a float literal that binary floating point cannot
//! represent exactly (`0.1`, `0.3`, `1.1`). The literal becomes the nearest
//! double, and arithmetic rounds at each step, so the same number reached
//! by computation usually differs from it (`0.1 + 0.2 == 0.3` is false).
//! The computed side is an arithmetic expression or a local whose reaching
//! value is one (or a `+=` accumulation), found through the data-flow
//! layer. A value parsed or assigned from the same literal text compares
//! equal and is not reported, nor is a literal exact in binary (`0.0`,
//! `0.5`, `1.0`), a common deliberate sentinel. Kotlin and C# reals and C
//! and C++ number literals are read with their suffixes (`0.1f`, `0.1L`);
//! a C# `m` literal is a base-10 `decimal`, exact, and not a float.

use auditor_ast::{AstNode, BindingKind, FunctionFlow};
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::Language;

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::Kotlin
            | Language::CSharp
            | Language::C
            | Language::Cpp
    )
}

pub(crate) fn float_equality_comparison_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    ast_scan::kind_findings(
        scan,
        supported,
        comparison_kinds,
        |node, language, relative| {
            if *language == Language::Rust && ast_scan::is_inside_rust_test(node) {
                return None;
            }
            let (comparison, literal) = inexact_comparison(node, language)?;
            Some(finding(comparison, literal, relative))
        },
    )
}

fn comparison_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Python => &["comparison_operator"],
        Language::Kotlin => &["equality_expression"],
        _ => &["binary_expression"],
    }
}

/// The comparison and its literal, when `node` tests a computed value for
/// equality with a float literal that is inexact in binary.
fn inexact_comparison<'a>(
    node: AstNode<'a>,
    language: &Language,
) -> Option<(AstNode<'a>, AstNode<'a>)> {
    let (left, operator, right) = match language {
        // Python comparisons and Kotlin equality expressions have no fields.
        Language::Python | Language::Kotlin => {
            if node.named_child_count() != 2 {
                return None;
            }
            (node.named_child(0)?, node.child(1)?, node.named_child(1)?)
        }
        _ => (
            node.child_by_field_name("left")?,
            node.child_by_field_name("operator")?,
            node.child_by_field_name("right")?,
        ),
    };
    if !matches!(operator.kind(), "==" | "!=" | "===" | "!==") {
        return None;
    }
    let (literal, other) = match (
        float_literal(left, language),
        float_literal(right, language),
    ) {
        (Some(literal), None) => (literal, right),
        (None, Some(literal)) => (literal, left),
        _ => return None,
    };
    let numeric_other = matches!(
        other.kind(),
        "integer" | "integer_literal" | "int_literal" | "number" | "decimal_integer_literal"
    );
    (!numeric_other && !exact_in_binary(literal.text()) && computed(other))
        .then_some((node, literal))
}

/// Whether `operand` is float arithmetic: an arithmetic expression, or a
/// name whose reaching value is one or which a `+=`-style update reaches.
fn computed(operand: AstNode<'_>) -> bool {
    if arithmetic(operand) {
        return true;
    }
    if !matches!(operand.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    FunctionFlow::enclosing(operand)
        .reaching(operand)
        .iter()
        .any(|binding| {
            binding.kind == BindingKind::Augmented || binding.value.is_some_and(arithmetic)
        })
}

fn arithmetic(node: AstNode<'_>) -> bool {
    match node.kind() {
        "parenthesized_expression" => node.named_child(0).is_some_and(arithmetic),
        "binary_expression" | "binary_operator" => node
            .child_by_field_name("operator")
            .is_some_and(|operator| matches!(operator.kind(), "+" | "-" | "*" | "/")),
        // Kotlin's arithmetic expressions have no operator field.
        "additive_expression" | "multiplicative_expression" => node
            .child(1)
            .is_some_and(|operator| matches!(operator.kind(), "+" | "-" | "*" | "/")),
        _ => false,
    }
}

/// The float literal `node` is, through a unary minus or parentheses.
fn float_literal<'a>(node: AstNode<'a>, language: &Language) -> Option<AstNode<'a>> {
    match node.kind() {
        "unary_expression"
        | "unary_operator"
        | "prefix_expression"
        | "prefix_unary_expression"
        | "parenthesized_expression" => float_literal(
            node.named_child(node.named_child_count().checked_sub(1)?)?,
            language,
        ),
        "float_literal" | "float" | "decimal_floating_point_literal" => Some(node),
        // Kotlin and C# reals; a C# `m` literal is a base-10 `decimal`, exact.
        "real_literal" => (!node.text().to_ascii_lowercase().ends_with('m')).then_some(node),
        // C and C++ number literals that are decimal floats (`0.1`, `1e-1f`).
        "number_literal" => {
            let text = node.text().to_ascii_lowercase();
            (!text.starts_with("0x") && (text.contains('.') || text.contains('e'))).then_some(node)
        }
        "number" if matches!(language, Language::JavaScript | Language::TypeScript) => {
            let text = node.text().to_ascii_lowercase();
            let decimal =
                !text.starts_with("0x") && !text.starts_with("0b") && !text.starts_with("0o");
            (decimal && (text.contains('.') || text.contains('e'))).then_some(node)
        }
        _ => None,
    }
}

/// Whether a decimal float literal names a value binary floating point holds
/// exactly: once the factors of 5 in its denominator cancel, the
/// denominator must be a power of two and the significand must fit (53
/// bits, or 24 for an `f32`/`f` literal; a C `l` suffix is read as a
/// double). Unparsed forms count as exact.
pub(crate) fn exact_in_binary(literal: &str) -> bool {
    let Some((numerator, scale, single)) = decimal_parts(literal) else {
        return true;
    };
    let bits = if single { 24 } else { 53 };
    odd_part(numerator, scale).is_some_and(|odd| odd < (1u128 << bits))
}

/// A literal as `numerator * 10^scale` (no trailing zeros in the
/// numerator), and whether it is an `f32`/`f` literal.
fn decimal_parts(literal: &str) -> Option<(u128, i32, bool)> {
    let lower = literal
        .trim_start_matches(['-', '+'])
        .to_ascii_lowercase()
        .replace('_', "");
    let single = lower.ends_with("f32") || (lower.ends_with('f') && !lower.starts_with("0x"));
    let body = lower
        .trim_end_matches("f32")
        .trim_end_matches("f64")
        .trim_end_matches(['f', 'd', 'l']);
    let (mantissa, exponent) = match body.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (body, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    let mut numerator = if digits.is_empty() {
        0
    } else {
        digits.parse::<u128>().ok()?
    };
    let mut scale = exponent - fraction.len() as i32;
    while numerator != 0 && numerator % 10 == 0 {
        numerator /= 10;
        scale += 1;
    }
    Some((numerator, scale, single))
}

/// The odd part of `numerator * 10^scale` (`10^scale = 2^scale * 5^scale`),
/// or `None` when a factor of 5 in the denominator does not cancel, so the
/// value is not a binary fraction, or the value overflows.
fn odd_part(numerator: u128, scale: i32) -> Option<u128> {
    let mut value = numerator;
    for _ in 0..scale.max(0) {
        value = value.checked_mul(5)?;
    }
    for _ in 0..(-scale).max(0) {
        (value % 5 == 0).then_some(())?;
        value /= 5;
    }
    while value > 0 && value % 2 == 0 {
        value /= 2;
    }
    Some(value)
}

fn finding(comparison: AstNode<'_>, literal: AstNode<'_>, relative: &str) -> Finding {
    let line = comparison.start_position().0 + 1;
    let expression = comparison
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    auditor_core::finding!(
        "float_equality_comparison",
        "correctness",
        "Exact comparison with an inexact float",
        format!(
            "`{expression}` at line {line} tests equality with `{}`, which binary floating point cannot represent exactly",
            literal.text()
        ),
        Severity::Medium,
        Confidence::High,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Compare within a tolerance (`abs(a - b) < epsilon`, `math.isclose`, `approx_eq`), or use an exact type (integers in the smallest unit, a decimal type) when the value must match exactly.",
    )
    .with_identity("comparison", expression.as_str())
    .with_evidence("literal", literal.text())
}

#[cfg(test)]
mod tests {
    use super::exact_in_binary;

    #[test]
    fn exactness_follows_the_binary_representation() {
        for exact in [
            "0.5", "0.0", "1.0", "2.25", "1e3", "0.125f", "1_000.5", "3.0f64", "0.75f32",
        ] {
            assert!(exact_in_binary(exact), "{exact}");
        }
        for inexact in [
            "0.1",
            "0.1F",
            "0.1L",
            "1.1e0",
            "0.3",
            "1.1",
            "2.675",
            "1e-1",
            "0.2f32",
            "0.1d",
            "9007199254740993.0",
            "-1.1",
        ] {
            assert!(!exact_in_binary(inexact), "{inexact}");
        }
    }
}

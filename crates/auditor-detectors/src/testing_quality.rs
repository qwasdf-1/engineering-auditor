//! Test-quality detectors: tests that pass whatever the code under test does.

use auditor_ast::{AstNode, RepoAst};
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::AstScan;
use crate::testing_scope::{self, call_view, compact, AssertingHelpers, CallView, EvidenceScope};
use crate::{util, AnalysisMethod, AuditedFile, Language};

pub(crate) fn test_without_assertion_findings(
    files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut RepoAst,
) -> Vec<Finding> {
    let helpers = AssertingHelpers::collect(AstScan::test_scope(files, method, cache, repo_ast));
    let mut findings = Vec::new();
    testing_scope::visit_test_scope(
        AstScan::test_scope(files, method, cache, repo_ast),
        |file, tree| {
            let relative = util::path_slash(&file.relative);
            for test in testing_scope::test_cases(tree.root_node(), file) {
                if test.skipped
                    || test.expects_failure
                    || testing_scope::exempt_by_name(&file.language, &relative, &test.name)
                {
                    continue;
                }
                let scope = EvidenceScope {
                    language: &file.language,
                    context: &test.context,
                    helpers: helpers.for_language(&file.language),
                };
                if testing_scope::asserts(test.body, &scope) {
                    continue;
                }
                findings.push(
                    auditor_core::finding!(
                        "test_without_assertion",
                        "test_quality",
                        "Test without assertion",
                        format!(
                            "Test `{}` at line {} runs code but checks no result",
                            test.name, test.line
                        ),
                        Severity::Medium,
                        Confidence::Medium,
                        &relative,
                        Some(test.line),
                    )
                    .with_recommendation(
                        "Assert the result or side effect the test exercises; a test that only runs code passes whatever that code returns.",
                    )
                    .with_evidence("test", test.name.as_str())
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

pub(crate) fn tautological_assertion_findings(
    files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut RepoAst,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    testing_scope::visit_test_scope(
        AstScan::test_scope(files, method, cache, repo_ast),
        |file, tree| {
            let relative = util::path_slash(&file.relative);
            let language = &file.language;
            for test in testing_scope::test_cases(tree.root_node(), file) {
                if test.skipped {
                    continue;
                }
                let mut stubs = Vec::new();
                for node in testing_scope::preorder(test.body) {
                    if let Some(stub) = stub(node, language) {
                        stubs.push(stub);
                        continue;
                    }
                    let Some(tautology) = claim(node, language, &test.context)
                        .and_then(|claim| tautology(claim, &stubs))
                    else {
                        continue;
                    };
                    let line = node.start_position().0 + 1;
                    findings.push(
                        auditor_core::finding!(
                            "tautological_assertion",
                            "test_quality",
                            "Assertion that cannot fail",
                            format!("Test `{}` {} at line {line}", test.name, tautology.describe()),
                            Severity::Medium,
                            Confidence::High,
                            &relative,
                            Some(line),
                        )
                        .with_recommendation(
                            "Compare the code under test's result with an independently stated expected value.",
                        )
                        .with_evidence("test", test.name.as_str())
                        .with_evidence("assertion", compact(node.text()))
                        .with_evidence("analysis", "ast"),
                    );
                }
            }
        },
    );
    findings
}

/// What an assertion claims.
enum Claim {
    /// Two operands claimed equal, and whether that equality is fixed by the
    /// language or framework (identity, structural, or reflective) rather
    /// than dispatched to the operand's own `equals`/`__eq__`/`PartialEq`.
    Equal(String, String, bool),
    /// An always-true literal (`assert True`, `assertTrue(true)`).
    Constant(String),
}

enum Tautology {
    SelfComparison(String),
    Literal(String),
    StubbedValue { call: String, value: String },
}

impl Tautology {
    fn describe(&self) -> String {
        match self {
            Self::SelfComparison(operand) => format!("compares `{operand}` with itself"),
            Self::Literal(literal) => format!("asserts the literal `{literal}`"),
            Self::StubbedValue { call, value } => {
                format!("asserts that the mocked `{call}` returns `{value}`, the value it was stubbed with")
            }
        }
    }
}

fn tautology(claim: Claim, stubs: &[(String, String)]) -> Option<Tautology> {
    let (left, right, fixed) = match claim {
        Claim::Constant(literal) => return Some(Tautology::Literal(literal)),
        Claim::Equal(left, right, fixed) => (left, right, fixed),
    };
    let stubbed = stubs.iter().find_map(|(target, value)| {
        let call = [(&left, &right), (&right, &left)]
            .into_iter()
            .find(|(call, other)| stubbed_call(call, target) && *other == value)
            .map(|(call, _)| call.clone())?;
        Some(Tautology::StubbedValue {
            call,
            value: value.clone(),
        })
    });
    if stubbed.is_some() {
        return stubbed;
    }
    // A user-defined equality compared with itself is a reflexivity test
    // (`assertEquals(token, token)` checks `Token.equals`), and NaN is the
    // value unequal to itself.
    let self_comparison = left == right && pure(&left) && (fixed || is_literal(&left));
    self_comparison.then_some(Tautology::SelfComparison(left))
}

/// Whether `side` is the stubbed call: the stubbed expression itself, or a
/// call of the stubbed function (`m.f` stubbed, `m.f(1)` asserted).
fn stubbed_call(side: &str, target: &str) -> bool {
    if side == target {
        return true;
    }
    side.strip_prefix(target)
        .is_some_and(|rest| rest.starts_with('(') && closing_paren(rest) == Some(rest.len() - 1))
}

fn closing_paren(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// An operand without calls, mutation, or fresh allocations: evaluating it
/// twice yields the same value.
fn pure(operand: &str) -> bool {
    !operand.is_empty()
        && !operand.contains('(')
        && !operand.contains("++")
        && !operand.contains("--")
        && !operand.starts_with(['[', '{'])
        && !operand.starts_with("new")
        && !operand.starts_with("await")
        && !operand.to_ascii_lowercase().contains("nan")
}

fn is_literal(operand: &str) -> bool {
    operand.starts_with(['"', '\''])
        || operand.parse::<f64>().is_ok()
        || matches!(
            operand,
            "true" | "false" | "True" | "False" | "null" | "None" | "nil"
        )
}

fn equal(arguments: &[String], left: usize, right: usize, fixed: bool) -> Option<Claim> {
    Some(Claim::Equal(
        arguments.get(left)?.clone(),
        arguments.get(right)?.clone(),
        fixed,
    ))
}

fn constant(argument: Option<&String>, literal: &str) -> Option<Claim> {
    (argument? == literal).then(|| Claim::Constant(literal.to_string()))
}

/// The claim of an assertion node, if it is an equality or truth assertion
/// of a modelled framework.
fn claim(node: AstNode<'_>, language: &Language, context: &[String]) -> Option<Claim> {
    if *language == Language::Python && node.kind() == "assert_statement" {
        return python_assert_claim(node.named_child(0)?);
    }
    let call = call_view(node, language)?;
    match language {
        Language::Rust if call.is_macro => rust_macro_claim(&call),
        Language::Python => python_call_claim(&call),
        Language::JavaScript | Language::TypeScript => js_claim(&call, language, context),
        Language::Go => go_claim(&call),
        Language::Java => java_claim(&call),
        _ => None,
    }
}

fn rust_macro_claim(call: &CallView<'_>) -> Option<Claim> {
    match call.name() {
        "assert_eq" | "debug_assert_eq" => equal(&call.arguments, 0, 1, false),
        "assert" | "debug_assert" => rust_assert_claim(call.arguments.first()?),
        _ => None,
    }
}

fn python_call_claim(call: &CallView<'_>) -> Option<Claim> {
    let arguments = &call.arguments;
    match call.name() {
        "assertIs" => equal(arguments, 0, 1, true),
        "assertEqual"
        | "assertEquals"
        | "assertListEqual"
        | "assertDictEqual"
        | "assertTupleEqual"
        | "assertSetEqual"
        | "assertSequenceEqual"
        | "assertCountEqual"
        | "assertMultiLineEqual" => equal(arguments, 0, 1, false),
        "assertTrue" => constant(arguments.first(), "True"),
        "assertFalse" => constant(arguments.first(), "False"),
        _ => None,
    }
}

fn python_assert_claim(test: AstNode<'_>) -> Option<Claim> {
    match test.kind() {
        "true" => Some(Claim::Constant("True".to_string())),
        // A non-empty string or tuple is always truthy: `assert (x, "msg")`.
        "string" if test.text().len() > 2 => Some(Claim::Constant(test.text().to_string())),
        "tuple" if test.named_child_count() > 0 => Some(Claim::Constant(compact(test.text()))),
        "comparison_operator" if test.named_child_count() == 2 => {
            let operator = test.child(1)?.kind();
            matches!(operator, "==" | "is").then(|| {
                Claim::Equal(
                    compact(test.named_child(0).map_or("", |node| node.text())),
                    compact(test.named_child(1).map_or("", |node| node.text())),
                    operator == "is",
                )
            })
        }
        _ => None,
    }
}

fn rust_assert_claim(argument: &str) -> Option<Claim> {
    if argument == "true" {
        return Some(Claim::Constant("true".to_string()));
    }
    let (left, right) = argument.split_once("==")?;
    let comparison = !right.contains("==")
        && !left.ends_with(['!', '<', '>', '='])
        && !right.contains(['&', '|']);
    comparison.then(|| Claim::Equal(left.to_string(), right.to_string(), false))
}

fn js_claim(call: &CallView<'_>, language: &Language, context: &[String]) -> Option<Claim> {
    let name = call.name();
    if matches!(
        name,
        "toBe" | "toEqual" | "toStrictEqual" | "toBeTruthy" | "toBeFalsy"
    ) {
        return fluent_claim(
            call,
            language,
            &["expect"],
            ("toBeTruthy", "toBeFalsy"),
            true,
        );
    }
    let base = *call.segments.first()?;
    if call.segments.len() == 1 {
        return (base == "assert")
            .then(|| constant(call.arguments.first(), "true"))
            .flatten();
    }
    let asserting_object = base == "assert" || context.iter().any(|context| context == base);
    (asserting_object && call.segments.len() == 2)
        .then(|| assert_object_claim(name, &call.arguments))
        .flatten()
}

/// A fluent assertion on a subject call, `expect(actual).toBe(expected)` or
/// `assertThat(actual).isEqualTo(expected)`, with its `(truthy, falsy)`
/// matchers.
fn fluent_claim(
    call: &CallView<'_>,
    language: &Language,
    subjects: &[&str],
    (truthy, falsy): (&str, &str),
    fixed: bool,
) -> Option<Claim> {
    let subject = call_view(call.receiver?, language)?;
    if !matches!(subject.segments.as_slice(), [only] if subjects.contains(only)) {
        return None;
    }
    let actual = subject.arguments.first()?;
    let name = call.name();
    if name == truthy {
        constant(Some(actual), "true")
    } else if name == falsy {
        constant(Some(actual), "false")
    } else {
        Some(Claim::Equal(
            actual.clone(),
            call.arguments.first()?.clone(),
            fixed,
        ))
    }
}

/// Node and Chai `assert.*`, and AVA or tap `t.*`.
fn assert_object_claim(name: &str, arguments: &[String]) -> Option<Claim> {
    match name {
        "equal" | "strictEqual" | "deepEqual" | "deepStrictEqual" | "is" | "same"
        | "deepEquals" | "equals" => equal(arguments, 0, 1, true),
        "ok" | "isTrue" | "true" | "truthy" => constant(arguments.first(), "true"),
        "isFalse" | "false" | "falsy" | "notOk" => constant(arguments.first(), "false"),
        _ => None,
    }
}

fn go_claim(call: &CallView<'_>) -> Option<Claim> {
    let [package, name] = call.segments.as_slice() else {
        return None;
    };
    if !matches!(*package, "assert" | "require") {
        return None;
    }
    match *name {
        "Equal" | "EqualValues" | "Exactly" | "Same" => equal(&call.arguments, 1, 2, true),
        "True" => constant(call.arguments.get(1), "true"),
        "False" => constant(call.arguments.get(1), "false"),
        _ => None,
    }
}

fn java_claim(call: &CallView<'_>) -> Option<Claim> {
    let arguments = &call.arguments;
    // JUnit 4 puts the message first, JUnit 5 last.
    let message_first = arguments.len() >= 3 && arguments[0].starts_with('"');
    let only_literal = |literal: &str| {
        arguments.iter().any(|argument| argument == literal)
            && arguments
                .iter()
                .all(|argument| argument == literal || argument.starts_with('"'))
    };
    match call.name() {
        "assertEquals" | "assertSame" | "assertArrayEquals" | "assertIterableEquals" => {
            let offset = usize::from(message_first);
            equal(arguments, offset, offset + 1, call.name() == "assertSame")
        }
        "assertTrue" => only_literal("true").then(|| Claim::Constant("true".to_string())),
        "assertFalse" => only_literal("false").then(|| Claim::Constant("false".to_string())),
        "isEqualTo" | "isSameAs" | "isTrue" | "isFalse" => fluent_claim(
            call,
            &Language::Java,
            &["assertThat", "then"],
            ("isTrue", "isFalse"),
            call.name() == "isSameAs",
        ),
        "assertThat" if arguments.len() >= 2 => {
            let expected = hamcrest_expected(arguments.last()?)?;
            Some(Claim::Equal(
                arguments[arguments.len() - 2].clone(),
                expected.to_string(),
                false,
            ))
        }
        _ => None,
    }
}

/// The value of an equality matcher: `is(x)`, `equalTo(x)`, `is(equalTo(x))`.
fn hamcrest_expected(matcher: &str) -> Option<&str> {
    let mut inner = matcher;
    let mut stripped = false;
    loop {
        let unqualified = inner
            .strip_prefix("Matchers.")
            .or_else(|| inner.strip_prefix("CoreMatchers."))
            .unwrap_or(inner);
        let Some(rest) = ["is(", "equalTo(", "sameInstance("]
            .iter()
            .find_map(|prefix| unqualified.strip_prefix(prefix))
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            break;
        };
        inner = rest;
        stripped = true;
    }
    stripped.then_some(inner)
}

/// A mock stub in the test body: the stubbed call target and the value it
/// returns.
fn stub(node: AstNode<'_>, language: &Language) -> Option<(String, String)> {
    if *language == Language::Python {
        if node.kind() != "assignment" {
            return None;
        }
        let target = compact(node.child_by_field_name("left")?.text());
        let target = target.strip_suffix(".return_value")?.to_string();
        return Some((target, compact(node.child_by_field_name("right")?.text())));
    }
    let call = call_view(node, language)?;
    let [value] = call.arguments.as_slice() else {
        return None;
    };
    let value = value.clone();
    let receiver = call.receiver?;
    if matches!(language, Language::JavaScript | Language::TypeScript) {
        return matches!(call.name(), "mockReturnValue" | "mockReturnValueOnce")
            .then(|| js_mock_target(receiver, language))
            .flatten()
            .map(|target| (target, value));
    }
    let stubbed = call_view(receiver, language)?;
    let target = match (language, call.name()) {
        (Language::Java, "thenReturn" | "willReturn")
            if matches!(stubbed.name(), "when" | "given") =>
        {
            stubbed.arguments.first()?.clone()
        }
        (Language::Go, "Return") if stubbed.name() == "On" => format!(
            "{}.{}",
            compact(stubbed.receiver?.text()),
            stubbed.arguments.first()?.trim_matches('"')
        ),
        (Language::Rust, "return_const") => format!(
            "{}.{}",
            compact(stubbed.receiver?.text()),
            stubbed.name().strip_prefix("expect_")?
        ),
        _ => return None,
    };
    Some((target, value))
}

/// The function a Jest/Vitest mock stands for: the variable a `jest.fn()`
/// is bound to, or the mocked function itself. A `spyOn` of a real object's
/// method is not one: asserting through the object checks that the spy
/// intercepts its dispatch.
fn js_mock_target(receiver: AstNode<'_>, language: &Language) -> Option<String> {
    let Some(mock) = call_view(receiver, language) else {
        return Some(compact(receiver.text()));
    };
    match mock.segments.as_slice() {
        ["jest" | "vi", "fn"] => {
            let mut node = mock.node;
            while let Some(parent) = node.parent() {
                if parent.kind() == "variable_declarator" {
                    return Some(parent.child_by_field_name("name")?.text().to_string());
                }
                node = parent;
            }
            None
        }
        _ => None,
    }
}

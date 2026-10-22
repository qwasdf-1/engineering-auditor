//! C and C++ tests and their skip marks. The frameworks declare tests with
//! macros, which tree-sitter reads as ordinary syntax:
//! - GoogleTest `TEST(Suite, Name) { .. }` (and `TEST_F`, `TEST_P`,
//!   `TYPED_TEST`, `TYPED_TEST_P`), CppUTest and Unity fixture
//!   `TEST(Group, Name)`/`IGNORE_TEST(Group, Name)`: a function definition
//!   whose declarator is the macro.
//! - Catch2 and doctest `TEST_CASE("name", "[tags]") { .. }` and their
//!   variants: a call statement followed by the body block.
//! - Unity `void test_name(void) { .. }`: a plain `test*` function.
//!
//! A test is skipped by a `DISABLED_` suite or test name (GoogleTest), by
//! `IGNORE_TEST`, by a hidden Catch2 tag (`[.]`, `[!hide]`), by a doctest
//! `* doctest::skip()` decorator, or by a skip call in its body:
//! `GTEST_SKIP()`, Catch2 `SKIP()`, Unity `TEST_IGNORE()`.

use auditor_ast::AstNode;

use super::skips::{conditional, row, states_reason, SkipMark, SkippedGroup};
use super::{preorder, TestCase};
use crate::ast_scan::nodes_of_kind;

/// GoogleTest and CppUTest test macros, and the CppUTest/Unity fixture
/// macro that declares an ignored test.
const DECLARATOR_MACROS: &[&str] = &[
    "TEST",
    "TEST_F",
    "TEST_P",
    "TYPED_TEST",
    "TYPED_TEST_P",
    "IGNORE_TEST",
];

/// Catch2 and doctest test macros, without a `CATCH_`/`DOCTEST_` prefix.
const CASE_MACROS: &[&str] = &[
    "TEST_CASE",
    "TEST_CASE_METHOD",
    "TEST_CASE_FIXTURE",
    "TEST_CASE_TEMPLATE",
    "SCENARIO",
    "SCENARIO_METHOD",
    "TEMPLATE_TEST_CASE",
    "TEMPLATE_TEST_CASE_SIG",
    "TEMPLATE_PRODUCT_TEST_CASE",
    "TEMPLATE_LIST_TEST_CASE",
];

pub(super) fn c_family_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    let mut tests = declarator_tests(root);
    tests.extend(case_tests(root));
    tests.extend(unity_tests(root));
    tests
}

fn macro_name(text: &str) -> &str {
    text.strip_prefix("CATCH_")
        .or_else(|| text.strip_prefix("DOCTEST_"))
        .unwrap_or(text)
}

fn test_case<'t>(
    name: String,
    line_node: AstNode<'t>,
    body: AstNode<'t>,
    skip: Option<SkipMark<'t>>,
) -> TestCase<'t> {
    TestCase {
        name,
        line: line_node.start_position().0 + 1,
        body,
        context: Vec::new(),
        skipped: skip.is_some(),
        skip,
        expects_failure: false,
    }
}

/// `TEST(Suite, Name) { .. }`, named `Suite.Name` as GoogleTest reports it.
/// C++ reads it as a function definition; C, without implicit `int`, as a
/// call statement before a block.
fn declarator_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    let definitions = nodes_of_kind(root, &["function_definition"])
        .into_iter()
        .filter_map(|function| {
            let declarator = function
                .child_by_field_name("declarator")
                .filter(|declarator| declarator.kind() == "function_declarator")?;
            let callee = declarator
                .child_by_field_name("declarator")
                .filter(|callee| callee.kind() == "identifier")?;
            let parts = declarator
                .child_by_field_name("parameters")?
                .named_children()
                .filter(|part| part.kind() == "parameter_declaration")
                .collect::<Vec<_>>();
            Some((
                callee,
                parts,
                function,
                function.child_by_field_name("body")?,
            ))
        });
    let statements = nodes_of_kind(root, &["expression_statement"])
        .into_iter()
        .filter_map(|statement| {
            let (callee, arguments, body) = case_macro(statement)?;
            let parts = arguments
                .named_children()
                .filter(|part| part.kind() == "identifier")
                .collect::<Vec<_>>();
            Some((callee, parts, statement, body))
        });
    definitions
        .chain(statements)
        .filter_map(|(callee, parts, declaration, body)| {
            if !DECLARATOR_MACROS.contains(&callee.text()) {
                return None;
            }
            let [suite, name] = parts.as_slice() else {
                return None;
            };
            let skip = if callee.text() == "IGNORE_TEST" {
                Some(unstated(callee, declaration))
            } else {
                parts
                    .iter()
                    .copied()
                    .find(|part| part.text().starts_with("DISABLED_"))
                    .map(|part| unstated(part, declaration))
                    .or_else(|| body_skip(body))
            };
            Some(test_case(
                format!("{}.{}", suite.text(), name.text()),
                declaration,
                body,
                skip,
            ))
        })
        .collect()
}

fn same(a: AstNode<'_>, b: AstNode<'_>) -> bool {
    (a.start_byte(), a.end_byte(), a.kind()) == (b.start_byte(), b.end_byte(), b.kind())
}

/// A mark with no reason argument: `DISABLED_`, `IGNORE_TEST`, `[.]`.
fn unstated<'t>(node: AstNode<'t>, declaration: AstNode<'t>) -> SkipMark<'t> {
    SkipMark {
        node,
        block_row: row(declaration),
        stated: false,
    }
}

/// A macro statement before a block (`TEST_CASE("..") { .. }`): the macro,
/// its arguments, and the block.
fn case_macro<'t>(statement: AstNode<'t>) -> Option<(AstNode<'t>, AstNode<'t>, AstNode<'t>)> {
    let call = statement
        .named_child(0)
        .filter(|call| call.kind() == "call_expression")?;
    let callee = call
        .child_by_field_name("function")
        .filter(|callee| callee.kind() == "identifier")?;
    let body = statement
        .parent()?
        .named_children()
        .find(|sibling| sibling.start_byte() >= statement.end_byte())
        .filter(|body| body.kind() == "compound_statement")?;
    Some((callee, call.child_by_field_name("arguments")?, body))
}

/// `TEST_CASE("name", "[tags]") { .. }` and its Catch2 and doctest variants.
fn case_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["expression_statement"])
        .into_iter()
        .filter_map(|statement| {
            let (callee, arguments, body) = case_macro(statement)?;
            if !CASE_MACROS.contains(&macro_name(callee.text())) {
                return None;
            }
            let strings: Vec<AstNode<'_>> = arguments
                .named_children()
                .filter_map(|argument| first_string(argument))
                .collect();
            let title = strings.first()?;
            let skip = strings
                .get(1)
                .filter(|tags| tags.parent().is_some_and(|parent| same(parent, arguments)))
                .and_then(|tags| hidden_tags(*tags, statement))
                .or_else(|| doctest_skip(arguments, statement))
                .or_else(|| body_skip(body));
            Some(test_case(string_content(*title), statement, body, skip))
        })
        .collect()
}

/// Unity tests: `test*` functions, skipped by `TEST_IGNORE()` in their body.
fn unity_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["function_definition"])
        .into_iter()
        .filter_map(|function| {
            let name = function
                .child_by_field_name("declarator")
                .filter(|declarator| declarator.kind() == "function_declarator")?
                .child_by_field_name("declarator")
                .filter(|name| name.kind() == "identifier" && name.text().starts_with("test"))?;
            let body = function.child_by_field_name("body")?;
            Some(test_case(
                name.text().to_string(),
                function,
                body,
                body_skip(body),
            ))
        })
        .collect()
}

/// The string literal an argument is or begins with: the title of
/// `"name" * doctest::skip()`.
fn first_string(argument: AstNode<'_>) -> Option<AstNode<'_>> {
    match argument.kind() {
        "string_literal" | "raw_string_literal" => Some(argument),
        "binary_expression" => first_string(argument.child_by_field_name("left")?),
        _ => None,
    }
}

fn string_content(literal: AstNode<'_>) -> String {
    literal
        .named_children()
        .filter(|part| part.kind() == "string_content")
        .map(|part| part.text())
        .collect()
}

/// A Catch2 tag list that hides the test: `[.]`, `[.slow]`, `[!hide]`,
/// `[hide]`. `[.slow]` is shorthand for `[.][slow]`, so any tag beyond the
/// hide marker (not a `[!flag]`) names why the test is hidden.
fn hidden_tags<'t>(tags: AstNode<'t>, statement: AstNode<'t>) -> Option<SkipMark<'t>> {
    let text = string_content(tags);
    let names: Vec<&str> = text
        .split(['[', ']'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let hidden = names
        .iter()
        .any(|name| name.starts_with('.') || matches!(*name, "!hide" | "hide"));
    hidden.then(|| SkipMark {
        stated: names.iter().any(|name| {
            let name = name.trim_start_matches('.');
            !name.is_empty() && !name.starts_with('!') && name != "hide"
        }),
        ..unstated(tags, statement)
    })
}

/// A doctest `* doctest::skip()` decorator. `skip(false)` runs the test; a
/// computed condition, or a `doctest::description(..)`, states the reason.
fn doctest_skip<'t>(arguments: AstNode<'t>, statement: AstNode<'t>) -> Option<SkipMark<'t>> {
    let decorators: Vec<(&str, AstNode<'t>)> = preorder(arguments)
        .into_iter()
        .filter(|node| node.kind() == "call_expression")
        .filter_map(|call| {
            let callee = call.child_by_field_name("function")?.text();
            let name = callee.strip_prefix("doctest::").unwrap_or(callee);
            Some((name, call))
        })
        .collect();
    let (_, skip) = decorators
        .iter()
        .copied()
        .find(|(name, _)| *name == "skip")?;
    let condition = skip
        .child_by_field_name("arguments")
        .and_then(|arguments| arguments.named_child(0));
    if condition.is_some_and(|condition| condition.text() == "false") {
        return None;
    }
    let described = decorators.iter().any(|(name, _)| *name == "description");
    Some(SkipMark {
        stated: described || condition.is_some_and(|condition| condition.text() != "true"),
        ..unstated(skip, statement)
    })
}

/// The first skip call in a test body: GoogleTest `GTEST_SKIP()`, Catch2
/// `SKIP(..)`, Unity `TEST_IGNORE()`/`TEST_IGNORE_MESSAGE(..)`. A message
/// (streamed into `GTEST_SKIP() << ".."`, or an argument), or a condition
/// it runs under, states why.
fn body_skip(body: AstNode<'_>) -> Option<SkipMark<'_>> {
    preorder(body).into_iter().find_map(|node| {
        if node.kind() != "call_expression" {
            return None;
        }
        let callee = node.child_by_field_name("function")?.text();
        if !matches!(
            macro_name(callee),
            "GTEST_SKIP" | "SKIP" | "TEST_IGNORE" | "TEST_IGNORE_MESSAGE"
        ) {
            return None;
        }
        let arguments = node
            .child_by_field_name("arguments")
            .map(|arguments| arguments.text())
            .unwrap_or_default();
        let streamed = node.parent().is_some_and(|parent| {
            parent.kind() == "binary_expression"
                && parent
                    .child_by_field_name("left")
                    .is_some_and(|left| same(left, node))
                && parent
                    .child_by_field_name("right")
                    .is_some_and(|message| states_reason(message.text()))
        });
        Some(SkipMark {
            node,
            block_row: row(node),
            stated: states_reason(arguments) || streamed || conditional(node, body),
        })
    })
}

/// doctest `TEST_SUITE("name" * doctest::skip()) { .. }`, which skips every
/// test in the suite.
pub(super) fn c_family_groups(root: AstNode<'_>) -> Vec<SkippedGroup<'_>> {
    nodes_of_kind(root, &["expression_statement"])
        .into_iter()
        .filter_map(|statement| {
            let (callee, arguments, _) = case_macro(statement)?;
            if macro_name(callee.text()) != "TEST_SUITE" {
                return None;
            }
            let title = arguments.named_children().find_map(first_string)?;
            Some(SkippedGroup {
                name: string_content(title),
                mark: doctest_skip(arguments, statement)?,
            })
        })
        .collect()
}

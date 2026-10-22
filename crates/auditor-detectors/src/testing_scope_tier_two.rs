//! Kotlin and C# test declarations: JUnit and kotlin.test annotations, and
//! xUnit, NUnit, and MSTest attributes. Only the skip rule reads them so
//! far.

use auditor_ast::AstNode;

use super::frameworks::{declared_test, marks};
use super::skips::{csharp_skip_mark, kotlin_skip_mark};
use super::TestCase;
use crate::ast_scan::nodes_of_kind;

/// A Kotlin declaration's annotations (`@Test`, `@Disabled("..")`).
pub(super) fn kotlin_annotations(declaration: AstNode<'_>) -> Vec<AstNode<'_>> {
    marks(declaration, "modifiers", &["annotation"])
}

/// A Kotlin annotation's simple name: `Disabled` of `@org.junit.Disabled("..")`.
pub(super) fn kotlin_annotation_name(annotation: AstNode<'_>) -> &str {
    annotation
        .first_descendant_of_kind(&auditor_ast::AstNodeKind::from("user_type"))
        .and_then(|user_type| {
            user_type
                .named_children()
                .filter(|part| part.kind() == "type_identifier")
                .last()
        })
        .map(|name| name.text())
        .unwrap_or_default()
}

/// JUnit 4 and 5 and kotlin.test functions: `@Test`, `@ParameterizedTest`,
/// `@RepeatedTest`.
pub(super) fn kotlin_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["function_declaration"])
        .into_iter()
        .filter_map(|function| {
            let annotations = kotlin_annotations(function);
            let test = annotations.iter().copied().find(|annotation| {
                matches!(
                    kotlin_annotation_name(*annotation),
                    "Test" | "ParameterizedTest" | "RepeatedTest"
                )
            })?;
            let name = function
                .named_children()
                .find(|child| child.kind() == "simple_identifier")?;
            let body = function
                .named_children()
                .find(|child| child.kind() == "function_body")?;
            Some(declared_test(
                name,
                body,
                kotlin_skip_mark(function, &annotations),
                super::compact(test.text()).contains("expected="),
            ))
        })
        .collect()
}

/// A C# declaration's attributes, without an `Attribute` suffix.
pub(super) fn csharp_attributes(declaration: AstNode<'_>) -> Vec<AstNode<'_>> {
    marks(declaration, "attribute_list", &["attribute"])
}

pub(super) fn csharp_attribute_name(attribute: AstNode<'_>) -> &str {
    let name = attribute
        .child_by_field_name("name")
        .or_else(|| attribute.named_child(0))
        .map(|name| name.text())
        .unwrap_or_default();
    let name = name.rsplit('.').next().unwrap_or(name);
    name.strip_suffix("Attribute").unwrap_or(name)
}

/// xUnit `[Fact]`/`[Theory]`, NUnit `[Test]`/`[TestCase]`/`[TestCaseSource]`,
/// and MSTest `[TestMethod]`/`[DataTestMethod]` methods.
pub(super) fn csharp_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["method_declaration"])
        .into_iter()
        .filter_map(|method| {
            let attributes = csharp_attributes(method);
            let names: Vec<&str> = attributes
                .iter()
                .map(|attribute| csharp_attribute_name(*attribute))
                .collect();
            let test = names.iter().any(|name| {
                matches!(
                    *name,
                    "Fact"
                        | "Theory"
                        | "Test"
                        | "TestCase"
                        | "TestCaseSource"
                        | "TestMethod"
                        | "DataTestMethod"
                )
            });
            if !test {
                return None;
            }
            let name = method.child_by_field_name("name")?;
            let body = method.child_by_field_name("body").or_else(|| {
                method
                    .named_children()
                    .find(|child| child.kind() == "arrow_expression_clause")
            })?;
            Some(declared_test(
                name,
                body,
                csharp_skip_mark(method, &attributes),
                names.contains(&"ExpectedException"),
            ))
        })
        .collect()
}

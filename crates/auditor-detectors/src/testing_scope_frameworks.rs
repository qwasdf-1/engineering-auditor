//! The test functions each language's frameworks declare, and the
//! function-like definitions a test may delegate to.

use auditor_ast::AstNode;

use super::skips::{
    csharp_skip_mark, go_skip_mark, java_skip_mark, js_skip_mark, kotlin_skip_mark,
    python_skip_mark, rust_skip_mark,
};
use super::{call_view, preorder, TestCase};
use crate::ast_scan::{self, nodes_of_kind};
use crate::{AuditedFile, Language, SourceRole};

/// The tests in one file. Python and JavaScript/TypeScript tests count only
/// in test files, where their frameworks collect them; the attribute and
/// annotation forms count anywhere.
pub(crate) fn test_cases<'t>(root: AstNode<'t>, file: &AuditedFile) -> Vec<TestCase<'t>> {
    let (language, role) = (&file.language, file.role);
    match language {
        Language::Rust => rust_tests(root),
        Language::Java => java_tests(root),
        Language::Kotlin => kotlin_tests(root),
        Language::CSharp => csharp_tests(root),
        Language::Go if role == SourceRole::Test => go_tests(root),
        Language::Python if role == SourceRole::Test && pytest_file(file) => python_tests(root),
        Language::JavaScript | Language::TypeScript if role == SourceRole::Test => {
            js_tests(root, language)
        }
        _ => Vec::new(),
    }
}

/// pytest collects `test_*.py` and `*_test.py`; `unittest` discovery
/// `test*.py`. Other Python files under a test tree are fixtures or data.
fn pytest_file(file: &AuditedFile) -> bool {
    let stem = file
        .relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    stem.starts_with("test") || stem.ends_with("_test")
}

fn line_of(node: AstNode<'_>) -> usize {
    node.start_position().0 + 1
}

/// `#[tokio::test(flavor = "..")]` is `tokio::test`.
fn attribute_path(attribute: &str) -> &str {
    attribute
        .trim_start_matches("#[")
        .trim_end_matches(']')
        .split(['(', '='])
        .next()
        .unwrap_or_default()
        .trim()
}

fn rust_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["function_item"])
        .into_iter()
        .filter_map(|function| {
            let attribute_nodes = ast_scan::preceding_attributes(function).collect::<Vec<_>>();
            let attributes = attribute_nodes
                .iter()
                .map(|attribute| attribute_path(attribute.text()).to_string())
                .collect::<Vec<_>>();
            let is_test = attributes.iter().any(|path| {
                path == "test"
                    || path.ends_with("::test")
                    || path == "rstest"
                    || path == "test_case"
            });
            // A test returning a value the harness checks (quickcheck's
            // `bool`, a `TestResult`) asserts through that value.
            let returns_value = function
                .child_by_field_name("return_type")
                .is_some_and(|returned| !returned.text().contains("Result"));
            if !is_test || returns_value {
                return None;
            }
            let name = function.child_by_field_name("name")?;
            Some(TestCase {
                name: name.text().to_string(),
                line: line_of(name),
                body: function.child_by_field_name("body")?,
                context: Vec::new(),
                skipped: attributes.iter().any(|path| path == "ignore"),
                skip: rust_skip_mark(&attribute_nodes),
                expects_failure: attributes.iter().any(|path| path == "should_panic"),
            })
        })
        .collect()
}

fn python_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["function_definition"])
        .into_iter()
        .filter_map(|function| {
            let name = function.child_by_field_name("name")?;
            if !name.text().starts_with("test") {
                return None;
            }
            let decorated = function
                .parent()
                .filter(|parent| parent.kind() == "decorated_definition");
            let holder = decorated.unwrap_or(function).parent()?;
            let collected = match holder.kind() {
                "module" => true,
                "block" => holder.parent().is_some_and(|class| {
                    class.kind() == "class_definition" && python_test_class(class)
                }),
                _ => false,
            };
            let parameters = function
                .child_by_field_name("parameters")
                .map(|parameters| parameters.text())
                .unwrap_or_default();
            // pytest-benchmark tests time the code rather than check it.
            if !collected || parameters.contains("benchmark") {
                return None;
            }
            let skipped = decorated.is_some_and(|decorated| {
                decorated
                    .named_children()
                    .filter(|child| child.kind() == "decorator")
                    .any(|decorator| {
                        decorator.text().contains("skip") || decorator.text().contains("xfail")
                    })
            });
            Some(TestCase {
                name: name.text().to_string(),
                line: line_of(name),
                body: function.child_by_field_name("body")?,
                context: Vec::new(),
                skipped,
                skip: decorated.and_then(python_skip_mark),
                expects_failure: false,
            })
        })
        .collect()
}

fn python_test_class(class: AstNode<'_>) -> bool {
    class
        .child_by_field_name("name")
        .is_some_and(|name| name.text().starts_with("Test"))
        || class
            .child_by_field_name("superclasses")
            .is_some_and(|bases| bases.text().contains("TestCase"))
}

const JS_TEST_FUNCTIONS: &[&str] = &["it", "test", "specify", "xit", "xtest", "xspecify"];
const JS_TEST_MODIFIERS: &[&str] = &["only", "skip", "todo", "concurrent", "failing"];

fn js_tests<'t>(root: AstNode<'t>, language: &Language) -> Vec<TestCase<'t>> {
    nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter_map(|call| {
            let view = call_view(call, language)?;
            let (base, modifiers) = view.segments.split_first()?;
            if !JS_TEST_FUNCTIONS.contains(base)
                || !modifiers
                    .iter()
                    .all(|modifier| JS_TEST_MODIFIERS.contains(modifier))
            {
                return None;
            }
            let arguments = call
                .child_by_field_name("arguments")?
                .named_children()
                .filter(|argument| !argument.kind().contains("comment"))
                .collect::<Vec<_>>();
            let title = arguments.first()?;
            if !matches!(title.kind(), "string" | "template_string") {
                return None;
            }
            let function = arguments.iter().rev().find(|argument| {
                matches!(
                    argument.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
            })?;
            Some(TestCase {
                name: title.text().trim_matches(['\'', '"', '`']).to_string(),
                line: line_of(call),
                body: function.child_by_field_name("body")?,
                context: js_parameters(*function),
                skipped: base.starts_with('x')
                    || modifiers
                        .iter()
                        .any(|modifier| matches!(*modifier, "skip" | "todo")),
                skip: (base.starts_with('x') || modifiers.contains(&"skip"))
                    .then(|| js_skip_mark(call))
                    .flatten(),
                expects_failure: modifiers.contains(&"failing"),
            })
        })
        .collect()
}

fn js_parameters(function: AstNode<'_>) -> Vec<String> {
    if let Some(parameter) = function.child_by_field_name("parameter") {
        return vec![parameter.text().to_string()];
    }
    function
        .child_by_field_name("parameters")
        .map(|parameters| {
            parameters
                .named_children()
                .filter_map(|parameter| match parameter.kind() {
                    "identifier" => Some(parameter),
                    _ => parameter
                        .child_by_field_name("pattern")
                        .filter(|pattern| pattern.kind() == "identifier"),
                })
                .map(|parameter| parameter.text().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Parameters of a Go function typed as a test context (`*testing.T`,
/// `testing.TB`).
fn go_test_parameters(function: AstNode<'_>) -> Vec<String> {
    function
        .child_by_field_name("parameters")
        .map(|parameters| {
            parameters
                .named_children()
                .filter(|parameter| {
                    parameter
                        .child_by_field_name("type")
                        .is_some_and(|kind| matches!(kind.text(), "*testing.T" | "testing.TB"))
                })
                .filter_map(|parameter| parameter.child_by_field_name("name"))
                .map(|name| name.text().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn go_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["function_declaration"])
        .into_iter()
        .filter_map(|function| {
            let name = function.child_by_field_name("name")?;
            let suffix = name.text().strip_prefix("Test")?;
            let parameters = function.child_by_field_name("parameters")?;
            let single_t = parameters.named_child_count() == 1
                && parameters
                    .named_child(0)
                    .and_then(|parameter| parameter.child_by_field_name("type"))
                    .is_some_and(|kind| kind.text() == "*testing.T");
            if suffix.starts_with(|c: char| c.is_ascii_lowercase()) || !single_t {
                return None;
            }
            let body = function.child_by_field_name("body")?;
            let mut context = go_test_parameters(function);
            for literal in nodes_of_kind(body, &["func_literal"]) {
                context.extend(go_test_parameters(literal));
            }
            let skipped = preorder(body)
                .into_iter()
                .filter_map(|node| call_view(node, &Language::Go))
                .any(|call| {
                    call.segments.len() == 2
                        && context.iter().any(|context| context == call.segments[0])
                        && call.name().starts_with("Skip")
                });
            let skip = go_skip_mark(body, &context);
            Some(TestCase {
                name: name.text().to_string(),
                line: line_of(name),
                body,
                context,
                skipped,
                skip,
                expects_failure: false,
            })
        })
        .collect()
}

pub(super) fn java_annotations(method: AstNode<'_>) -> Vec<AstNode<'_>> {
    marks(method, "modifiers", &["annotation", "marker_annotation"])
}

/// The `kinds` children of a declaration's `container` children: the
/// annotations in its modifiers, the attributes in its attribute lists.
fn marks<'t>(declaration: AstNode<'t>, container: &str, kinds: &[&str]) -> Vec<AstNode<'t>> {
    declaration
        .named_children()
        .filter(|child| child.kind() == container)
        .flat_map(|container| container.named_children())
        .filter(|child| kinds.contains(&child.kind()))
        .collect()
}

fn annotation_name(annotation: AstNode<'_>) -> &str {
    annotation
        .child_by_field_name("name")
        .map(|name| name.text().rsplit('.').next().unwrap_or_default())
        .unwrap_or_default()
}

fn java_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
    nodes_of_kind(root, &["method_declaration"])
        .into_iter()
        .filter_map(|method| {
            let annotations = java_annotations(method);
            let test_annotation = annotations.iter().find(|annotation| {
                matches!(
                    annotation_name(**annotation),
                    "Test" | "ParameterizedTest" | "RepeatedTest"
                )
            });
            let name = method.child_by_field_name("name")?;
            if test_annotation.is_none() && !junit3_test(method, name.text()) {
                return None;
            }
            let test_arguments = test_annotation
                .map(|annotation| super::compact(annotation.text()))
                .unwrap_or_default();
            Some(TestCase {
                name: name.text().to_string(),
                line: line_of(name),
                body: method.child_by_field_name("body")?,
                context: Vec::new(),
                skipped: test_arguments.contains("enabled=false")
                    || annotations.iter().any(|annotation| {
                        matches!(annotation_name(*annotation), "Disabled" | "Ignore")
                    }),
                skip: java_skip_mark(method, &annotations),
                expects_failure: test_arguments.contains("expected="),
            })
        })
        .collect()
}

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
fn kotlin_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
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
fn csharp_tests(root: AstNode<'_>) -> Vec<TestCase<'_>> {
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

/// A test declared by an annotation or attribute: no per-test context, and
/// skipped exactly when a skip mark disables it. A Kotlin backtick name is
/// read without its backticks.
fn declared_test<'t>(
    name: AstNode<'t>,
    body: AstNode<'t>,
    skip: Option<super::SkipMark<'t>>,
    expects_failure: bool,
) -> TestCase<'t> {
    TestCase {
        name: name.text().trim_matches('`').to_string(),
        line: line_of(name),
        body,
        context: Vec::new(),
        skipped: skip.is_some(),
        skip,
        expects_failure,
    }
}

/// A JUnit 3 test: a public, parameterless `test*` method of a `TestCase`.
fn junit3_test(method: AstNode<'_>, name: &str) -> bool {
    let public = method
        .named_children()
        .any(|child| child.kind() == "modifiers" && child.text().contains("public"));
    let parameterless = method
        .child_by_field_name("parameters")
        .is_some_and(|parameters| parameters.named_child_count() == 0);
    let in_test_case = method
        .parent()
        .and_then(|body| body.parent())
        .and_then(|class| class.child_by_field_name("superclass"))
        .is_some_and(|superclass| superclass.text().contains("TestCase"));
    name.starts_with("test") && public && parameterless && in_test_case
}

/// Every named function definition: its name, body, and test-context
/// parameters.
pub(crate) fn functions<'t>(
    root: AstNode<'t>,
    language: &Language,
) -> Vec<(&'t str, AstNode<'t>, Vec<String>)> {
    let kinds: &[&str] = match language {
        Language::Rust => &["function_item"],
        Language::Python => &["function_definition"],
        Language::JavaScript | Language::TypeScript => &[
            "function_declaration",
            "method_definition",
            "variable_declarator",
        ],
        Language::Go => &["function_declaration", "method_declaration"],
        Language::Java => &["method_declaration"],
        _ => return Vec::new(),
    };
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter_map(|function| {
            let definition = if function.kind() == "variable_declarator" {
                function.child_by_field_name("value").filter(|value| {
                    matches!(value.kind(), "arrow_function" | "function_expression")
                })?
            } else {
                function
            };
            let name = function.child_by_field_name("name")?.text();
            let context = match language {
                Language::Go => go_test_parameters(definition),
                Language::JavaScript | Language::TypeScript => js_parameters(definition)
                    .into_iter()
                    .filter(|parameter| parameter == "t")
                    .collect(),
                _ => Vec::new(),
            };
            Some((name, definition.child_by_field_name("body")?, context))
        })
        .collect()
}

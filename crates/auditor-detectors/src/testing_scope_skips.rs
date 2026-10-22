//! Skip marks: the attribute, decorator, annotation, or call that disables
//! a test or a group of tests, and whether a reason for it is written down.

use auditor_ast::{AstNode, AstNodeKind};

use super::{call_view, compact, preorder};
use crate::ast_scan::nodes_of_kind;
use crate::{AuditedFile, Language, SourceRole};

/// What disables a test or a group of tests.
#[derive(Clone, Copy)]
pub(crate) struct SkipMark<'t> {
    pub(crate) node: AstNode<'t>,
    /// The first row of the declaration's attribute block, or the row of the
    /// statement that calls the skip.
    pub(crate) block_row: usize,
    /// The mark carries its own reason: an argument (`#[ignore = ".."]`,
    /// `@Disabled("..")`, `t.Skip("..")`), or a condition it runs under.
    pub(crate) stated: bool,
}

/// A group of tests disabled as a whole: a JavaScript `describe.skip`, a
/// Java test class annotated `@Disabled`, a skipped Python test class.
pub(crate) struct SkippedGroup<'t> {
    pub(crate) name: String,
    pub(crate) mark: SkipMark<'t>,
}

fn row(node: AstNode<'_>) -> usize {
    node.start_position().0
}

/// The Rust `#[ignore]` among a test's attributes; `#[ignore = ".."]`
/// states its reason.
pub(super) fn rust_skip_mark<'t>(attributes: &[AstNode<'t>]) -> Option<SkipMark<'t>> {
    let ignore = attributes.iter().copied().find(|attribute| {
        let inner = compact(attribute.text());
        inner == "#[ignore]" || inner.starts_with("#[ignore=")
    })?;
    Some(SkipMark {
        node: ignore,
        block_row: attributes.iter().copied().map(row).min()?,
        stated: ignore.text().contains('='),
    })
}

/// A `@pytest.mark.skip` or `@unittest.skip` decorator of a decorated
/// definition. A conditional skip (`skipif`, `skipIf`) names its condition.
pub(super) fn python_skip_mark(decorated: AstNode<'_>) -> Option<SkipMark<'_>> {
    decorated
        .named_children()
        .filter(|child| child.kind() == "decorator")
        .find_map(|decorator| {
            let expression = decorator.named_child(0)?;
            let (callee, arguments) = match expression.kind() {
                "call" => (
                    expression.child_by_field_name("function")?,
                    expression.child_by_field_name("arguments"),
                ),
                _ => (expression, None),
            };
            matches!(
                callee.text(),
                "pytest.mark.skip" | "mark.skip" | "unittest.skip" | "skip"
            )
            .then(|| SkipMark {
                node: decorator,
                block_row: row(decorated),
                stated: arguments.is_some_and(|arguments| states_reason(arguments.text())),
            })
        })
}

/// A JUnit `@Disabled`/`@Ignore` (or a TestNG `@Test(enabled = false)`)
/// among a declaration's annotations.
pub(super) fn java_skip_mark<'t>(
    declaration: AstNode<'t>,
    annotations: &[AstNode<'t>],
) -> Option<SkipMark<'t>> {
    annotations.iter().copied().find_map(|annotation| {
        let name = annotation
            .child_by_field_name("name")?
            .text()
            .rsplit('.')
            .next()?;
        let arguments = annotation.child_by_field_name("arguments");
        let stated = match name {
            "Disabled" | "Ignore" => {
                arguments.is_some_and(|arguments| states_reason(arguments.text()))
            }
            "Test"
                if arguments.is_some_and(|arguments| {
                    compact(arguments.text()).contains("enabled=false")
                }) =>
            {
                false
            }
            _ => return None,
        };
        Some(SkipMark {
            node: annotation,
            block_row: row(declaration),
            stated,
        })
    })
}

/// A JUnit `@Disabled` or `@Ignore` among a Kotlin declaration's
/// annotations.
pub(super) fn kotlin_skip_mark<'t>(
    declaration: AstNode<'t>,
    annotations: &[AstNode<'t>],
) -> Option<SkipMark<'t>> {
    let annotation = annotations.iter().copied().find(|annotation| {
        matches!(
            super::frameworks::kotlin_annotation_name(*annotation),
            "Disabled" | "Ignore"
        )
    })?;
    let arguments = annotation.first_descendant_of_kind(&AstNodeKind::from("value_arguments"));
    Some(SkipMark {
        node: annotation,
        block_row: row(declaration),
        stated: arguments.is_some_and(|arguments| states_reason(arguments.text())),
    })
}

/// An NUnit or MSTest `[Ignore]`, or an xUnit `[Fact(Skip = "..")]`/
/// `[Theory(Skip = "..")]`, among a C# declaration's attributes.
pub(super) fn csharp_skip_mark<'t>(
    declaration: AstNode<'t>,
    attributes: &[AstNode<'t>],
) -> Option<SkipMark<'t>> {
    attributes.iter().copied().find_map(|attribute| {
        let arguments = attribute
            .named_children()
            .find(|child| child.kind() == "attribute_argument_list");
        let stated = match super::frameworks::csharp_attribute_name(attribute) {
            "Ignore" => arguments.is_some_and(|arguments| states_reason(arguments.text())),
            "Fact" | "Theory" => {
                let skip = arguments?.named_children().find(|argument| {
                    argument
                        .named_child(0)
                        .is_some_and(|name| name.text() == "Skip")
                })?;
                let value = skip.named_child(1)?;
                states_reason(value.text())
            }
            _ => return None,
        };
        Some(SkipMark {
            node: attribute,
            block_row: row(declaration),
            stated,
        })
    })
}

/// The first `t.Skip`/`t.SkipNow` call in a Go test. One with arguments, or
/// under a condition inside the test, states why it skips.
pub(super) fn go_skip_mark<'t>(body: AstNode<'t>, context: &[String]) -> Option<SkipMark<'t>> {
    preorder(body).into_iter().find_map(|node| {
        let call = call_view(node, &Language::Go)?;
        let skips = call.segments.len() == 2
            && context.iter().any(|context| context == call.segments[0])
            && matches!(call.name(), "Skip" | "Skipf" | "SkipNow");
        skips.then(|| SkipMark {
            node,
            block_row: row(node),
            stated: !call.arguments.is_empty() || conditional(node, body),
        })
    })
}

/// A JavaScript skip (`it.skip`, `xit`): its API has no reason argument.
pub(super) fn js_skip_mark(call: AstNode<'_>) -> Option<SkipMark<'_>> {
    Some(SkipMark {
        node: call.child_by_field_name("function")?,
        block_row: row(call),
        stated: false,
    })
}

/// Whether `node` runs under a condition inside `body`.
fn conditional(node: AstNode<'_>, body: AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .take_while(|ancestor| ancestor.start_byte() > body.start_byte())
        .any(|ancestor| {
            matches!(
                ancestor.kind(),
                "if_statement"
                    | "expression_switch_statement"
                    | "type_switch_statement"
                    | "select_statement"
                    | "communication_case"
                    | "expression_case"
                    | "type_case"
            )
        })
}

/// Arguments that say something: not empty and not only an empty string.
fn states_reason(arguments: &str) -> bool {
    let inner = compact(arguments);
    let inner = inner
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim_start_matches("reason=")
        .trim_start_matches("value=");
    !matches!(inner, "" | "\"\"" | "''")
}

/// The test groups disabled as a whole in one file.
pub(crate) fn skipped_groups<'t>(root: AstNode<'t>, file: &AuditedFile) -> Vec<SkippedGroup<'t>> {
    match &file.language {
        Language::Java => nodes_of_kind(root, &["class_declaration"])
            .into_iter()
            .filter_map(|class| {
                let annotations = super::frameworks::java_annotations(class);
                Some(SkippedGroup {
                    name: class.child_by_field_name("name")?.text().to_string(),
                    mark: java_skip_mark(class, &annotations)?,
                })
            })
            .collect(),
        Language::Kotlin => nodes_of_kind(root, &["class_declaration"])
            .into_iter()
            .filter_map(|class| {
                let annotations = super::frameworks::kotlin_annotations(class);
                Some(SkippedGroup {
                    name: class
                        .named_children()
                        .find(|child| child.kind() == "type_identifier")?
                        .text()
                        .to_string(),
                    mark: kotlin_skip_mark(class, &annotations)?,
                })
            })
            .collect(),
        // NUnit and MSTest ignore a whole fixture with a class `[Ignore]`.
        Language::CSharp => nodes_of_kind(root, &["class_declaration"])
            .into_iter()
            .filter_map(|class| {
                let attributes = super::frameworks::csharp_attributes(class);
                let mark = csharp_skip_mark(class, &attributes)?;
                Some(SkippedGroup {
                    name: class.child_by_field_name("name")?.text().to_string(),
                    mark,
                })
            })
            .collect(),
        Language::Python if file.role == SourceRole::Test => {
            nodes_of_kind(root, &["decorated_definition"])
                .into_iter()
                .filter_map(|decorated| {
                    let class = decorated
                        .child_by_field_name("definition")
                        .filter(|definition| definition.kind() == "class_definition")?;
                    Some(SkippedGroup {
                        name: class.child_by_field_name("name")?.text().to_string(),
                        mark: python_skip_mark(decorated)?,
                    })
                })
                .collect()
        }
        Language::JavaScript | Language::TypeScript if file.role == SourceRole::Test => {
            nodes_of_kind(root, &["call_expression"])
                .into_iter()
                .filter_map(|call| {
                    let view = call_view(call, &file.language)?;
                    let skipped = match view.segments.as_slice() {
                        ["describe" | "context" | "suite", "skip"] => true,
                        [only] => matches!(*only, "xdescribe" | "xcontext" | "xsuite"),
                        _ => false,
                    };
                    let title = call
                        .child_by_field_name("arguments")?
                        .named_children()
                        .find(|argument| !argument.kind().contains("comment"))
                        .filter(|title| matches!(title.kind(), "string" | "template_string"))?;
                    skipped.then(|| SkippedGroup {
                        name: title.text().trim_matches(['\'', '"', '`']).to_string(),
                        mark: SkipMark {
                            node: call.child_by_field_name("function").unwrap_or(call),
                            block_row: row(call),
                            stated: false,
                        },
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A file's comments, for [`explained_by_comment`].
pub(crate) fn comments(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    nodes_of_kind(
        root,
        &[
            "comment",
            "line_comment",
            "block_comment",
            "multiline_comment",
        ],
    )
}

/// Whether a comment gives the reason for a skip.
///
/// A plain comment beside the mark (after it on its last line, inside the
/// declaration's attribute block, or ending on the row above the block)
/// does. A doc comment above the block, or a comment opening the test's
/// `body`, describes the test, so it counts only when it speaks of the skip
/// or failure or points to an issue; a comment elsewhere in the body only
/// when it names the skip (skip, disabled, ignored, manual); a comment above
/// the enclosing class only when it speaks of the skip. Tool directives (`eslint-disable`,
/// `noqa`) explain nothing.
pub(crate) fn explained_by_comment(
    mark: &SkipMark<'_>,
    comments: &[AstNode<'_>],
    body: Option<AstNode<'_>>,
) -> bool {
    let (mark_row, end_row) = (row(mark.node), mark.node.end_position().0);
    let class_row = enclosing_class_row(mark.node);
    let opening = body.and_then(opening_comment);
    comments.iter().any(|comment| {
        let Some(text) = comment_text(comment.text()) else {
            return false;
        };
        let (start, end) = (row(*comment), comment.end_position().0);
        let beside = (start == end_row && comment.start_byte() >= mark.node.end_byte())
            || (mark.block_row..=mark_row).contains(&start)
            || end + 1 == mark.block_row;
        let doc = ["///", "//!", "/**"]
            .iter()
            .any(|prefix| comment.text().starts_with(prefix));
        let describes = opening.is_some_and(|opening| opening.start_byte() == comment.start_byte());
        let inside = body.is_some_and(|body| {
            comment.start_byte() > body.start_byte() && comment.end_byte() < body.end_byte()
        });
        (beside && !doc)
            || ((beside || describes) && (mentions_skip(&text) || references_issue(comment.text())))
            || (inside && names_skip(&text))
            || (class_row.is_some_and(|class_row| end + 1 == class_row) && mentions_skip(&text))
    }) || opening
        .filter(|opening| opening.kind() == "expression_statement")
        .is_some_and(|docstring| mentions_skip(&docstring.text().to_ascii_lowercase()))
}

/// The first row of the class around a test (with its decorators or
/// annotations).
fn enclosing_class_row(node: AstNode<'_>) -> Option<usize> {
    let class = std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| matches!(ancestor.kind(), "class_declaration" | "class_definition"))?;
    let decorated = class
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition");
    Some(row(decorated.unwrap_or(class)))
}

/// What a test body opens with, when that is a comment or a Python
/// docstring.
fn opening_comment(body: AstNode<'_>) -> Option<AstNode<'_>> {
    let first = preorder(body)
        .into_iter()
        .skip(1)
        .find(|node| node.kind() != "statement_list")?;
    let docstring = first.kind() == "expression_statement"
        && first
            .named_child(0)
            .is_some_and(|child| child.kind() == "string");
    (first.kind().contains("comment") || docstring).then_some(first)
}

/// A comment's words, lower-cased, or `None` for an empty comment or a tool
/// directive.
fn comment_text(comment: &str) -> Option<String> {
    let text = comment
        .trim_start_matches(['/', '*', '#', ' ', '!'])
        .trim_end_matches(['/', '*', ' '])
        .to_ascii_lowercase();
    let directive = [
        "eslint",
        "@ts-",
        "prettier-ignore",
        "noqa",
        "type:",
        "nolint",
        "istanbul",
        "c8 ",
        "biome-ignore",
        "pylint:",
        "fmt:",
        "nosonar",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix));
    (!directive && !text.is_empty()).then_some(text)
}

fn mentions_skip(text: &str) -> bool {
    [
        "skip", "disabl", "ignor", "fail", "flaky", "broken", "todo", "fixme", "manual",
        "not pass", "n't pass", "enable", "not work", "n't work", "bug",
    ]
    .iter()
    .any(|word| text.contains(word))
}

/// Words that name the skip itself, which make a comment anywhere in the
/// test body its reason ("test ignored for now. Run manually."). The wider
/// [`mentions_skip`] words (`todo`, `fail`, `bug`) are common in bodies.
fn names_skip(text: &str) -> bool {
    ["skip", "disabl", "ignor", "manual"]
        .iter()
        .any(|word| text.contains(word))
}

/// An issue link or key: `#123`, `LANG-1655`, a URL, the word "issue".
fn references_issue(comment: &str) -> bool {
    let lower = comment.to_ascii_lowercase();
    let bytes = comment.as_bytes();
    lower.contains("issue")
        || lower.contains("http")
        || bytes.windows(2).enumerate().any(|(index, pair)| {
            let key = pair[0] == b'-'
                && index >= 2
                && bytes[index - 2..index].iter().all(u8::is_ascii_uppercase);
            (pair[0] == b'#' || key) && pair[1].is_ascii_digit()
        })
}

//! Test-scope facts for the test-quality detectors: the test functions each
//! framework declares, a uniform view of the calls inside them, and the
//! repository functions that assert on a test's behalf.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{AstNode, AstTree};

use crate::ast_scan::{self, AstScan};
use crate::{AuditedFile, Language};

#[path = "testing_scope_evidence.rs"]
mod evidence;
#[path = "testing_scope_frameworks.rs"]
mod frameworks;
#[path = "testing_scope_skips.rs"]
mod skips;
pub(crate) use evidence::{asserts, exempt_by_name, EvidenceScope};
pub(crate) use frameworks::test_cases;
pub(crate) use skips::{comments, explained_by_comment, skipped_groups, SkipMark};

/// Languages whose test frameworks the test-scope facts model.
pub(crate) fn models_tests(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
    )
}

/// Every test-scope file of a modelled language, parsed.
pub(crate) fn visit_test_scope(
    scan: AstScan<'_, '_>,
    mut visit: impl FnMut(&AuditedFile, &AstTree),
) {
    ast_scan::visit_ast_files(scan, models_tests, |file, tree, _| visit(file, tree));
}

/// Languages whose test declarations and skip marks are modelled: the
/// Tier-1 frameworks, Kotlin JUnit, and C# xUnit, NUnit, and MSTest. Only
/// the skip rule reads the Kotlin and C# tests so far.
pub(crate) fn models_skips(language: &Language) -> bool {
    models_tests(language) || matches!(language, Language::Kotlin | Language::CSharp)
}

/// Every test-scope file whose skip marks are modelled, parsed.
pub(crate) fn visit_skip_scope(
    scan: AstScan<'_, '_>,
    mut visit: impl FnMut(&AuditedFile, &AstTree),
) {
    ast_scan::visit_ast_files(scan, models_skips, |file, tree, _| visit(file, tree));
}

/// One test function a framework runs.
pub(crate) struct TestCase<'t> {
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) body: AstNode<'t>,
    /// Names bound to the framework's per-test context: Go's `*testing.T`,
    /// an AVA or node:test `t`, a Mocha `done` callback.
    pub(crate) context: Vec<String>,
    /// Disabled: `#[ignore]`, `@Disabled`, `it.skip`, `t.Skip()`.
    pub(crate) skipped: bool,
    /// The mark that disables the test, when it is a skip rather than an
    /// expected failure (`xfail`) or a placeholder (`it.todo`).
    pub(crate) skip: Option<SkipMark<'t>>,
    /// Passes by failing: `#[should_panic]`, `@Test(expected = ..)`.
    pub(crate) expects_failure: bool,
}

/// A call in any modelled language.
pub(crate) struct CallView<'t> {
    pub(crate) node: AstNode<'t>,
    /// Callee identifiers, receiver first: `assert.Equal` is `[assert, Equal]`.
    /// A call in the receiver chain ends them: `expect(x).toBe` is `[toBe]`.
    pub(crate) segments: Vec<&'t str>,
    /// The receiver expression of a member call.
    pub(crate) receiver: Option<AstNode<'t>>,
    /// Each argument's source text, without whitespace.
    pub(crate) arguments: Vec<String>,
    pub(crate) is_macro: bool,
}

impl CallView<'_> {
    pub(crate) fn name(&self) -> &str {
        self.segments.last().copied().unwrap_or_default()
    }
}

pub(crate) fn call_view<'t>(node: AstNode<'t>, language: &Language) -> Option<CallView<'t>> {
    match (language, node.kind()) {
        (Language::Java, "method_invocation") => {
            let receiver = node.child_by_field_name("object");
            let mut segments = receiver.map(callee_segments).unwrap_or_default();
            segments.push(node.child_by_field_name("name")?.text());
            Some(CallView {
                node,
                segments,
                receiver,
                arguments: argument_texts(node.child_by_field_name("arguments")),
                is_macro: false,
            })
        }
        (Language::Rust, "macro_invocation") => {
            let name = node
                .child_by_field_name("macro")
                .or_else(|| node.named_child(0))?;
            let arguments = node
                .named_children()
                .find(|child| child.kind() == "token_tree")
                .map(|tree| split_token_tree(tree.text()))
                .unwrap_or_default();
            Some(CallView {
                node,
                segments: callee_segments(name),
                receiver: None,
                arguments,
                is_macro: true,
            })
        }
        (Language::Python, "call")
        | (
            Language::Rust | Language::JavaScript | Language::TypeScript | Language::Go,
            "call_expression",
        ) => function_call(node),
        _ => None,
    }
}

fn function_call(node: AstNode<'_>) -> Option<CallView<'_>> {
    let callee = node.child_by_field_name("function")?;
    Some(CallView {
        node,
        segments: callee_segments(callee),
        receiver: member_parts(callee).and_then(|(object, _)| callee.child_by_field_name(object)),
        arguments: argument_texts(node.child_by_field_name("arguments")),
        is_macro: false,
    })
}

/// The object and member fields of a member-access node.
pub(super) fn member_parts(node: AstNode<'_>) -> Option<(&'static str, &'static str)> {
    Some(match node.kind() {
        "member_expression" => ("object", "property"),
        "attribute" => ("object", "attribute"),
        "selector_expression" => ("operand", "field"),
        "field_expression" => ("value", "field"),
        "field_access" => ("object", "field"),
        _ => return None,
    })
}

fn callee_segments(node: AstNode<'_>) -> Vec<&str> {
    // `assert_send::<T>` names `assert_send`.
    if node.kind() == "generic_function" {
        return node
            .child_by_field_name("function")
            .map(callee_segments)
            .unwrap_or_default();
    }
    if node.kind() == "scoped_identifier" {
        let mut segments = node
            .child_by_field_name("path")
            .map(callee_segments)
            .unwrap_or_default();
        segments.extend(node.child_by_field_name("name").map(|name| name.text()));
        return segments;
    }
    let Some((object, member)) = member_parts(node) else {
        return if node.named_child_count() == 0 {
            vec![node.text()]
        } else {
            Vec::new()
        };
    };
    let mut segments = node
        .child_by_field_name(object)
        .map(callee_segments)
        .unwrap_or_default();
    segments.extend(node.child_by_field_name(member).map(|name| name.text()));
    segments
}

pub(crate) fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn argument_texts(arguments: Option<AstNode<'_>>) -> Vec<String> {
    arguments
        .map(|arguments| {
            arguments
                .named_children()
                .filter(|argument| !argument.kind().contains("comment"))
                .map(|argument| compact(argument.text()))
                .collect()
        })
        .unwrap_or_default()
}

/// The comma-separated arguments of a Rust macro's token tree.
fn split_token_tree(text: &str) -> Vec<String> {
    let inner = text
        .get(1..text.len().saturating_sub(1))
        .unwrap_or_default();
    let mut arguments = Vec::new();
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    let mut current = String::new();
    for character in inner.chars() {
        if in_string {
            in_string = escaped || character != '"';
            escaped = !escaped && character == '\\';
        } else {
            match character {
                '"' => in_string = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    arguments.push(compact(&current));
                    current.clear();
                    continue;
                }
                _ => {}
            }
        }
        current.push(character);
    }
    arguments.push(compact(&current));
    arguments.retain(|argument| !argument.is_empty());
    arguments
}

/// Every named node under `root`, in source order, `root` included.
pub(crate) fn preorder(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        let children = node.named_children().collect::<Vec<_>>();
        pending.extend(children.into_iter().rev());
    }
    nodes
}

/// Repository functions of one language: every declared name, and the names
/// whose bodies assert directly or by calling another asserting function.
#[derive(Default)]
pub(crate) struct LanguageHelpers {
    pub(super) declared: BTreeSet<String>,
    pub(super) asserting: BTreeSet<String>,
}

/// Function names a test may delegate its assertions to, per language.
#[derive(Default)]
pub(crate) struct AssertingHelpers {
    languages: BTreeMap<Language, LanguageHelpers>,
}

impl AssertingHelpers {
    pub(crate) fn collect(scan: AstScan<'_, '_>) -> Self {
        // (name, asserts directly, called names) per function, per language.
        type Summary = (String, bool, BTreeSet<String>);
        let mut summaries: BTreeMap<Language, Vec<Summary>> = BTreeMap::new();
        visit_test_scope(scan, |file, tree| {
            let language = &file.language;
            let entries = summaries.entry(language.clone()).or_default();
            for (name, body, context) in frameworks::functions(tree.root_node(), language) {
                let scope = EvidenceScope {
                    language,
                    context: &context,
                    helpers: None,
                };
                let called = preorder(body)
                    .into_iter()
                    .filter_map(|node| call_view(node, language))
                    .map(|call| call.name().to_string())
                    .collect();
                entries.push((name.to_string(), asserts(body, &scope), called));
            }
        });
        let languages = summaries
            .into_iter()
            .map(|(language, entries)| {
                let declared = entries.iter().map(|entry| entry.0.clone()).collect();
                let mut asserting: BTreeSet<String> = entries
                    .iter()
                    .filter(|entry| entry.1)
                    .map(|entry| entry.0.clone())
                    .collect();
                loop {
                    let reached = entries
                        .iter()
                        .filter(|entry| !asserting.contains(&entry.0))
                        .filter(|entry| entry.2.iter().any(|name| asserting.contains(name)))
                        .map(|entry| entry.0.clone())
                        .collect::<Vec<_>>();
                    if reached.is_empty() {
                        break;
                    }
                    asserting.extend(reached);
                }
                (
                    language,
                    LanguageHelpers {
                        declared,
                        asserting,
                    },
                )
            })
            .collect();
        Self { languages }
    }

    pub(crate) fn for_language(&self, language: &Language) -> Option<&LanguageHelpers> {
        self.languages.get(language)
    }
}

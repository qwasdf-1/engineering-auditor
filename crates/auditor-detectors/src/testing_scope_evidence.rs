//! Whether a test body can fail on a wrong result, and the test names that
//! declare a check other than an assertion.

use auditor_ast::AstNode;

use super::{call_view, member_parts, preorder, CallView, LanguageHelpers};
use crate::Language;

/// What a body's nodes are judged against.
pub(crate) struct EvidenceScope<'s> {
    pub(crate) language: &'s Language,
    pub(crate) context: &'s [String],
    pub(crate) helpers: Option<&'s LanguageHelpers>,
}

/// Whether `body` can fail on a wrong result: an assertion, a failure call
/// or statement, a compile-time check, a use of the test context, or a call
/// to a helper that asserts.
pub(crate) fn asserts(body: AstNode<'_>, scope: &EvidenceScope<'_>) -> bool {
    preorder(body)
        .into_iter()
        .any(|node| node_asserts(node, scope))
}

fn node_asserts(node: AstNode<'_>, scope: &EvidenceScope<'_>) -> bool {
    failing_statement(node, scope.language)
        || static_check(node, scope.language)
        || mentions_context(node, scope.context)
        || call_view(node, scope.language).is_some_and(|call| call_asserts(&call, scope))
}

/// Statements that fail the test: assertions, raises, Rust's `?`, and a
/// Rust macro argument that unwraps or asserts (macro arguments are tokens,
/// not parsed calls).
fn failing_statement(node: AstNode<'_>, language: &Language) -> bool {
    match language {
        Language::Python => matches!(node.kind(), "assert_statement" | "raise_statement"),
        Language::Java => matches!(node.kind(), "assert_statement" | "throw_statement"),
        Language::JavaScript | Language::TypeScript => node.kind() == "throw_statement",
        Language::Rust => {
            node.kind() == "try_expression"
                || (node.kind() == "token_tree"
                    && ["unwrap()", ".expect(", "assert", "panic!"]
                        .iter()
                        .any(|marker| node.text().contains(marker)))
        }
        _ => false,
    }
}

/// Checks the compiler makes: a typed discarded binding (`let _: T = ..`,
/// `var _ T = ..`) and TypeScript's type-level assertions.
fn static_check(node: AstNode<'_>, language: &Language) -> bool {
    let typed_discard = |pattern: &str| {
        node.child_by_field_name(pattern)
            .is_some_and(|pattern| pattern.text() == "_")
            && node.child_by_field_name("type").is_some()
    };
    match (language, node.kind()) {
        (Language::Rust, "let_declaration") => typed_discard("pattern"),
        (Language::Go, "var_spec") => typed_discard("name"),
        (Language::TypeScript, "type_alias_declaration" | "satisfies_expression") => true,
        (Language::TypeScript, "comment") => {
            node.text().contains("@ts-expect-error") || node.text().contains("@ts-ignore")
        }
        (Language::TypeScript, "call_expression") => {
            node.child_by_field_name("type_arguments").is_some()
        }
        (Language::TypeScript, "variable_declarator") => node.child_by_field_name("type").is_some(),
        _ => false,
    }
}

/// A use of the per-test context other than a passive method (`t.Log`):
/// `t.Errorf(..)`, `helper(t)`, `{ end: done }`.
fn mentions_context(node: AstNode<'_>, context: &[String]) -> bool {
    if node.kind() != "identifier" || !context.iter().any(|name| name == node.text()) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return true;
    };
    if matches!(
        parent.kind(),
        "parameter_declaration" | "formal_parameters" | "required_parameter" | "optional_parameter"
    ) {
        return false;
    }
    member_parts(parent)
        .and_then(|(_, member)| parent.child_by_field_name(member))
        .is_none_or(|member| !PASSIVE_CONTEXT_METHODS.contains(&member.text()))
}

fn call_asserts(call: &CallView<'_>, scope: &EvidenceScope<'_>) -> bool {
    let language = scope.language;
    let name = call.name();
    let asserting_name = call
        .segments
        .iter()
        .any(|segment| asserting_word(segment, language));
    let rust_failure = *language == Language::Rust
        && ((call.is_macro && !PLAIN_RUST_MACROS.contains(&name))
            || (name.starts_with("unwrap") && !name.starts_with("unwrap_or")));
    asserting_name
        || rust_failure
        || scope
            .helpers
            .is_some_and(|helpers| delegates(call, language, helpers))
}

/// A call to a repository function that asserts, a passed asserting
/// function value (`t.Run(name, tc.test)`), or an undeclared method
/// inherited from a framework base class.
fn delegates(call: &CallView<'_>, language: &Language, helpers: &LanguageHelpers) -> bool {
    let name = call.name();
    let passes_asserting = call.arguments.iter().any(|argument| {
        argument
            .rsplit(['.', ':'])
            .next()
            .is_some_and(|last| helpers.asserting.contains(last))
    });
    if helpers.asserting.contains(name) || passes_asserting {
        return true;
    }
    let receiver = call.receiver.map(|receiver| receiver.text());
    let inherited = match language {
        Language::Java => matches!(receiver, None | Some("this" | "super")),
        Language::Python => matches!(receiver, Some("self" | "cls")),
        _ => false,
    };
    inherited && !helpers.declared.contains(name)
}

/// Rust macros that format, collect, or log; any other macro may assert.
const PLAIN_RUST_MACROS: &[&str] = &[
    "println",
    "print",
    "eprintln",
    "eprint",
    "format",
    "format_args",
    "write",
    "writeln",
    "vec",
    "dbg",
    "matches",
    "concat",
    "stringify",
    "include_str",
    "include_bytes",
    "env",
    "cfg",
    "line",
    "file",
    "column",
    "json",
    "trace",
    "debug",
    "info",
    "warn",
    "error",
    "thread_local",
];

/// Context methods that log or configure rather than check.
const PASSIVE_CONTEXT_METHODS: &[&str] = &[
    "Log", "Logf", "Helper", "Parallel", "Cleanup", "Setenv", "TempDir", "Name", "Deadline",
    "Chdir", "Context", "Run", "Attr", "Output", "log", "teardown", "timeout", "plan", "title",
    "context",
];

fn asserting_word(segment: &str, language: &Language) -> bool {
    let lower = segment.to_ascii_lowercase();
    [
        "assert", "expect", "verify", "should", "raises", "throws", "warns",
    ]
    .iter()
    .any(|word| lower.contains(word))
        || IMPLICIT_ASSERTION_PREFIXES
            .iter()
            .any(|word| lower.starts_with(word))
        || (*language == Language::Go && segment == "require")
        || (*language == Language::Java && segment == "then")
}

/// Calls that fail when their check fails: failure and check verbs, and the
/// Playwright and Testing Library waits and queries that throw when the
/// element never appears.
const IMPLICIT_ASSERTION_PREFIXES: &[&str] = &[
    "fail",
    "check",
    "ensure",
    "must",
    "validate",
    "waitfor",
    "getby",
    "getallby",
    "findby",
    "findallby",
];

/// A test whose name declares a check other than an assertion: a smoke test
/// passing when the code runs without an exception, panic, or crash, a
/// constructor-exists test, a placeholder, or (TypeScript) a type test the
/// compiler checks.
pub(crate) fn exempt_by_name(language: &Language, path: &str, name: &str) -> bool {
    let compact = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    let type_test = *language == Language::TypeScript
        && (path.to_ascii_lowercase().contains("type")
            || ["type", "infer"]
                .iter()
                .any(|marker| compact.contains(marker)));
    type_test || SMOKE_MARKERS.iter().any(|marker| compact.contains(marker))
}

const SMOKE_MARKERS: &[&str] = &[
    "smoke",
    "nothrow",
    "notthrow",
    "doesntthrow",
    "noexception",
    "noerror",
    "noraise",
    "notraise",
    "doesntraise",
    "nopanic",
    "notpanic",
    "doesntpanic",
    "nocrash",
    "notcrash",
    "doesntcrash",
    "notfail",
    "doesntfail",
    "notexit",
    "doesntexit",
    "withoutcrash",
    "withouterror",
    "withoutthrow",
    "withoutexception",
    "withoutpanic",
    "withoutfail",
    "withoutrais",
    "compiles",
    "contextloads",
    "constructor",
    "constructable",
    "instantiat",
    "dummy",
    "placeholder",
];

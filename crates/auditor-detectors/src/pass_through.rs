//! `pass_through_wrapper`: a function whose whole body calls one other
//! function with its own parameters, unchanged and in order. It adds a name
//! and a layer of indirection and nothing else: no default, conversion,
//! check, or error handling, so its callers could call the target directly.
//!
//! Only private free functions (and Java static methods) are read. A
//! method's forwarding call is delegation, often required by the interface
//! it implements. A public function is API: a facade over internals, a
//! convenience name, or an overload, which callers outside the module cannot
//! replace with its target; a private one's callers can. The target must be
//! a function, named plainly or through a module, package, or type (a JS
//! global namespace such as `Array` or `JSON`): a method of a data object
//! (`PATTERN.test(s)`, `MAP.get(k)`) binds that object, which the caller
//! does not supply, and a constructor (`Self(x)`, a capitalized Python
//! class) builds a value. A function is not reported when it takes no
//! parameters (an
//! accessor or hook), is decorated or annotated (`@Override`, a Python
//! decorator), implements a trait, is a platform shim (`#[cfg(..)]`, a Go
//! build-constrained file) or an FFI export, or is documented as deprecated
//! or as an alias, which keeps an old name on purpose. A call with explicit
//! type arguments (`parse::<T>(s)`, `load<T>(s)`) adds them, so it is not a
//! pass-through.

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::Language;

#[path = "pass_through_forms.rs"]
mod forms;
use forms::{
    go_build_constrained, go_wrappers, java_wrappers, js_wrappers, namespaces, python_wrappers,
    rust_wrappers,
};

fn supported(language: &Language) -> bool {
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

pub(crate) fn pass_through_wrapper_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    ast_scan::site_findings(scan, supported, wrapper_sites, finding)
}

/// A candidate wrapper: its name, parameters in order, and the call its
/// body makes.
struct Wrapper<'t> {
    function: AstNode<'t>,
    name: &'t str,
    parameters: Vec<Forwarded>,
    callee: AstNode<'t>,
    arguments: Vec<Forwarded>,
    public: bool,
}

/// A parameter, or an argument that passes one on: plain (`x`), spread
/// (`*args`, `...rest`, `args...`), or by keyword (`x=x`, `**kwargs`).
#[derive(PartialEq, Eq, Clone, Debug)]
enum Forwarded {
    Plain(String),
    Spread(String),
    Keywords(String),
}

impl Forwarded {
    fn name(&self) -> &str {
        match self {
            Forwarded::Plain(name) | Forwarded::Spread(name) | Forwarded::Keywords(name) => name,
        }
    }
}

fn wrapper_sites<'a>(language: &Language, root: AstNode<'a>) -> Vec<(AstNode<'a>, String)> {
    if *language == Language::Go && go_build_constrained(root) {
        return Vec::new();
    }
    let wrappers = match language {
        Language::Rust => rust_wrappers(root),
        Language::Python => python_wrappers(root),
        Language::Go => go_wrappers(root),
        Language::Java => java_wrappers(root),
        _ => js_wrappers(root),
    };
    let namespaces = namespaces(language, root);
    wrappers
        .into_iter()
        .filter(|wrapper| !wrapper.public)
        .filter(|wrapper| targets_function(language, wrapper.callee, &namespaces))
        .filter(forwards_unchanged)
        .filter(|wrapper| !documented_alias(wrapper.function))
        .map(|wrapper| {
            let label = format!("{}\u{1f}{}", wrapper.name, wrapper.callee.text());
            (wrapper.function, label)
        })
        .collect()
}

/// A function target: a plain name, or one qualified by a namespace the
/// file imports (Rust paths and Java type names qualify by form); not a
/// constructor.
fn targets_function(language: &Language, callee: AstNode<'_>, namespaces: &[String]) -> bool {
    let text = callee.text();
    let segments: Vec<&str> = text
        .split(['.', ':'])
        .filter(|part| !part.is_empty())
        .collect();
    let last = segments.last().copied().unwrap_or(text);
    let capitalized = last.starts_with(|c: char| c.is_ascii_uppercase());
    // `LINEBREAK_MATCHER.test(s)`, `utils.PATTERN.test(s)`: a constant's method.
    let constant = segments[..segments.len().saturating_sub(1)]
        .iter()
        .any(|segment| {
            segment.len() > 1
                && segment
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        });
    if constant {
        return false;
    }
    match language {
        Language::Rust => !capitalized,
        Language::Java => text.split_once('.').is_none_or(|(object, _)| {
            object.starts_with(|c: char| c.is_ascii_uppercase())
                && object.chars().any(|c| c.is_ascii_lowercase())
                && !object.contains('.')
        }),
        Language::Python if capitalized => false,
        _ => text
            .split_once('.')
            .is_none_or(|(root, _)| namespaces.iter().any(|name| name == root)),
    }
}

/// Every parameter passed on once, in order, and nothing else passed; the
/// target is another function, not the wrapper itself.
fn forwards_unchanged(wrapper: &Wrapper<'_>) -> bool {
    let target = wrapper
        .callee
        .text()
        .rsplit(['.', ':'])
        .next()
        .unwrap_or_default();
    !wrapper.parameters.is_empty()
        && wrapper.arguments == wrapper.parameters
        && !(target == wrapper.name && !wrapper.callee.text().contains(['.', ':']))
        && !wrapper.parameters.iter().any(|parameter| {
            // `x.f(..)`: a method of a parameter, not a function.
            wrapper.callee.text().split(['.', ':']).next() == Some(parameter.name())
        })
}

/// A comment or docstring at the function, or at the class or impl around
/// it, that gives the indirection a purpose: an old name kept on purpose
/// (deprecated, an alias, compatibility), a test seam (a bridge, a mock
/// point), or a deferred lookup (lazy, at runtime), or a hook.
fn documented_alias(function: AstNode<'_>) -> bool {
    let container = std::iter::successors(function.parent(), |parent| parent.parent())
        .find(|ancestor| matches!(ancestor.kind(), "class_declaration" | "impl_item"));
    let mut texts = preceding_comments(function);
    texts.extend(container.map(preceding_comments).unwrap_or_default());
    if let Some(docstring) = python_docstring(function) {
        texts.push(docstring.text().to_string());
    }
    texts.iter().any(|text| {
        let lower = text.to_ascii_lowercase();
        [
            "deprecated",
            "alias",
            "backward",
            "compatib",
            "legacy",
            "kept for",
            "bridge",
            "testing",
            "mock",
            "seam",
            "lazy",
            "runtime",
            "hook",
        ]
        .iter()
        .any(|word| lower.contains(word))
    })
}

/// The comments directly above a declaration (or above its export or
/// attributes).
fn preceding_comments(function: AstNode<'_>) -> Vec<String> {
    let mut anchor = function;
    while let Some(parent) = anchor.parent() {
        if matches!(
            parent.kind(),
            "export_statement"
                | "lexical_declaration"
                | "variable_declarator"
                | "decorated_definition"
        ) {
            anchor = parent;
        } else {
            break;
        }
    }
    let Some(parent) = anchor.parent() else {
        return Vec::new();
    };
    let siblings: Vec<AstNode<'_>> = parent.named_children().collect();
    let Some(index) = siblings
        .iter()
        .position(|sibling| sibling.start_byte() == anchor.start_byte())
    else {
        return Vec::new();
    };
    siblings[..index]
        .iter()
        .rev()
        .take_while(|sibling| {
            sibling.kind().contains("comment") || sibling.kind() == "attribute_item"
        })
        .map(|comment| comment.text().to_string())
        .collect()
}

fn python_docstring(function: AstNode<'_>) -> Option<AstNode<'_>> {
    if function.kind() != "function_definition" {
        return None;
    }
    let first = function.child_by_field_name("body")?.named_child(0)?;
    (first.kind() == "expression_statement"
        && first
            .named_child(0)
            .is_some_and(|child| child.kind() == "string"))
    .then_some(first)
}

/// The statements of a body (through a Go `statement_list`), without
/// comments and a leading docstring.
fn statements(body: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut statements: Vec<AstNode<'_>> = body
        .named_children()
        .flat_map(|child| {
            if child.kind() == "statement_list" {
                child.named_children().collect::<Vec<_>>()
            } else {
                vec![child]
            }
        })
        .filter(|child| !child.kind().contains("comment"))
        .collect();
    if statements.first().is_some_and(|first| {
        first.kind() == "expression_statement"
            && first.named_child_count() == 1
            && first
                .named_child(0)
                .is_some_and(|child| child.kind() == "string")
    }) && statements.len() > 1
    {
        statements.remove(0);
    }
    statements
}

/// The one call a single-statement body makes: `return f(..)`, `f(..)`, or
/// a tail expression, through an `await`.
fn sole_call<'t>(body: AstNode<'t>, call_kind: &str) -> Option<AstNode<'t>> {
    let [statement] = statements(body)[..] else {
        return None;
    };
    let mut expression = statement;
    loop {
        match expression.kind() {
            kind if kind == call_kind => return Some(expression),
            "return_statement"
            | "expression_statement"
            | "return_expression"
            | "await_expression"
            | "await"
            | "expression_list"
            | "parenthesized_expression" => {
                let [inner] = expression
                    .named_children()
                    .filter(|child| !child.kind().contains("comment"))
                    .collect::<Vec<_>>()[..]
                else {
                    return None;
                };
                expression = inner;
            }
            _ => return None,
        }
    }
}

fn has_child_kind(node: AstNode<'_>, kind: &str) -> bool {
    node.named_children().any(|child| child.kind() == kind)
}

fn finding(function: AstNode<'_>, label: &str, relative: &str) -> Finding {
    let mut parts = label.split('\u{1f}');
    let name = parts.next().unwrap_or_default();
    let callee = java_callee_text(parts.next().unwrap_or_default());
    let line = function.start_position().0 + 1;
    auditor_core::finding!(
        "pass_through_wrapper",
        "structural",
        "Pass-through wrapper",
        format!(
            "`{name}` at line {line} only passes its parameters on to `{callee}`, unchanged; it adds a name and a call and nothing else"
        ),
        Severity::Low,
        Confidence::Medium,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Call the target directly and remove the wrapper, or make it earn its place: a narrower signature, a default, a conversion, validation, or error context.",
    )
    .with_identity("function", name)
    .with_evidence("target", callee.as_str())
}

/// A Java callee recorded as its whole invocation (`Foo.bar(a)`) is named
/// without the arguments.
fn java_callee_text(callee: &str) -> String {
    callee.split('(').next().unwrap_or(callee).to_string()
}

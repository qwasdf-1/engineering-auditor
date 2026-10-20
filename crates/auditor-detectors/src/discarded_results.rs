//! `discarded_fallible_result`: a call whose only failure signal is its
//! result, with the result thrown away. Rust `let _ =`, `_ =`, and `.ok();`
//! on durability I/O (`write_all`, `flush`, `sync_all`, `commit`, `fs::write`,
//! `fs::rename`); Go blank assignments of `Sync`, `Commit`, `os.WriteFile`,
//! and `os.Rename`; Java statements dropping the boolean `File` operations
//! return; a JavaScript or TypeScript promise from a same-file `async`
//! function, or a `.then` without a rejection handler, left floating; and a
//! Python coroutine from a same-file `async def` never awaited, or an
//! `asyncio.create_task` reference dropped. Go's bare unchecked calls stay
//! with `go_errcheck`; Rust's bare `#[must_use]` statements stay with the
//! compiler.

use std::collections::BTreeSet;

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::python_api_identity::PyScope;
use crate::{util, Language};

#[derive(Clone, Copy)]
enum Discard {
    /// The result is the failure signal.
    Failure,
    /// A promise nothing awaits or handles.
    Promise,
    /// A coroutine nothing awaits: its body never runs.
    Coroutine,
    /// A task whose only strong reference is dropped.
    Task,
}

struct Site<'a> {
    node: AstNode<'a>,
    api: String,
    discard: Discard,
}

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Go
            | Language::Java
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
    )
}

pub(crate) fn discarded_fallible_result_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, supported, |file, tree, _| {
        let root = tree.root_node();
        let sites = match file.language {
            Language::Rust => rust_sites(root),
            Language::Go => go_sites(root),
            Language::Java => java_sites(root),
            Language::Python => python_sites(root),
            Language::JavaScript | Language::TypeScript => js_sites(root),
            _ => Vec::new(),
        };
        let relative = util::path_slash(&file.relative);
        let lines = tree.source().lines().collect::<Vec<_>>();
        findings.extend(
            sites
                .into_iter()
                .filter(|site| !justified(&lines, site.node.start_position().0))
                .map(|site| finding(&site, &relative)),
        );
    });
    findings
}

fn finding(site: &Site<'_>, relative: &str) -> Finding {
    let line = site.node.start_position().0 + 1;
    let api = &site.api;
    let (message, recommendation, kind) = match site.discard {
        Discard::Failure => (
            format!("`{api}` at line {line} reports failure only through its result, which is discarded"),
            "Propagate or handle the result; a write, sync, rename, or commit that fails silently loses data the caller believes is stored.",
            "failure_result",
        ),
        Discard::Promise => (
            format!("`{api}` at line {line} returns a promise that is neither awaited nor given a rejection handler"),
            "Await the promise, return it, or attach a rejection handler; mark deliberate fire-and-forget calls with `void`.",
            "floating_promise",
        ),
        Discard::Coroutine => (
            format!("`{api}` at line {line} creates a coroutine that is never awaited, so its body never runs"),
            "Await the call, or schedule it as a task and keep the task.",
            "unawaited_coroutine",
        ),
        Discard::Task => (
            format!("`{api}` at line {line} drops the only reference to its task; the event loop keeps a weak one, so the task can be collected before it finishes"),
            "Keep the task (a set of running tasks, or a `TaskGroup`) until it completes.",
            "dropped_task",
        ),
    };
    auditor_core::finding!(
        "discarded_fallible_result",
        "correctness",
        "Fallible result discarded",
        message,
        Severity::Medium,
        Confidence::Medium,
        relative,
        Some(line),
    )
    .with_recommendation(recommendation)
    .with_evidence("api", api.as_str())
    .with_evidence("discard", kind)
}

/// A comment on the site's line or the line above that says the discard is
/// deliberate (`// ignore errors: we are exiting anyway`).
fn justified(lines: &[&str], row: usize) -> bool {
    const REASONS: &[&str] = &[
        "ignor",
        "best effort",
        "best-effort",
        "intentional",
        "deliberate",
        "don't care",
        "fire and forget",
        "fire-and-forget",
    ];
    let start = row.saturating_sub(1);
    lines
        .get(start..=row)
        .into_iter()
        .flatten()
        .any(|line| util::comment_gives_reason(line, REASONS))
}

fn site(node: AstNode<'_>, api: impl Into<String>, discard: Discard) -> Site<'_> {
    Site {
        node,
        api: api.into(),
        discard,
    }
}

/// Rust methods and functions whose `Result` is the only sign a write, a
/// sync, or a commit failed.
const RUST_DURABLE_METHODS: &[&str] = &[
    "write_all",
    "flush",
    "sync_all",
    "sync_data",
    "commit",
    "persist",
];
const RUST_DURABLE_FUNCTIONS: &[&str] = &["fs::write", "fs::rename", "fs::copy"];

fn rust_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    ast_scan::nodes_of_kind(root, &["let_declaration", "expression_statement"])
        .into_iter()
        .filter_map(rust_discarded_call)
        .filter(|call| !ast_scan::is_inside_rust_test(*call) && !inside_rust_drop(*call))
        .filter_map(|call| rust_durable(call).map(|api| site(call, api, Discard::Failure)))
        .collect()
}

/// The call whose value `let _ =`, `_ =`, or a trailing `.ok();` discards.
fn rust_discarded_call(statement: AstNode<'_>) -> Option<AstNode<'_>> {
    let value = if statement.kind() == "let_declaration" {
        statement
            .child_by_field_name("pattern")
            .filter(|pattern| pattern.text() == "_")?;
        statement.child_by_field_name("value")?
    } else {
        let expression = statement.named_child(0)?;
        match expression.kind() {
            "assignment_expression" => {
                expression
                    .child_by_field_name("left")
                    .filter(|left| left.text() == "_")?;
                expression.child_by_field_name("right")?
            }
            "call_expression" => {
                let function = expression.child_by_field_name("function")?;
                let ok = function.kind() == "field_expression"
                    && function
                        .child_by_field_name("field")
                        .is_some_and(|field| field.text() == "ok")
                    && expression
                        .child_by_field_name("arguments")
                        .is_some_and(|arguments| arguments.named_child_count() == 0);
                ok.then(|| function.child_by_field_name("value"))??
            }
            _ => return None,
        }
    };
    let value = if value.kind() == "await_expression" {
        value.named_child(0)?
    } else {
        value
    };
    (value.kind() == "call_expression").then_some(value)
}

fn rust_durable(call: AstNode<'_>) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    match function.kind() {
        "field_expression" => {
            let method = function.child_by_field_name("field")?.text();
            let receiver = function.child_by_field_name("value")?.text();
            let console = receiver.contains("stdout") || receiver.contains("stderr");
            (RUST_DURABLE_METHODS.contains(&method) && !console && !in_memory(call, receiver))
                .then(|| format!("{}.{method}", short(receiver)))
        }
        "scoped_identifier" => {
            let path = function.text();
            RUST_DURABLE_FUNCTIONS
                .iter()
                .any(|suffix| path == *suffix || path.ends_with(&format!("::{suffix}")))
                .then(|| path.to_string())
        }
        _ => None,
    }
}

/// A receiver the enclosing function binds to an in-memory buffer (a `Vec`,
/// a `String`, a `BytesMut` writer, a `Cursor`), whose writes cannot fail.
fn in_memory(call: AstNode<'_>, receiver: &str) -> bool {
    const BUFFERS: &[&str] = &[
        "Vec::",
        "vec!",
        "Vec<u8>",
        ".writer()",
        "BytesMut",
        "String::",
        "&mut String",
        "Cursor::",
        "io::sink",
    ];
    let Some(function) = std::iter::successors(call.parent(), |parent| parent.parent())
        .find(|ancestor| ancestor.kind() == "function_item")
    else {
        return false;
    };
    let declarations = ast_scan::nodes_of_kind(function, &["let_declaration", "parameter"]);
    declarations.into_iter().any(|declaration| {
        let bound = declaration
            .child_by_field_name("pattern")
            .map(|pattern| pattern.text().trim_start_matches("mut ").trim());
        let value = declaration
            .child_by_field_name("value")
            .or_else(|| declaration.child_by_field_name("type"));
        bound == Some(receiver)
            && value.is_some_and(|value| BUFFERS.iter().any(|buffer| value.text().contains(buffer)))
    })
}

/// A `Drop` implementation cannot return an error, so discarding there is
/// the only option.
fn inside_rust_drop(node: AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent()).any(|ancestor| {
        ancestor.kind() == "function_item"
            && ancestor
                .child_by_field_name("name")
                .is_some_and(|name| name.text() == "drop")
    })
}

fn go_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    let mut sites = Vec::new();
    for statement in
        ast_scan::nodes_of_kind(root, &["assignment_statement", "expression_statement"])
    {
        let (call, blank) = if statement.kind() == "assignment_statement" {
            let left = statement.child_by_field_name("left");
            let right = statement.child_by_field_name("right");
            let blank =
                left.is_some_and(|left| left.named_children().all(|name| name.text() == "_"));
            let call = right
                .filter(|right| right.named_child_count() == 1)
                .and_then(|right| right.named_child(0));
            (call, blank)
        } else {
            (statement.named_child(0), false)
        };
        let Some(call) = call.filter(|call| call.kind() == "call_expression") else {
            continue;
        };
        if statement.kind() == "assignment_statement" && !blank {
            continue;
        }
        if let Some(api) = go_durable(call, blank) {
            sites.push(site(call, api, Discard::Failure));
        }
    }
    sites
}

/// `os.WriteFile` and `os.Rename` return only an error; `Sync` and `Commit`
/// are counted only when a blank assignment shows they return a value.
fn go_durable(call: AstNode<'_>, blank: bool) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "selector_expression" {
        return None;
    }
    let operand = function.child_by_field_name("operand")?.text();
    let field = function.child_by_field_name("field")?.text();
    let arguments = call.child_by_field_name("arguments")?.named_child_count();
    let os_function = matches!(
        (operand, field),
        ("os", "WriteFile" | "Rename") | ("ioutil", "WriteFile")
    );
    // A logger's or a console's `Sync` fails harmlessly on terminals.
    let console = ["log", "Log", "Stdout", "Stderr"]
        .iter()
        .any(|marker| operand.contains(marker));
    let method = blank && arguments == 0 && matches!(field, "Sync" | "Commit") && !console;
    (os_function || method).then(|| format!("{}.{field}", short(operand)))
}

/// `java.io.File` operations that report failure by returning `false`.
fn java_boolean_file_operation(name: &str, arguments: usize) -> bool {
    match name {
        "mkdir" | "mkdirs" | "createNewFile" | "setReadOnly" => arguments == 0,
        "renameTo" | "setLastModified" => arguments == 1,
        "setReadable" | "setWritable" | "setExecutable" => matches!(arguments, 1 | 2),
        _ => false,
    }
}

fn java_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    let files = java_file_variables(root);
    ast_scan::nodes_of_kind(root, &["expression_statement"])
        .into_iter()
        .filter_map(|statement| statement.named_child(0))
        .filter(|call| call.kind() == "method_invocation")
        .filter_map(|call| {
            let name = call.child_by_field_name("name")?.text();
            let arguments = call.child_by_field_name("arguments")?.named_child_count();
            let object = call.child_by_field_name("object")?;
            let file_receiver = (object.kind() == "object_creation_expression"
                && object
                    .child_by_field_name("type")
                    .is_some_and(|kind| kind.text() == "File"))
                || files.contains(object.text());
            let fallible = java_boolean_file_operation(name, arguments)
                || (name == "delete" && arguments == 0 && file_receiver);
            fallible.then(|| {
                site(
                    call,
                    format!("{}.{name}", short(object.text())),
                    Discard::Failure,
                )
            })
        })
        .collect()
}

/// Names the file declares with type `File`: locals, fields, parameters.
fn java_file_variables(root: AstNode<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for declaration in ast_scan::nodes_of_kind(
        root,
        &[
            "local_variable_declaration",
            "field_declaration",
            "formal_parameter",
        ],
    ) {
        if declaration
            .child_by_field_name("type")
            .is_none_or(|kind| kind.text() != "File")
        {
            continue;
        }
        if let Some(name) = declaration.child_by_field_name("name") {
            names.insert(name.text().to_string());
        }
        for declarator in declaration.named_children() {
            if declarator.kind() == "variable_declarator" {
                if let Some(name) = declarator.child_by_field_name("name") {
                    names.insert(name.text().to_string());
                }
            }
        }
    }
    names
}

/// Function and method names a file defines, split by whether every
/// definition of the name is `async`.
#[derive(Default)]
struct AsyncNames {
    functions: BTreeSet<String>,
    methods: BTreeSet<String>,
    synchronous: BTreeSet<String>,
}

impl AsyncNames {
    fn function(&self, name: &str) -> bool {
        self.functions.contains(name) && !self.synchronous.contains(name)
    }

    fn method(&self, name: &str) -> bool {
        self.methods.contains(name) && !self.synchronous.contains(name)
    }

    fn record(&mut self, name: &str, asynchronous: bool, method: bool) {
        let set = match (asynchronous, method) {
            (false, _) => &mut self.synchronous,
            (true, true) => &mut self.methods,
            (true, false) => &mut self.functions,
        };
        set.insert(name.to_string());
    }
}

fn has_async_keyword(node: AstNode<'_>) -> bool {
    node.children().any(|child| child.kind() == "async")
}

fn python_async_names(root: AstNode<'_>) -> AsyncNames {
    const TRANSPARENT: &[&str] = &["staticmethod", "classmethod", "abstractmethod", "override"];
    let mut names = AsyncNames::default();
    for definition in ast_scan::nodes_of_kind(root, &["function_definition"]) {
        let Some(name) = definition.child_by_field_name("name") else {
            continue;
        };
        let parent = definition.parent();
        // A decorator can wrap a coroutine function in a synchronous one.
        let rewrapped = parent
            .filter(|parent| parent.kind() == "decorated_definition")
            .is_some_and(|decorated| {
                decorated
                    .named_children()
                    .filter(|child| child.kind() == "decorator")
                    .any(|decorator| {
                        let text = decorator.text().trim_start_matches('@');
                        !TRANSPARENT.iter().any(|known| text.ends_with(known))
                    })
            });
        let holder = if parent.is_some_and(|parent| parent.kind() == "decorated_definition") {
            parent.and_then(|parent| parent.parent())
        } else {
            parent
        };
        let method = holder
            .and_then(|block| block.parent())
            .is_some_and(|owner| owner.kind() == "class_definition");
        names.record(
            name.text(),
            has_async_keyword(definition) && !rewrapped,
            method,
        );
    }
    names
}

fn python_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    let names = python_async_names(root);
    let scope = PyScope::new(root);
    let mut sites = Vec::new();
    for statement in ast_scan::nodes_of_kind(root, &["expression_statement"]) {
        let Some(call) = statement
            .named_child(0)
            .filter(|call| call.kind() == "call")
        else {
            continue;
        };
        let Some(function) = call.child_by_field_name("function") else {
            continue;
        };
        let local = match function.kind() {
            "identifier" => names.function(function.text()),
            "attribute" => {
                let object = function
                    .child_by_field_name("object")
                    .map(|object| object.text());
                let attribute = function
                    .child_by_field_name("attribute")
                    .map(|name| name.text());
                matches!(object, Some("self" | "cls"))
                    && attribute.is_some_and(|attribute| names.method(attribute))
            }
            _ => false,
        };
        let discard = match scope.resolve(function).as_deref() {
            _ if local => Some(Discard::Coroutine),
            Some("asyncio.sleep") => Some(Discard::Coroutine),
            Some("asyncio.create_task" | "asyncio.ensure_future") => Some(Discard::Task),
            _ => None,
        };
        if let Some(discard) = discard {
            sites.push(site(call, short(function.text()), discard));
        }
    }
    sites
}

fn js_async_names(root: AstNode<'_>) -> AsyncNames {
    let mut names = AsyncNames::default();
    for definition in ast_scan::nodes_of_kind(
        root,
        &[
            "function_declaration",
            "method_definition",
            "variable_declarator",
        ],
    ) {
        let Some(name) = definition.child_by_field_name("name") else {
            continue;
        };
        let asynchronous = match definition.kind() {
            "variable_declarator" => {
                let Some(value) = definition.child_by_field_name("value") else {
                    continue;
                };
                if !matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "function"
                ) {
                    continue;
                }
                has_async_keyword(value) && !handles_own_errors(value)
            }
            _ => has_async_keyword(definition) && !handles_own_errors(definition),
        };
        names.record(
            name.text(),
            asynchronous,
            definition.kind() == "method_definition",
        );
    }
    names
}

fn js_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    let names = js_async_names(root);
    let mut sites = Vec::new();
    for statement in ast_scan::nodes_of_kind(root, &["expression_statement"]) {
        // A script's top-level call is its entry point; an unhandled
        // rejection there ends the process with the error, like a throw.
        if statement
            .parent()
            .is_none_or(|parent| parent.kind() == "program")
        {
            continue;
        }
        let Some(call) = statement
            .named_child(0)
            .filter(|call| call.kind() == "call_expression")
        else {
            continue;
        };
        let Some(function) = call.child_by_field_name("function") else {
            continue;
        };
        if js_floating(function, call, &names) {
            sites.push(site(call, short(function.text()), Discard::Promise));
        }
    }
    sites
}

/// A function whose whole body is a `try` with a `catch`: its promise does
/// not reject on the errors the body raises.
fn handles_own_errors(function: AstNode<'_>) -> bool {
    let Some(body) = function.child_by_field_name("body") else {
        return false;
    };
    body.named_child_count() == 1
        && body.named_child(0).is_some_and(|statement| {
            statement.kind() == "try_statement"
                && statement.child_by_field_name("handler").is_some()
        })
}

fn js_floating(function: AstNode<'_>, call: AstNode<'_>, names: &AsyncNames) -> bool {
    match function.kind() {
        "identifier" => names.function(function.text()),
        "member_expression" => {
            let object = function.child_by_field_name("object");
            let property = function
                .child_by_field_name("property")
                .map(|property| property.text());
            let this_method = object.is_some_and(|object| object.kind() == "this")
                && property.is_some_and(|property| names.method(property));
            // `Promise.resolve().then(..)` schedules a microtask; a throw
            // there surfaces as it would from `queueMicrotask`.
            let microtask =
                object.is_some_and(|object| object.text().starts_with("Promise.resolve("));
            let unhandled_then = property == Some("then")
                && !microtask
                && call
                    .child_by_field_name("arguments")
                    .is_some_and(|arguments| arguments.named_child_count() == 1);
            this_method || unhandled_then
        }
        _ => false,
    }
}

/// A call's receiver or callee text, cut to one line; a callee spanning
/// lines keeps its first line and its final member (`call(..)….then`).
fn short(text: &str) -> String {
    let first = text.lines().next().unwrap_or(text).trim();
    let head = if first.chars().count() > 48 {
        format!("{}…", first.chars().take(47).collect::<String>())
    } else {
        first.to_string()
    };
    match text.lines().last().map(str::trim) {
        Some(last) if text.contains('\n') => {
            let member = last.rsplit('.').next().unwrap_or(last);
            format!("{}….{member}", head.trim_end_matches('…'))
        }
        _ => head,
    }
}

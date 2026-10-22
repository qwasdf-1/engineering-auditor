//! `unbounded_concurrency`: one concurrent task, thread, or request started
//! per item of a collection, with no limit on how many run at once. The
//! collection's size sets the concurrency: a large input opens that many
//! connections, file handles, or threads together, exhausting the process or
//! overwhelming the service it calls.
//!
//! Fan-out sites:
//! - Go: a `go` statement in a `range` loop.
//! - Rust: `tokio::spawn`, `tokio::task::spawn(_blocking)`,
//!   `std::thread::spawn`, or `async_std::task::spawn` in a `for` loop, and
//!   `futures::future::join_all`/`try_join_all` over a mapped iterator.
//! - Python: `asyncio.gather(*..)` over a comprehension or generator, and
//!   `asyncio.create_task`/`ensure_future`, a `create_task` method (a
//!   `TaskGroup`), or `threading.Thread` in a `for` loop.
//! - JavaScript/TypeScript: `Promise.all`/`allSettled` over a `.map(..)`.
//! - Java: `new Thread(..)` in a loop, and
//!   `Executors.newCachedThreadPool()`, which adds a thread per waiting task.
//!
//! A loop over a literal collection, or over workers, shards, partitions, or
//! registered handlers, listeners, callbacks, hooks, or plugins
//! (`for _, worker := range workers`), is bounded by construction, and a
//! `.map` whose callback takes a promise, future, or task, or calls its item
//! (`callback => callback(event)`), starts nothing new. A limit in
//! reach is a bound: a semaphore, limiter, or `p-limit` call, a Go send
//! before the `go` statement (`sem <- struct{}{}`), an errgroup `SetLimit`,
//! or `buffer_unordered`, anywhere in the enclosing function.

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, descendants, nodes_of_kind, AstScan};
use crate::java_api_identity::JavaScope;
use crate::js_api_identity::{js_arguments, JsScope};
use crate::python_api_identity::PyScope;
use crate::rust_api_identity::{rust_arguments, RustScope};
use crate::{util, Language};

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

const RUST_SPAWNS: &[&str] = &[
    "tokio::spawn",
    "tokio::task::spawn",
    "tokio::task::spawn_blocking",
    "std::thread::spawn",
    "async_std::task::spawn",
];

const RUST_JOINS: &[&str] = &[
    "futures::future::join_all",
    "futures::future::try_join_all",
    "futures::join_all",
    "futures::try_join_all",
];

const PYTHON_SPAWNS: &[&str] = &[
    "asyncio.create_task",
    "asyncio.ensure_future",
    "threading.Thread",
];

pub(crate) fn unbounded_concurrency_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    ast_scan::site_findings(scan, supported, fan_out_sites, finding)
}

fn fan_out_sites<'a>(language: &Language, root: AstNode<'a>) -> Vec<(AstNode<'a>, String)> {
    let sites = match language {
        Language::Go => go_sites(root),
        Language::Rust => rust_sites(root),
        Language::Python => python_sites(root),
        Language::Java => java_sites(root),
        _ => js_sites(root),
    };
    sites
        .into_iter()
        .filter(|(site, _)| !limited(*site))
        .collect()
}

fn go_sites(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    nodes_of_kind(root, &["go_statement"])
        .into_iter()
        .filter(|statement| {
            enclosing_loop(*statement).is_some_and(|node| {
                node.named_children()
                    .find(|child| child.kind() == "range_clause")
                    .and_then(|clause| clause.child_by_field_name("right"))
                    .is_some_and(unbounded_collection)
                    && !sends_before(node, *statement)
            })
        })
        .map(|statement| (statement, "go".to_string()))
        .collect()
}

/// Whether the loop body sends on a channel outside the goroutine, as a
/// semaphore acquire does.
fn sends_before(node: AstNode<'_>, statement: AstNode<'_>) -> bool {
    descendants(node).any(|child| {
        child.kind() == "send_statement" && child.start_byte() < statement.start_byte()
    })
}

fn rust_sites(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    RustScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(_, call)| !ast_scan::is_inside_rust_test(*call))
        .filter(|(api, call)| {
            if RUST_SPAWNS.contains(&api.as_str()) {
                return per_item(*call);
            }
            RUST_JOINS.contains(&api.as_str())
                && rust_arguments(*call)
                    .first()
                    .is_some_and(|argument| maps_collection(*argument))
        })
        .map(|(api, call)| (call, api))
        .collect()
}

fn python_sites(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    let scope = PyScope::new(root);
    let mut sites: Vec<_> = scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| match api.as_str() {
            "asyncio.gather" => call_arguments(*call).any(|argument| {
                argument.kind() == "list_splat"
                    && argument.named_child(0).is_some_and(|value| {
                        matches!(value.kind(), "list_comprehension" | "generator_expression")
                            && comprehension_source(value).is_some_and(unbounded_collection)
                    })
            }),
            api => PYTHON_SPAWNS.contains(&api) && per_item(*call),
        })
        .map(|(api, call)| (call, api))
        .collect();
    // `TaskGroup.create_task` and `loop.create_task` on any receiver.
    sites.extend(
        nodes_of_kind(root, &["call"])
            .into_iter()
            .filter(|call| {
                call.child_by_field_name("function")
                    .is_some_and(|function| {
                        function.kind() == "attribute"
                            && function
                                .child_by_field_name("attribute")
                                .is_some_and(|name| name.text() == "create_task")
                            && function
                                .child_by_field_name("object")
                                .is_some_and(|object| object.text() != "asyncio")
                    })
            })
            .filter(|call| per_item(*call))
            .map(|call| (call, "create_task".to_string())),
    );
    sites
}

fn call_arguments(call: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    call.child_by_field_name("arguments")
        .into_iter()
        .flat_map(|arguments| arguments.named_children())
}

/// The iterable of a comprehension's first `for` clause.
fn comprehension_source(node: AstNode<'_>) -> Option<AstNode<'_>> {
    node.named_children()
        .find(|child| child.kind() == "for_in_clause")?
        .child_by_field_name("right")
}

fn js_sites(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            matches!(api.as_str(), "Promise.all" | "Promise.allSettled")
                && js_arguments(*call)
                    .first()
                    .is_some_and(|argument| maps_collection(*argument))
        })
        .map(|(api, call)| (call, api))
        .collect()
}

fn java_sites(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    JavaScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| match api.as_str() {
            "java.util.concurrent.Executors.newCachedThreadPool" => true,
            "java.lang.Thread.<init>" | "Thread.<init>" => per_item(*call),
            _ => false,
        })
        .map(|(api, call)| (call, api))
        .collect()
}

/// Whether `call` runs once per item: its innermost loop iterates a
/// collection that is not bounded by construction.
fn per_item(call: AstNode<'_>) -> bool {
    enclosing_loop(call)
        .and_then(loop_iterable)
        .is_some_and(unbounded_collection)
}

/// The collection a loop iterates: Python `for x in c`, Rust `for x in c`,
/// Java `for (T x : c)`, JS `for (const x of c)`.
fn loop_iterable(node: AstNode<'_>) -> Option<AstNode<'_>> {
    match node.kind() {
        "for_statement" | "for_in_statement" => node.child_by_field_name("right"),
        "for_expression" | "enhanced_for_statement" => node.child_by_field_name("value"),
        _ => None,
    }
}

/// `c.map(..)` (`c.iter().map(..)`) on a collection not bounded by
/// construction.
fn maps_collection(argument: AstNode<'_>) -> bool {
    let Some(function) = argument.child_by_field_name("function") else {
        return false;
    };
    let (receiver, method) = match function.kind() {
        "member_expression" => (
            function.child_by_field_name("object"),
            function.child_by_field_name("property"),
        ),
        "field_expression" => (
            function.child_by_field_name("value"),
            function.child_by_field_name("field"),
        ),
        _ => return false,
    };
    method.is_some_and(|method| method.text() == "map")
        && receiver.is_some_and(unbounded_collection)
        && !argument
            .child_by_field_name("arguments")
            .and_then(|arguments| arguments.named_child(0))
            .is_some_and(settles_started_work)
}

/// A mapping callback that starts nothing new: its parameter is already a
/// promise, future, or task (`valueOrPromise => ..`), or it calls the item
/// itself (`callback => callback(event)`), running registered hooks.
fn settles_started_work(callback: AstNode<'_>) -> bool {
    let parameter = callback
        .child_by_field_name("parameter")
        .or_else(|| {
            callback
                .child_by_field_name("parameters")
                .and_then(|parameters| parameters.named_child(0))
        })
        .map(|parameter| {
            parameter
                .child_by_field_name("pattern")
                .unwrap_or(parameter)
                .text()
                .trim_matches('|')
        });
    let Some(parameter) = parameter else {
        return false;
    };
    let started = util::split_identifier(parameter).iter().any(|word| {
        matches!(
            word.as_str(),
            "promise" | "promises" | "future" | "futures" | "task" | "tasks" | "handle"
        )
    });
    let invokes_item = callback.child_by_field_name("body").is_some_and(|body| {
        body.kind() == "call_expression"
            && body
                .child_by_field_name("function")
                .is_some_and(|function| function.text() == parameter)
    });
    started || invokes_item
}

/// Whether a collection's size is not fixed by construction: not a literal,
/// a range, or a set of workers, shards, partitions, or cores.
fn unbounded_collection(node: AstNode<'_>) -> bool {
    if node.kind() == "parenthesized_expression" {
        return node.named_child(0).is_some_and(unbounded_collection);
    }
    let literal = matches!(
        node.kind(),
        "list"
            | "tuple"
            | "array"
            | "array_expression"
            | "array_initializer"
            | "composite_literal"
            | "range_expression"
            | "int_literal"
            | "integer"
            | "integer_literal"
            | "number"
    ) || (node.kind() == "call" && ast_scan::callee(node) == "range")
        || node.text().starts_with("vec!");
    let fixed = descendants(node)
        .filter(|child| {
            matches!(
                child.kind(),
                "identifier" | "field_identifier" | "property_identifier"
            )
        })
        .any(|child| {
            util::split_identifier(child.text()).iter().any(|word| {
                matches!(
                    word.as_str(),
                    "worker"
                        | "workers"
                        | "shard"
                        | "shards"
                        | "partition"
                        | "partitions"
                        | "core"
                        | "cores"
                        | "cpu"
                        | "cpus"
                        | "thread"
                        | "threads"
                        | "concurrency"
                        | "parallelism"
                        | "handler"
                        | "handlers"
                        | "listener"
                        | "listeners"
                        | "callback"
                        | "callbacks"
                        | "hook"
                        | "hooks"
                        | "plugin"
                        | "plugins"
                        | "subscriber"
                        | "subscribers"
                )
            })
        });
    // A receiver chain (`items.iter()`) is judged by its root collection.
    let chained = matches!(node.kind(), "call_expression" | "call")
        && node
            .child_by_field_name("function")
            .and_then(|function| function.child_by_field_name("value"))
            .is_some_and(|inner| !unbounded_collection(inner));
    !literal && !fixed && !chained
}

fn enclosing_loop(node: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(node.parent(), |node| node.parent())
        .take_while(|ancestor| !is_function(*ancestor) || is_closure_argument(*ancestor))
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "for_statement"
                    | "for_in_statement"
                    | "for_expression"
                    | "enhanced_for_statement"
                    | "while_statement"
                    | "while_expression"
                    | "loop_expression"
            )
        })
}

fn is_function(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "function_definition"
            | "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "constructor_declaration"
            | "lambda_expression"
            | "lambda"
            | "func_literal"
            | "function_item"
            | "closure_expression"
    )
}

/// A closure passed as an argument runs where it is passed (`thread::spawn(
/// move || ..)` inside a loop), unlike a named function.
fn is_closure_argument(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "closure_expression" | "lambda_expression" | "lambda" | "arrow_function" | "func_literal"
    ) && node
        .parent()
        .is_some_and(|parent| matches!(parent.kind(), "arguments" | "argument_list"))
}

/// Whether the function enclosing `site` limits concurrency: it names a
/// semaphore or limiter, calls `SetLimit`, `buffer_unordered`, or a
/// `p-limit` function, or acquires a permit.
fn limited(site: AstNode<'_>) -> bool {
    let scope = std::iter::successors(site.parent(), |node| node.parent())
        .find(|ancestor| is_function(*ancestor) && !is_closure_argument(*ancestor))
        .unwrap_or(site);
    descendants(scope)
        .filter(|child| {
            matches!(
                child.kind(),
                "identifier" | "field_identifier" | "property_identifier" | "type_identifier"
            )
        })
        .any(|child| {
            let words = util::split_identifier(child.text());
            words.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "semaphore" | "sem" | "limiter" | "limit" | "permit" | "permits" | "acquire"
                )
            }) || matches!(child.text(), "buffer_unordered" | "buffered" | "SetLimit")
        })
}

fn finding(site: AstNode<'_>, api: &str, relative: &str) -> Finding {
    let line = site.start_position().0 + 1;
    let name = api.trim_end_matches(".<init>");
    let short = name
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(name);
    let message = if short == "newCachedThreadPool" {
        format!(
            "`newCachedThreadPool` at line {line} creates a thread for every task submitted while the others are busy, with no limit on how many run at once"
        )
    } else {
        format!(
            "`{short}` at line {line} starts one concurrent task per item of a collection, with no limit on how many run at once"
        )
    };
    auditor_core::finding!(
        "unbounded_concurrency",
        "systems_economics",
        "Unbounded concurrency",
        message,
        Severity::Medium,
        Confidence::Medium,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Bound the fan-out: a worker pool or fixed-size executor, a semaphore around each task, `buffer_unordered(n)`, an errgroup with `SetLimit(n)`, `p-limit`, or batches of a fixed size.",
    )
    .with_identity("site", ast_scan::source_summary(site).as_str())
    .with_evidence("api", name)
}

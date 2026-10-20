//! Java callbacks that run on a shared, non-blocking executor, where blocking
//! I/O stalls unrelated work:
//!
//! - a lambda passed to `CompletableFuture.supplyAsync`/`runAsync` without an
//!   executor runs on the JVM's common fork-join pool;
//! - a lambda passed to an operator of a Reactor (`Mono`, `Flux`) or RxJava
//!   chain runs on the thread that emits, typically an event loop
//!   (BlockHound's concern), unless the chain moves work with `subscribeOn`
//!   or `publishOn`.
//!
//! A reactive chain must resolve to a library type at its root (a declared
//! `Mono<T>` value or a `Mono.just(..)` factory). A chain rooted in a call
//! the resolver cannot type, or one that schedules work anywhere along it,
//! is not claimed: the caller may still subscribe it on a blocking scheduler.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::java_api_identity::JavaScope;
use crate::java_syntax::{java_arguments, variable_declaration};

const REACTIVE_TYPES: &[&str] = &[
    "reactor.core.publisher.Mono",
    "reactor.core.publisher.Flux",
    "io.reactivex.rxjava3.core.Observable",
    "io.reactivex.rxjava3.core.Flowable",
    "io.reactivex.rxjava3.core.Single",
    "io.reactivex.rxjava3.core.Maybe",
    "io.reactivex.rxjava3.core.Completable",
    "io.reactivex.Observable",
    "io.reactivex.Flowable",
    "io.reactivex.Single",
    "io.reactivex.Maybe",
];

const SCHEDULING_OPERATORS: &[&str] = &["subscribeOn", "publishOn", "observeOn"];

/// The lambdas in `root` that run on a shared asynchronous executor.
pub(crate) fn async_callbacks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = JavaScope::new(root);
    ast_scan::nodes_of_kind(root, &["method_invocation"])
        .into_iter()
        .filter(|call| default_pool_task(&scope, *call) || reactive_operator(&scope, *call))
        .flat_map(java_arguments)
        .filter(|argument| argument.kind() == "lambda_expression")
        .collect()
}

/// `CompletableFuture.supplyAsync(task)` or `runAsync(task)` with no executor.
fn default_pool_task(scope: &JavaScope, call: AstNode<'_>) -> bool {
    matches!(
        scope.resolve_call(call).as_deref(),
        Some(
            "java.util.concurrent.CompletableFuture.supplyAsync"
                | "java.util.concurrent.CompletableFuture.runAsync"
        )
    ) && java_arguments(call).len() == 1
}

/// An operator call on a reactive chain with a resolved library root and no
/// scheduling operator anywhere along the chain.
fn reactive_operator(scope: &JavaScope, call: AstNode<'_>) -> bool {
    let Some(mut receiver) = call.child_by_field_name("object") else {
        return false;
    };
    let mut methods = Vec::new();
    while receiver.kind() == "method_invocation" {
        let Some(object) = receiver.child_by_field_name("object") else {
            break;
        };
        // The innermost call is the chain's source (`Mono.just(..)`).
        if object.kind() != "method_invocation" && is_reactive_factory(scope, receiver, object) {
            break;
        }
        methods.extend(receiver.child_by_field_name("name").map(|name| name.text()));
        receiver = object;
    }
    let rooted = if receiver.kind() == "method_invocation" {
        receiver
            .child_by_field_name("object")
            .is_some_and(|object| is_reactive_factory(scope, receiver, object))
    } else {
        // A bare type name is a static factory call (`Mono.fromCallable(..)`),
        // whose argument is not an operator callback.
        (receiver.kind() != "identifier" || variable_declaration(receiver).is_some())
            && scope
                .expression_type(receiver)
                .is_some_and(|kind| REACTIVE_TYPES.contains(&kind.as_str()))
    };
    if !rooted {
        return false;
    }
    let mut chain = call;
    while let Some(parent) = chain
        .parent()
        .filter(|parent| parent.kind() == "method_invocation")
        .filter(|parent| {
            parent
                .child_by_field_name("object")
                .is_some_and(|object| object.start_byte() == chain.start_byte())
        })
    {
        methods.extend(parent.child_by_field_name("name").map(|name| name.text()));
        chain = parent;
    }
    methods.extend(call.child_by_field_name("name").map(|name| name.text()));
    !methods
        .iter()
        .any(|method| SCHEDULING_OPERATORS.contains(method))
}

/// A static factory on a reactive type (`Mono.just(..)`, `Flux.fromIterable`).
fn is_reactive_factory(scope: &JavaScope, call: AstNode<'_>, object: AstNode<'_>) -> bool {
    object.kind() == "identifier"
        && scope
            .type_named(object.text())
            .is_some_and(|kind| REACTIVE_TYPES.contains(&kind.as_str()))
        && call.child_by_field_name("name").is_some()
}

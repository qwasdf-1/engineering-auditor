//! `unbounded_channel_or_queue`: a channel or work queue created without a
//! capacity. Nothing slows a producer that outpaces its consumers, so the
//! backlog grows in memory until the process runs out, and latency grows
//! with it; a bounded one blocks or rejects the producer instead
//! (backpressure).
//!
//! Constructors are matched by their resolved API identity:
//! - Rust: `std::sync::mpsc::channel` (`sync_channel(n)` is bounded), tokio
//!   `mpsc::unbounded_channel`, and the `unbounded` constructors of
//!   crossbeam, flume, async-channel, and `futures::channel::mpsc`.
//! - Python: `queue.Queue`/`LifoQueue`/`PriorityQueue`, `asyncio.Queue`
//!   variants, and `multiprocessing.Queue` without a positive `maxsize`, and
//!   `queue.SimpleQueue`, which has none.
//! - Java: `LinkedBlockingQueue`/`LinkedBlockingDeque` without a capacity,
//!   `LinkedTransferQueue`, `PriorityBlockingQueue`, and
//!   `Executors.newFixedThreadPool`/`newSingleThreadExecutor`, whose work
//!   queue is an unbounded `LinkedBlockingQueue`.
//!
//! Go channels always have a fixed capacity, and JavaScript has no channel
//! or blocking queue type.

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::java_api_identity::JavaScope;
use crate::python_api_identity::{keyword_argument, positional_arguments, PyScope};
use crate::rust_api_identity::{rust_arguments, RustScope};
use crate::Language;

fn supported(language: &Language) -> bool {
    matches!(language, Language::Rust | Language::Python | Language::Java)
}

const RUST_UNBOUNDED: &[&str] = &[
    "std::sync::mpsc::channel",
    "tokio::sync::mpsc::unbounded_channel",
    "crossbeam_channel::unbounded",
    "crossbeam::channel::unbounded",
    "flume::unbounded",
    "async_channel::unbounded",
    "futures::channel::mpsc::unbounded",
];

/// Python queues bounded by a positive `maxsize`, their first parameter.
const PYTHON_SIZED: &[&str] = &[
    "queue.Queue",
    "queue.LifoQueue",
    "queue.PriorityQueue",
    "asyncio.Queue",
    "asyncio.LifoQueue",
    "asyncio.PriorityQueue",
    "asyncio.queues.Queue",
    "multiprocessing.Queue",
    "multiprocessing.JoinableQueue",
];

/// Java constructors bounded by a capacity argument.
const JAVA_SIZED: &[&str] = &[
    "java.util.concurrent.LinkedBlockingQueue.<init>",
    "java.util.concurrent.LinkedBlockingDeque.<init>",
];

const JAVA_UNBOUNDED: &[&str] = &[
    "java.util.concurrent.LinkedTransferQueue.<init>",
    "java.util.concurrent.PriorityBlockingQueue.<init>",
    "java.util.concurrent.Executors.newFixedThreadPool",
    "java.util.concurrent.Executors.newSingleThreadExecutor",
];

pub(crate) fn unbounded_channel_or_queue_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    ast_scan::site_findings(scan, supported, queue_sites, finding)
}

fn queue_sites<'a>(language: &Language, root: AstNode<'a>) -> Vec<(AstNode<'a>, String)> {
    let sites = match language {
        Language::Rust => rust_sites(root),
        Language::Python => python_sites(root),
        _ => java_sites(root),
    };
    sites.into_iter().map(|(api, call)| (call, api)).collect()
}

fn rust_sites(root: AstNode<'_>) -> Vec<(String, AstNode<'_>)> {
    RustScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            RUST_UNBOUNDED.contains(&api.as_str())
                && rust_arguments(*call).is_empty()
                && !ast_scan::is_inside_rust_test(*call)
        })
        .collect()
}

fn python_sites(root: AstNode<'_>) -> Vec<(String, AstNode<'_>)> {
    PyScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            if api == "queue.SimpleQueue" {
                return true;
            }
            if !PYTHON_SIZED.contains(&api.as_str()) {
                return false;
            }
            let size = keyword_argument(*call, "maxsize")
                .or_else(|| positional_arguments(*call).first().copied());
            // `**options` may carry the size; a name may hold a positive one.
            let splat = call
                .child_by_field_name("arguments")
                .is_some_and(|arguments| arguments.text().contains("**"));
            !splat && size.is_none_or(|size| non_positive_literal(size.text()))
        })
        .collect()
}

fn java_sites(root: AstNode<'_>) -> Vec<(String, AstNode<'_>)> {
    JavaScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            JAVA_UNBOUNDED.contains(&api.as_str())
                || (JAVA_SIZED.contains(&api.as_str())
                    && call
                        .child_by_field_name("arguments")
                        .is_some_and(|arguments| arguments.named_child_count() == 0))
        })
        .collect()
}

/// A literal size of zero or less, which Python queues read as unbounded.
fn non_positive_literal(text: &str) -> bool {
    text.trim().parse::<i64>().is_ok_and(|size| size <= 0)
}

fn finding(call: AstNode<'_>, api: &str, relative: &str) -> Finding {
    let line = call.start_position().0 + 1;
    let name = api.trim_end_matches(".<init>");
    let short = name.rsplit(['.', ':']).next().unwrap_or(name);
    auditor_core::finding!(
        "unbounded_channel_or_queue",
        "systems_economics",
        "Unbounded channel or queue",
        format!(
            "`{short}` at line {line} creates a channel or queue with no capacity, so a producer that outpaces its consumers grows the backlog in memory without limit"
        ),
        Severity::Medium,
        Confidence::High,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Give the channel or queue a capacity sized to the expected backlog (`sync_channel(n)`, `mpsc::channel(n)`, `Queue(maxsize=n)`, `new LinkedBlockingQueue<>(n)`, a `ThreadPoolExecutor` with a bounded queue), so producers block or are rejected when consumers fall behind.",
    )
    .with_identity("site", ast_scan::source_summary(call.parent().unwrap_or(call)).as_str())
    .with_evidence("api", name)
}

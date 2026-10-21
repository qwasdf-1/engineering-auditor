//! `mutable_default_argument`: a Python parameter whose default is a mutable
//! value (`[]`, `{}`, `set()`), evaluated once when the function is defined
//! and shared by every call that omits the argument, which the function then
//! mutates, stores, or returns, so one call's changes reach the next.
//! Python is the only parsed language that evaluates defaults once.

use auditor_ast::{AstNode, BindingKind, FunctionFlow};
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, nodes_of_kind, AstScan};
use crate::{util, Language};

const MUTABLE_LITERALS: &[&str] = &[
    "list",
    "dictionary",
    "set",
    "list_comprehension",
    "dictionary_comprehension",
    "set_comprehension",
];

const MUTABLE_CONSTRUCTORS: &[&str] = &[
    "list",
    "dict",
    "set",
    "bytearray",
    "defaultdict",
    "deque",
    "OrderedDict",
    "Counter",
    "collections.defaultdict",
    "collections.deque",
    "collections.OrderedDict",
    "collections.Counter",
];

/// Methods that change a list, dict, set, deque, or bytearray in place.
const MUTATORS: &[&str] = &[
    "append",
    "appendleft",
    "extend",
    "extendleft",
    "insert",
    "pop",
    "popleft",
    "popitem",
    "remove",
    "clear",
    "sort",
    "reverse",
    "rotate",
    "update",
    "setdefault",
    "add",
    "discard",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
];

/// How the shared default leaves the call that received it.
enum Hazard {
    Mutated(String),
    Stored,
    Returned,
}

/// A shared default and the use through which it leaves the call.
struct SharedDefault<'a> {
    function: &'a str,
    parameter: &'a str,
    default: AstNode<'a>,
    target: AstNode<'a>,
    hazard: Hazard,
    at: AstNode<'a>,
}

fn is_python(language: &Language) -> bool {
    *language == Language::Python
}

pub(crate) fn mutable_default_argument_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, is_python, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        findings.extend(
            nodes_of_kind(tree.root_node(), &["function_definition"])
                .into_iter()
                .flat_map(shared_defaults)
                .map(|shared| finding(&shared, &relative)),
        );
    });
    findings
}

fn shared_defaults(function: AstNode<'_>) -> Vec<SharedDefault<'_>> {
    let Some(name) = function.child_by_field_name("name").map(|name| name.text()) else {
        return Vec::new();
    };
    let flow = FunctionFlow::of(function);
    flow.bindings()
        .iter()
        .filter(|binding| {
            binding.kind == BindingKind::Parameter
                && binding.node.parent() == function.child_by_field_name("parameters")
                && !deliberate(binding.name)
        })
        .filter_map(|binding| {
            let default = binding.value.filter(|value| mutable(*value))?;
            let (hazard, at) = hazard(&flow, binding)?;
            Some(SharedDefault {
                function: name,
                parameter: binding.name,
                default,
                target: binding.target,
                hazard,
                at,
            })
        })
        .collect()
}

/// A private or cache-named parameter (`_cache={}`, `memo={}`): the shared
/// default is the point, a memo table that outlives each call.
fn deliberate(parameter: &str) -> bool {
    let lower = parameter.to_ascii_lowercase();
    parameter.starts_with('_') || lower.contains("cache") || lower.contains("memo")
}

fn mutable(value: AstNode<'_>) -> bool {
    MUTABLE_LITERALS.contains(&value.kind())
        || (value.kind() == "call"
            && value
                .child_by_field_name("function")
                .is_some_and(|callee| MUTABLE_CONSTRUCTORS.contains(&callee.text())))
}

/// The first use, in source order, through which the default leaves the
/// call: a read the parameter binding reaches, or an in-place `+=`/`|=`.
fn hazard<'a>(
    flow: &FunctionFlow<'a>,
    parameter: &auditor_ast::Binding<'a>,
) -> Option<(Hazard, AstNode<'a>)> {
    let reads = flow
        .reads_of(parameter)
        .into_iter()
        .filter_map(|read| read_hazard(read).map(|hazard| (hazard, read)));
    let augmented = flow
        .bindings()
        .iter()
        .filter(|binding| binding.kind == BindingKind::Augmented && binding.name == parameter.name)
        .filter(|binding| binding.node.kind() == "augmented_assignment")
        .filter(|binding| flow.reaching(binding.target).contains(parameter))
        .map(|binding| {
            let operator = binding
                .node
                .child_by_field_name("operator")
                .map_or("+=", |operator| operator.text());
            (Hazard::Mutated(operator.to_string()), binding.node)
        });
    reads
        .chain(augmented)
        .min_by_key(|(_, node)| node.start_byte())
}

fn read_hazard(read: AstNode<'_>) -> Option<Hazard> {
    let parent = read.parent()?;
    let is_field = |node: AstNode<'_>, field: &str| node.child_by_field_name(field) == Some(read);
    match parent.kind() {
        "attribute" if is_field(parent, "object") => {
            let method = parent.child_by_field_name("attribute")?.text();
            let called = parent.parent().is_some_and(|call| {
                call.kind() == "call" && call.child_by_field_name("function") == Some(parent)
            });
            (called && MUTATORS.contains(&method)).then(|| Hazard::Mutated(method.to_string()))
        }
        "subscript" if is_field(parent, "value") => {
            let holder = parent.parent()?;
            let written = matches!(holder.kind(), "assignment" | "augmented_assignment")
                && holder.child_by_field_name("left") == Some(parent);
            let deleted = holder.kind() == "delete_statement";
            (written || deleted).then(|| Hazard::Mutated("item assignment".to_string()))
        }
        "return_statement" | "yield" => Some(Hazard::Returned),
        "assignment" if is_field(parent, "right") => parent
            .child_by_field_name("left")
            .filter(|left| matches!(left.kind(), "attribute" | "subscript"))
            .map(|_| Hazard::Stored),
        _ => None,
    }
}

fn finding(shared: &SharedDefault<'_>, relative: &str) -> Finding {
    let (function, parameter) = (shared.function, shared.parameter);
    let line = shared.target.start_position().0 + 1;
    let at_line = shared.at.start_position().0 + 1;
    let default_text = shared
        .default
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let (effect, kind) = match &shared.hazard {
        Hazard::Mutated(how) => (format!("mutates it (`{how}`)"), "mutated"),
        Hazard::Stored => ("stores it".to_string(), "stored"),
        Hazard::Returned => ("returns it".to_string(), "returned"),
    };
    auditor_core::finding!(
        "mutable_default_argument",
        "correctness",
        "Mutable default argument",
        format!(
            "Parameter `{parameter}` of `{function}` defaults to `{default_text}`, one object shared by every call, and the function {effect} at line {at_line}"
        ),
        Severity::Medium,
        Confidence::High,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Default to `None` and build the value inside the function (`if items is None: items = []`), so each call gets its own.",
    )
    .with_evidence("function", function)
    .with_evidence("parameter", parameter)
    .with_evidence("default", default_text.as_str())
    .with_evidence("use", kind)
}

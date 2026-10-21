//! C++ forms of the L5 facts: `<cmath>` calls that are pure, `std::regex`
//! construction, `value_or` defaults built eagerly, and the
//! find-then-insert map shape.

use super::collections::call_argument_is_typed;
use super::language_forms::java_map_value_is_pure;
use super::map_entry::MapEntryForm;
use super::syntax::{call_name, compact, strip_call_delimiters};
use super::FunctionView;
use crate::structure_index::StructureCall;

const PURE_NUMERIC_CALLS: &[&str] = &[
    "std::abs",
    "std::fabs",
    "std::floor",
    "std::ceil",
    "std::round",
];
const NUMERIC_TYPES: &[&str] = &["double ", "float ", "int ", "long "];
const CONTAINERS: &[&str] = &[
    "std::vector<",
    "std::string",
    "std::map<",
    "std::unordered_map<",
    "std::set<",
    "std::unordered_set<",
    "std::deque<",
    "std::list<",
];

/// `std::abs(x)` and its rounding siblings on a numeric local or parameter.
pub(super) fn cpp_pure_numeric_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    PURE_NUMERIC_CALLS.contains(&compact(&call.callee).as_str())
        && call_argument_is_typed(view, call, NUMERIC_TYPES)
}

pub(super) fn cpp_regex_construction(call: &StructureCall) -> bool {
    matches!(compact(&call.callee).as_str(), "std::regex" | "std::wregex")
}

/// The default of `opt.value_or(std::vector<int>{})`: an empty container
/// built whether or not the optional holds a value.
pub(super) fn cpp_eager_fallback(call: &StructureCall) -> Option<String> {
    if call_name(call) != "value_or" {
        return None;
    }
    let fallback = strip_call_delimiters(call.arguments.as_deref()?).trim();
    let value = compact(fallback);
    let constructed = value
        .strip_suffix("{}")
        .or_else(|| value.strip_suffix("()"))?;
    CONTAINERS
        .iter()
        .any(|container| constructed.starts_with(container))
        .then(|| fallback.to_string())
}

/// `if (m.find(k) == m.end()) { m[k] = v; }` (or `!m.count(k)`,
/// `m.count(k) == 0`, `!m.contains(k)`, and an `emplace`) on a map, which
/// `try_emplace` does in one lookup.
pub(super) const CPP_MAP_ENTRY: MapEntryForm = MapEntryForm {
    guard: cpp_absence_guard,
    lookups: &["find", "end", "count", "contains"],
    insert: "emplace",
    map_types: &["map<"],
    value_is_pure: java_map_value_is_pure,
};

/// The `(map, key)` a C++ condition tests for absence. The condition clause
/// keeps its parentheses: `(m.find(k) == m.end())`.
fn cpp_absence_guard(condition: &str) -> Option<(&str, &str)> {
    let tested = condition
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(condition);
    if let Some((find, end)) = tested.split_once("==") {
        if let Some((map, key)) = find
            .strip_suffix(')')
            .and_then(|find| find.split_once(".find("))
        {
            return (end == format!("{map}.end()")).then_some((map, key));
        }
    }
    let lookup = tested
        .strip_prefix('!')
        .or_else(|| tested.strip_suffix("==0"))?
        .strip_suffix(')')?;
    lookup
        .split_once(".count(")
        .or_else(|| lookup.split_once(".contains("))
}

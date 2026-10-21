//! The membership-guarded insertion that a map's entry API does in one
//! lookup, in the languages that guard with a lookup call: C#
//! `!map.ContainsKey(k)` before `map[k] = v` or `Add` (`TryAdd`), and C++
//! `map.find(k) == map.end()` before `map[k] = v` or `emplace`
//! (`try_emplace`). The value must be pure, since the entry API evaluates it
//! whether or not it inserts.

use super::collections::declared_with_any_type;
use super::collectors::{push_fact, structural_proof};
use super::flow::{assignments_within, calls_within, map_guards, map_is_touched_between};
use super::syntax::{call_name, call_receiver, compact, split_arguments, strip_call_delimiters};
use super::{FunctionView, OptimizationFact, OptimizationFactKind};
use crate::structure_index::StructureCall;

/// A language's guarded insertion.
pub(super) struct MapEntryForm {
    /// The `(map, key)` a compacted guard tests for absence.
    pub(super) guard: fn(&str) -> Option<(&str, &str)>,
    /// The lookup calls the guard itself makes on the map.
    pub(super) lookups: &'static [&'static str],
    /// The insertion method other than an indexer store.
    pub(super) insert: &'static str,
    pub(super) map_types: &'static [&'static str],
    pub(super) value_is_pure: fn(&str) -> bool,
}

pub(super) fn collect_guarded_map_entries(
    view: &FunctionView<'_>,
    form: &MapEntryForm,
    facts: &mut Vec<OptimizationFact>,
) {
    for (control, map, key) in map_guards(view, form.guard) {
        let (lookups, rest): (Vec<_>, Vec<_>) = calls_within(view, control.span)
            .into_iter()
            .partition(|call| {
                call_receiver(call) == Some(map.as_str())
                    && form.lookups.contains(&call_name(call).as_str())
            });
        // The guard's last lookup (`m.end()` after `m.find(k)`).
        let Some(lookup) = lookups.iter().max_by_key(|call| call.span.end_byte) else {
            continue;
        };
        let Some(insertion) = insertion(view, control.span, &rest, form.insert, (&map, &key))
        else {
            continue;
        };
        // Nothing else runs in the branch: an insertion call is the insertion.
        if rest.iter().any(|call| call.span != insertion.span)
            || !(form.value_is_pure)(&insertion.value)
            || !declared_with_any_type(view, &map, form.map_types)
            || map_is_touched_between(view, control.span, lookup.span, insertion.span, &map)
        {
            continue;
        }
        push_fact(
            view,
            insertion.line,
            OptimizationFactKind::MapEntryOpportunity {
                map,
                key,
                insertion: insertion.expression,
            },
            structural_proof("hash_map", None, "same_inserted_value", "literal_default"),
            facts,
        );
    }
}

struct Insertion {
    line: usize,
    span: auditor_ast::SourceSpan,
    value: String,
    expression: String,
}

/// The one insertion of `key` into `map` inside `scope`: an indexer store
/// or an `insert(key, value)` call.
fn insertion(
    view: &FunctionView<'_>,
    scope: auditor_ast::SourceSpan,
    scoped: &[&StructureCall],
    insert: &str,
    (map, key): (&str, &str),
) -> Option<Insertion> {
    let stores = assignments_within(view, scope);
    let calls = scoped
        .iter()
        .filter(|call| call_name(call) == insert && call_receiver(call) == Some(map))
        .collect::<Vec<_>>();
    match (stores.as_slice(), calls.as_slice()) {
        ([store], []) if compact(&store.target) == format!("{map}[{key}]") => Some(Insertion {
            line: store.line,
            span: store.span,
            value: store.value.clone()?,
            expression: store.expression.clone(),
        }),
        ([], [call]) => {
            let arguments = split_arguments(strip_call_delimiters(call.arguments.as_deref()?));
            let [inserted_key, value] = arguments.as_slice() else {
                return None;
            };
            (compact(inserted_key) == key).then(|| Insertion {
                line: call.line,
                span: call.span,
                value: value.to_string(),
                expression: call.expression.clone(),
            })
        }
        _ => None,
    }
}

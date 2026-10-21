//! Python and Rust loop forms for the L5 facts: Python's membership-guarded
//! insertion (`setdefault`), and Rust's known-size `Vec` growth, `format!`
//! rebuilding a string, and a range driven by `for_each` (Rust) or
//! `forEach` (Java).

use super::collections::*;
use super::collectors::{push_fact, structural_proof};
use super::flow::*;
use super::syntax::*;
use super::{FunctionView, OptimizationFact, OptimizationFactKind};
use crate::Language;

pub(super) fn collect_loop_form_facts(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    match view.file.language {
        Language::Python => collect_python_map_entry(view, facts),
        Language::Rust => {
            collect_rust_known_size_preallocation(view, facts);
            collect_rust_format_accumulation(view, facts);
            collect_range_for_each(view, facts);
        }
        Language::Java => collect_range_for_each(view, facts),
        Language::CSharp => {
            collect_range_for_each(view, facts);
            super::csharp::collect_csharp_map_entry(view, facts);
        }
        _ => {}
    }
}

/// `if key not in groups: groups[key] = []` on a dict, which
/// `groups.setdefault(key, [])` does in one lookup. The default must be a
/// literal or empty constructor, since `setdefault` evaluates it eagerly.
fn collect_python_map_entry(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    let guards = map_guards(view, |condition| {
        condition.split_once("notin").map(|(key, map)| (map, key))
    });
    for (control, map, key) in guards {
        let inside = assignments_within(view, control.span);
        let [insertion] = inside.as_slice() else {
            continue;
        };
        let value = insertion.value.as_deref().map(compact).unwrap_or_default();
        if compact(&insertion.target) != format!("{map}[{key}]")
            || !matches!(
                value.as_str(),
                "[]" | "{}" | "set()" | "list()" | "dict()" | "0" | "0.0" | "\"\"" | "''" | "None"
            )
            || calls(view).any(|call| contains_span(control.span, call.span))
            || !(declared_with_any_type(view, &map, &["dict[", "Dict[", "MutableMapping["])
                || initialized_before(view, &map, control.span.start_byte, &["{}", "dict()"]))
        {
            continue;
        }
        push_fact(
            view,
            insertion.line,
            OptimizationFactKind::MapEntryOpportunity {
                map,
                key,
                insertion: insertion.expression.clone(),
            },
            structural_proof("hash_map", None, "same_inserted_value", "literal_default"),
            facts,
        );
    }
}

fn initialized_before(
    view: &FunctionView<'_>,
    target: &str,
    before: usize,
    values: &[&str],
) -> bool {
    assignments(view)
        .filter(|assignment| assignment.span.end_byte <= before)
        .filter(|assignment| simple_target(&assignment.target).as_deref() == Some(target))
        .last()
        .and_then(|assignment| assignment.value.as_deref())
        .is_some_and(|value| values.contains(&compact(value).as_str()))
}

/// `let mut result = Vec::new(); for item in items { result.push(f(item)) }`
/// over a slice or `Vec` grows one reallocation at a time;
/// `Vec::with_capacity(items.len())` allocates once. Pushing the item itself
/// unchanged is `bulk_collection_extension`'s instead.
fn collect_rust_known_size_preallocation(
    view: &FunctionView<'_>,
    facts: &mut Vec<OptimizationFact>,
) {
    for loop_info in loops(view) {
        let ([binding], Some(collection)) = (
            loop_info.binding.as_slice(),
            loop_info
                .iterable
                .as_deref()
                .and_then(rust_iterated_collection),
        ) else {
            continue;
        };
        if !declared_with_any_type(view, &collection, &["Vec<", "&[", "&mut ["])
            || bulk_extension_candidate(view, loop_info).is_some()
        {
            continue;
        }
        let pushes = calls(view)
            .filter(|call| {
                call_name(call) == "push"
                    && contains_span(loop_info.span, call.span)
                    && !has_nested_control(view.file, call.control_region, loop_info.control_region)
            })
            .collect::<Vec<_>>();
        let [push] = pushes.as_slice() else {
            continue;
        };
        let Some(target) = call_receiver(push).and_then(simple_target) else {
            continue;
        };
        let argument = compact_arguments(push);
        if argument == compact(binding)
            || !loop_has_only_primary_call(view, loop_info, push)
            || !initialized_before(
                view,
                &target,
                loop_info.span.start_byte,
                &["Vec::new()", "vec![]"],
            )
        {
            continue;
        }
        let Some(initializer) = initializer_before(view, loop_info, &target) else {
            continue;
        };
        let size_expression = format!("{collection}.len()");
        push_fact(
            view,
            initializer.line,
            OptimizationFactKind::KnownSizePreallocation {
                collection,
                target,
                size_expression: size_expression.clone(),
            },
            structural_proof(
                "growable_sequence",
                Some(size_expression),
                "same_vec_contents",
                "process_fatal_allocation_only",
            ),
            facts,
        );
    }
}

/// The collection a Rust `for` loop walks element by element: `items`,
/// `&items`, `items.iter()`.
fn rust_iterated_collection(iterable: &str) -> Option<String> {
    let compact = compact(iterable);
    let base = compact
        .strip_suffix(".iter()")
        .or_else(|| compact.strip_suffix(".iter_mut()"))
        .or_else(|| compact.strip_suffix(".into_iter()"))
        .unwrap_or(&compact);
    let base = base
        .strip_prefix("&mut")
        .or_else(|| base.strip_prefix('&'))
        .unwrap_or(base);
    simple_target(base)
}

/// `output = format!("{output}{item}")` in a loop copies the whole string
/// each pass; `push_str` or `write!` appends in place.
fn collect_rust_format_accumulation(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    for assignment in assignments(view) {
        let Some(loop_info) = containing_loop(view.file, view.owner, assignment.control_region)
        else {
            continue;
        };
        let Some(accumulator) = simple_target(&assignment.target) else {
            continue;
        };
        let value = assignment.value.as_deref().map(compact).unwrap_or_default();
        let Some(arguments) = value.strip_prefix("format!(") else {
            continue;
        };
        let mentions = arguments.contains(&format!("{{{accumulator}}}"))
            || split_arguments(arguments.trim_end_matches(')'))
                .iter()
                .skip(1)
                .any(|argument| compact(argument) == accumulator);
        if !mentions
            || has_nested_control(
                view.file,
                assignment.control_region,
                loop_info.control_region,
            )
            || !initialized_before(
                view,
                &accumulator,
                loop_info.span.start_byte,
                &["String::new()", "\"\".to_string()", "String::default()"],
            )
        {
            continue;
        }
        push_fact(
            view,
            assignment.line,
            OptimizationFactKind::StringAccumulation { accumulator },
            structural_proof(
                "owned_string",
                None,
                "same_string_contents",
                "process_fatal_allocation_only",
            ),
            facts,
        );
    }
}

/// `(0..n).for_each(|v| ..)` in Rust (clippy `needless_for_each`) and
/// `IntStream.range(0, n).forEach(i -> ..)` in Java, or
/// `Enumerable.Range(0, n).ToList().ForEach(i => ..)` in C#: a counting `for`
/// loop reads plainly and allows `break`, `?`, and checked exceptions (and
/// builds no list).
fn collect_range_for_each(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    let java = view.file.language == Language::Java;
    let method = match view.file.language {
        Language::Java => "forEach",
        Language::CSharp => "ForEach",
        _ => "for_each",
    };
    for call in calls(view).filter(|&call| call_name(call) == method) {
        let receiver = call.receiver.as_deref().unwrap_or_else(|| {
            call.expression
                .split(&format!(".{method}"))
                .next()
                .unwrap_or_default()
        });
        let receiver = compact(receiver);
        let range = if view.file.language == Language::CSharp {
            receiver.starts_with("Enumerable.Range(") && receiver.ends_with(").ToList()")
        } else if java {
            [
                "IntStream.range(",
                "IntStream.rangeClosed(",
                "LongStream.range(",
                "LongStream.rangeClosed(",
            ]
            .iter()
            .any(|factory| receiver.contains(factory))
                && receiver.ends_with(')')
        } else {
            receiver.starts_with('(') && receiver.ends_with(')') && receiver.contains("..")
        };
        if !range {
            continue;
        }
        push_fact(
            view,
            call.line,
            OptimizationFactKind::RangeForEach {
                range_expression: receiver,
            },
            structural_proof(
                "range",
                None,
                "iteration_side_effects",
                "same_iteration_order",
            ),
            facts,
        );
    }
}

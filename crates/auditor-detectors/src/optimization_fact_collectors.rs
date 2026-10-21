use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::ControlRegionKind;

use super::collections::*;
use super::flow::*;
use super::language_forms::*;
use super::operations::*;
use super::syntax::*;
use super::{FunctionView, OptimizationFact, OptimizationFactKind, OptimizationProof};
use crate::structure_index::StructureCall;
use crate::{util, Language, L5_REPEATED_CALL_THRESHOLD};

pub(super) fn collect_function_facts(
    view: FunctionView<'_>,
    algorithmic_lines: &BTreeSet<(&str, usize)>,
    facts: &mut Vec<OptimizationFact>,
) {
    collect_repeated_calls(&view, facts);
    collect_loop_calls(&view, algorithmic_lines, facts);
    collect_string_accumulation(&view, facts);
    collect_manual_collection_copy(&view, facts);
    super::fallback::collect_eager_fallback(&view, facts);
    collect_intermediate_materialization(&view, facts);
    collect_range_for_each(&view, facts);
    collect_known_size_preallocation(&view, facts);
    collect_bulk_collection_extension(&view, facts);
    collect_map_entry_opportunity(&view, facts);
    super::loop_forms::collect_loop_form_facts(&view, facts);
}

fn collect_repeated_calls(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    let mut groups = BTreeMap::<(String, Option<u32>), Vec<&StructureCall>>::new();
    for call in calls(view).filter(|&call| cacheable_pure_call(view, call)) {
        groups
            .entry((compact(&call.expression), call.control_region))
            .or_default()
            .push(call);
    }
    for ((expression, _), calls) in groups {
        if calls.len() < L5_REPEATED_CALL_THRESHOLD
            || operands_mutated_between(view, &expression, &calls)
        {
            continue;
        }
        let first = calls[0];
        push_fact(
            view,
            first.line,
            OptimizationFactKind::RepeatedCall {
                expression,
                callee: call_name(first),
                count: calls.len(),
            },
            pure_proof(
                "same_control_region",
                "infallible_pure_call",
                receiver_kind(first),
            ),
            facts,
        );
    }
}

fn collect_loop_calls(
    view: &FunctionView<'_>,
    algorithmic_lines: &BTreeSet<(&str, usize)>,
    facts: &mut Vec<OptimizationFact>,
) {
    for call in calls(view) {
        let Some(loop_info) = invariant_loop(view, call) else {
            continue;
        };
        if recognized_regex_call(view, call) {
            if lazily_cached(view, call) {
                continue;
            }
            push_fact(
                view,
                call.line,
                OptimizationFactKind::LoopInvariantRegex {
                    api: call_name(call),
                },
                pure_proof(
                    "lazy_first_use_cache",
                    "preserve_zero_iteration_and_first_failure",
                    "regular_expression",
                ),
                facts,
            );
            continue;
        }
        // Hoisting pays only when the call runs on every iteration; one in a
        // branch (an error message built before `throw`) may never run.
        if format_call(call)
            && format_operands_are_primitive(view, call)
            && !has_nested_control(view.file, call.control_region, loop_info.control_region)
        {
            push_fact(
                view,
                call.line,
                OptimizationFactKind::LoopInvariantFormat {
                    dynamic_identifiers: expression_operands(call),
                },
                pure_proof("hoist_before_loop", "infallible_primitive_format", "string"),
                facts,
            );
            continue;
        }
        if algorithmic_lines.contains(&(view.file.path.as_str(), call.line))
            || !general_invariant_call(view, call)
        {
            continue;
        }
        push_fact(
            view,
            call.line,
            OptimizationFactKind::LoopInvariantComputation {
                expression: call.expression.clone(),
                callee: call_name(call),
                loop_line: loop_info.span.start_line,
            },
            pure_proof(
                "hoist_before_loop",
                "infallible_pure_call",
                receiver_kind(call),
            ),
            facts,
        );
    }
}

fn collect_string_accumulation(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if !matches!(
        view.file.language,
        Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::Kotlin
            | Language::CSharp
    ) {
        return;
    }
    for assignment in assignments(view) {
        let Some(loop_info) = containing_loop(view.file, view.owner, assignment.control_region)
        else {
            continue;
        };
        if !iterable_has_string_elements(view, loop_info) {
            continue;
        }
        let Some(accumulator) = simple_target(&assignment.target) else {
            continue;
        };
        if !string_concatenation(assignment, &accumulator)
            || !initialized_as_string_before(view, &accumulator, loop_info.span.start_byte)
            || has_nested_control(
                view.file,
                assignment.control_region,
                loop_info.control_region,
            )
        {
            continue;
        }
        let Some(value) = assignment.value.as_deref() else {
            continue;
        };
        let allowed = loop_info
            .binding
            .iter()
            .chain(std::iter::once(&accumulator))
            .cloned()
            .collect::<BTreeSet<_>>();
        if util::identifiers(value)
            .iter()
            .any(|identifier| !allowed.contains(identifier) && !is_string_keyword(identifier))
        {
            continue;
        }
        push_fact(
            view,
            assignment.line,
            OptimizationFactKind::StringAccumulation { accumulator },
            pure_proof(
                "preserve_iteration_order",
                "same_string_conversion",
                "immutable_string",
            ),
            facts,
        );
    }
}

fn collect_manual_collection_copy(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if !matches!(
        view.file.language,
        Language::Rust
            | Language::Python
            | Language::Go
            | Language::Java
            | Language::JavaScript
            | Language::TypeScript
            | Language::CSharp
    ) {
        return;
    }
    for loop_info in loops(view) {
        let Some(iterable) = loop_info.iterable.as_deref() else {
            continue;
        };
        let Some((binding, source)) = indexed_copy_bounds(view, loop_info, iterable) else {
            continue;
        };
        let binding = &binding;
        // A Java counting loop's own `index = 0` initializer is not a body write.
        let inside = assignments(view)
            .filter(|assignment| contains_span(loop_info.span, assignment.span))
            .filter(|assignment| simple_target(&assignment.target).as_deref() != Some(binding))
            .collect::<Vec<_>>();
        if inside.len() != 1 {
            continue;
        }
        if calls(view).any(|call| {
            contains_span(loop_info.span, call.span)
                && !loop_info
                    .iterable_span
                    .is_some_and(|span| contains_span(span, call.span))
        }) {
            continue;
        }
        let assignment = inside[0];
        let Some(target) = indexed_target(&assignment.target, binding) else {
            continue;
        };
        let expected = compact(&format!("{source}[{binding}]"));
        if assignment.value.as_deref().map(compact).as_deref() != Some(expected.as_str()) {
            continue;
        }
        push_fact(
            view,
            loop_info.span.start_line,
            OptimizationFactKind::ManualCollectionCopy {
                source,
                target,
                loop_line: loop_info.span.start_line,
            },
            structural_proof("slice", None, "bulk_copy", "same_bounds_failure"),
            facts,
        );
    }
}

fn collect_intermediate_materialization(
    view: &FunctionView<'_>,
    facts: &mut Vec<OptimizationFact>,
) {
    for call in calls(view) {
        let Some(operation) = intermediate_materialization_operation(view, call) else {
            continue;
        };
        push_fact(
            view,
            call.line,
            OptimizationFactKind::IntermediateMaterialization {
                operation: operation.to_string(),
            },
            structural_proof(
                "iterator_or_sequence",
                None,
                "terminal_result",
                "same_callback_order",
            ),
            facts,
        );
    }
}

fn collect_range_for_each(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if view.file.language != Language::Kotlin {
        return;
    }
    for call in calls(view).filter(|&call| call_name(call) == "forEach") {
        let receiver = call
            .receiver
            .as_deref()
            .unwrap_or_else(|| call.expression.split(".forEach").next().unwrap_or_default());
        if !["..", "until", "downTo", "rangeTo"]
            .iter()
            .any(|marker| receiver.contains(marker))
        {
            continue;
        }
        push_fact(
            view,
            call.line,
            OptimizationFactKind::RangeForEach {
                range_expression: receiver.to_string(),
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

fn collect_known_size_preallocation(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if matches!(view.file.language, Language::Java | Language::CSharp) {
        collect_list_known_size_preallocation(view, facts);
        return;
    }
    if view.file.language != Language::Go {
        return;
    }
    for loop_info in loops(view) {
        let Some(collection) = loop_info.iterable.as_deref().and_then(simple_target) else {
            continue;
        };
        let appends = calls(view)
            .filter(|&call| {
                call_name(call) == "append"
                    && contains_span(loop_info.span, call.span)
                    && !has_nested_control(view.file, call.control_region, loop_info.control_region)
            })
            .collect::<Vec<_>>();
        if appends.len() != 1 {
            continue;
        }
        let Some(arguments) = appends[0].arguments.as_deref() else {
            continue;
        };
        let parts = split_arguments(strip_call_delimiters(arguments));
        let Some(target) = parts.first().and_then(|part| simple_target(part)) else {
            continue;
        };
        if !loop_is_single_append(view, loop_info, appends[0], &target) {
            continue;
        }
        let Some(initializer) = initializer_before(view, loop_info, &target) else {
            continue;
        };
        if !initializer_without_capacity(initializer) {
            continue;
        }
        let size_expression = format!("len({collection})");
        push_fact(
            view,
            initializer.line,
            OptimizationFactKind::KnownSizePreallocation {
                collection: collection.clone(),
                target,
                size_expression: size_expression.clone(),
            },
            structural_proof(
                "growable_sequence",
                Some(size_expression),
                "same_slice_contents",
                "process_fatal_allocation_only",
            ),
            facts,
        );
    }
}

fn collect_bulk_collection_extension(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if !matches!(
        view.file.language,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::CSharp
    ) {
        return;
    }
    for loop_info in loops(view) {
        let Some(candidate) = bulk_extension_candidate(view, loop_info) else {
            continue;
        };
        push_fact(
            view,
            candidate.call.line,
            OptimizationFactKind::BulkCollectionExtension {
                source: candidate.source,
                target: candidate.target,
                operation: candidate.operation.to_string(),
                loop_line: loop_info.span.start_line,
            },
            structural_proof(
                "growable_sequence",
                None,
                "same_element_order",
                "same_element_evaluation",
            ),
            facts,
        );
    }
}

fn collect_map_entry_opportunity(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    let java = view.file.language == Language::Java;
    if view.file.language != Language::Rust && !java {
        return;
    }
    // Java spells the entry API `putIfAbsent`/`computeIfAbsent`.
    let api = if java {
        MapEntryApi {
            java,
            contains: "containsKey",
            insert: "put",
            map_types: &["Map<"],
        }
    } else {
        MapEntryApi {
            java,
            contains: "contains_key",
            insert: "insert",
            map_types: &["HashMap", "BTreeMap"],
        }
    };
    for control in view.file.controls.iter().filter(|control| {
        control.owner == Some(view.owner)
            && control.kind == ControlRegionKind::Conditional
            && control.condition.as_deref().is_some_and(|condition| {
                condition.contains('!') && condition.contains(api.contains)
            })
    }) {
        if let Some((line, kind)) = map_entry_fact(view, control, &api) {
            push_fact(
                view,
                line,
                kind,
                structural_proof(
                    "hash_map",
                    None,
                    "same_inserted_value",
                    "lazy_value_closure",
                ),
                facts,
            );
        }
    }
}

/// A language's map membership and insertion methods.
struct MapEntryApi {
    java: bool,
    contains: &'static str,
    insert: &'static str,
    map_types: &'static [&'static str],
}

/// `if !map.contains_key(k) { map.insert(k, pure) }` in `control`: the same
/// map and key, a pure value, and no other touch of the map between them.
fn map_entry_fact(
    view: &FunctionView<'_>,
    control: &crate::structure_index::StructureControl,
    api: &MapEntryApi,
) -> Option<(usize, OptimizationFactKind)> {
    let scoped_calls = calls(view)
        .filter(|&call| contains_span(control.span, call.span))
        .collect::<Vec<_>>();
    let named = |name: &str| {
        scoped_calls
            .iter()
            .copied()
            .find(|call| call_name(call) == name)
    };
    let (contains_call, insert_call) = (named(api.contains)?, named(api.insert)?);
    let map = call_receiver(contains_call).and_then(simple_target)?;
    let same_map = call_receiver(insert_call)
        .and_then(simple_target)
        .as_deref()
        == Some(&map);
    if !same_map || !declared_with_any_type(view, &map, api.map_types) {
        return None;
    }
    let contains_args = contains_call
        .arguments
        .as_deref()
        .map(strip_call_delimiters)
        .unwrap_or_default();
    let insert_args = insert_call
        .arguments
        .as_deref()
        .map(strip_call_delimiters)
        .map(split_arguments)
        .unwrap_or_default();
    let (insert_key, insert_value) = (insert_args.first()?, insert_args.get(1)?);
    let key = compact(contains_args).trim_start_matches('&').to_string();
    let pure = if api.java {
        java_map_value_is_pure(insert_value)
    } else {
        rust_map_value_is_pure(insert_value)
    };
    if compact(insert_key) != key
        || !pure
        || map_is_touched_between(
            view,
            control.span,
            contains_call.span,
            insert_call.span,
            &map,
        )
    {
        return None;
    }
    Some((
        insert_call.line,
        OptimizationFactKind::MapEntryOpportunity {
            map,
            key,
            insertion: insert_call.expression.clone(),
        },
    ))
}

pub(super) fn push_fact(
    view: &FunctionView<'_>,
    line: usize,
    kind: OptimizationFactKind,
    proof: OptimizationProof,
    facts: &mut Vec<OptimizationFact>,
) {
    facts.push(OptimizationFact {
        path: view.file.path.clone(),
        function: view.function.name.clone(),
        line,
        kind,
        proof,
        via: None,
    });
}

pub(super) fn pure_proof(
    evaluation_order: &'static str,
    failure_behavior: &'static str,
    receiver_kind: &'static str,
) -> OptimizationProof {
    OptimizationProof {
        invariant_operands: true,
        purity: "known_pure_operation",
        no_intervening_mutation: true,
        evaluation_order,
        failure_behavior,
        receiver_kind,
        known_size: None,
        result_use: "unchanged",
    }
}

pub(super) fn structural_proof(
    receiver_kind: &'static str,
    known_size: Option<String>,
    result_use: &'static str,
    failure_behavior: &'static str,
) -> OptimizationProof {
    OptimizationProof {
        invariant_operands: true,
        purity: "structural_equivalence",
        no_intervening_mutation: true,
        evaluation_order: "source_order_preserved",
        failure_behavior,
        receiver_kind,
        known_size,
        result_use,
    }
}

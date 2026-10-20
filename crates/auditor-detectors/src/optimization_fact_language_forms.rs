//! Per-language forms of the L5 facts: the Java APIs and shapes, and the
//! indexed-copy loop bounds of Rust, Go, and Java.

use super::collections::{call_argument_is_typed, declared_with_any_type, JAVA_COLLECTION_TYPES};
use super::collectors::{push_fact, structural_proof};
use super::flow::{assignments, calls, has_nested_control, initializer_before, loops};
use super::operations::indexed_len_source;
use super::syntax::{
    call_name, call_receiver, compact, compact_arguments, contains_span, simple_target,
    strip_call_delimiters,
};
use super::{FunctionView, OptimizationFact, OptimizationFactKind};
use crate::structure_index::{StructureCall, StructureLoop};
use crate::Language;

/// `(index, source)` of an indexed copy loop: Go ranges a slice by index
/// (`for i := range source`), Rust indexes `0..source.len()`, and Java and
/// JS/TS count `i < source.length` over an array (PMD `AvoidArrayLoops`).
pub(super) fn indexed_copy_bounds(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    iterable: &str,
) -> Option<(String, String)> {
    let array = |source: &String| declared_with_any_type(view, source, &["[]"]);
    match (&view.file.language, loop_info.binding.as_slice()) {
        (Language::Java | Language::JavaScript | Language::TypeScript, []) => {
            counting_index_bound(iterable).filter(|(_, source)| array(source))
        }
        (Language::Go, [binding]) => simple_target(iterable)
            .filter(array)
            .map(|source| (binding.clone(), source)),
        (Language::Rust, [binding]) => {
            indexed_len_source(iterable).map(|source| (binding.clone(), source))
        }
        // `for index in range(len(source))` over a list.
        (Language::Python, [binding]) => compact(iterable)
            .strip_prefix("range(len(")
            .and_then(|rest| rest.strip_suffix("))"))
            .and_then(simple_target)
            .filter(|source| declared_with_any_type(view, source, &["list["]))
            .map(|source| (binding.clone(), source)),
        _ => None,
    }
}

/// A JS/TS `filter` whose result is only tested for emptiness
/// (`xs.filter(p).length > 0`, which `some` replaces) or read at index 0
/// (`find`). The predicate must be an arrow function with no nested call, so
/// evaluating it fewer times changes nothing.
pub(super) fn js_filter_materialization(call: &StructureCall) -> Option<&'static str> {
    if call_name(call) != "filter" {
        return None;
    }
    let predicate = strip_call_delimiters(call.arguments.as_deref()?).trim();
    let body = predicate.split_once("=>")?.1;
    if body.contains('(') {
        return None;
    }
    let consumer = call.consumer.as_deref()?;
    let expression = compact(&call.expression);
    let rest = consumer
        .strip_prefix(&expression)
        .or_else(|| consumer.strip_prefix('!')?.strip_prefix(&expression))?;
    if rest == "[0]" {
        return Some("filter_then_first");
    }
    matches!(
        rest,
        // A bare `.length` is a count (`const spaces = ..length`), not a test.
        ".length>0" | ".length!==0" | ".length!=0" | ".length===0" | ".length==0"
    )
    .then_some("filter_then_existence")
}

/// A Java stream collected to a list only to count it (`count()`).
pub(super) fn java_collects_only_to_count(expression: &str) -> bool {
    expression.contains(".stream()")
        && [
            ".collect(Collectors.toList()).size()",
            ".collect(toList()).size()",
            ".toList().size()",
        ]
        .iter()
        .any(|suffix| expression.ends_with(suffix))
}

/// `Math.abs(x)` and its kin on a numeric local or parameter.
pub(super) fn java_pure_numeric_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    call.api
        .as_deref()
        .is_some_and(|api| JAVA_PURE_NUMERIC_APIS.contains(&api))
        && call_argument_is_typed(view, call, JAVA_NUMERIC_TYPES)
}

/// A case conversion or trim of a value the resolver proved is a `String`.
pub(super) fn java_pure_string_call(call: &StructureCall) -> bool {
    call.api
        .as_deref()
        .is_some_and(|api| JAVA_PURE_STRING_APIS.contains(&api))
}

/// Java library methods that are pure in their argument or receiver.
const JAVA_PURE_NUMERIC_APIS: &[&str] = &[
    "java.lang.Math.abs",
    "java.lang.Math.floor",
    "java.lang.Math.ceil",
    "java.lang.Math.round",
];
const JAVA_PURE_STRING_APIS: &[&str] = &[
    "java.lang.String.toLowerCase",
    "java.lang.String.toUpperCase",
    "java.lang.String.trim",
];
const JAVA_NUMERIC_TYPES: &[&str] = &["double ", "float ", "int ", "long "];

/// A Java `orElse` fallback that only allocates an empty JDK container:
/// building it lazily with `orElseGet` cannot change behavior.
pub(super) fn java_pure_eager_fallback(fallback: &str) -> bool {
    let compact = compact(fallback);
    [
        "newArrayList<",
        "newLinkedList<",
        "newHashMap<",
        "newLinkedHashMap<",
        "newTreeMap<",
        "newHashSet<",
        "newLinkedHashSet<",
        "newTreeSet<",
        "newStringBuilder(",
    ]
    .iter()
    .any(|prefix| compact.starts_with(prefix))
        && compact.ends_with("()")
}

/// A Java map value with no effect: a literal or a string literal's length.
pub(super) fn java_map_value_is_pure(expression: &str) -> bool {
    let value = compact(expression);
    value.parse::<i128>().is_ok()
        || matches!(value.as_str(), "true" | "false")
        || (value.starts_with('"') && (value.ends_with('"') || value.ends_with("\".length()")))
}

/// `(index, array)` of a counting loop condition `index < array.length`
/// (Java, JavaScript, TypeScript).
pub(super) fn counting_index_bound(condition: &str) -> Option<(String, String)> {
    let compact = compact(condition);
    let (index, bound) = compact.split_once('<')?;
    let array = bound.strip_suffix(".length")?;
    Some((simple_target(index)?, simple_target(array)?))
}

/// `List<T> result = new ArrayList<>(); for (x : items) result.add(f(x));`
/// grows a list to `items.size()` elements one reallocation at a time. Adding
/// the element itself unchanged is `bulk_collection_extension`'s instead.
pub(super) fn collect_java_known_size_preallocation(
    view: &FunctionView<'_>,
    facts: &mut Vec<OptimizationFact>,
) {
    for loop_info in loops(view) {
        let ([binding], Some(collection)) = (
            loop_info.binding.as_slice(),
            loop_info.iterable.as_deref().and_then(simple_target),
        ) else {
            continue;
        };
        let size_expression = if declared_with_any_type(view, &collection, JAVA_COLLECTION_TYPES) {
            format!("{collection}.size()")
        } else if declared_with_any_type(view, &collection, &["[]"]) {
            format!("{collection}.length")
        } else {
            continue;
        };
        let inside = calls(view)
            .filter(|call| contains_span(loop_info.span, call.span))
            .collect::<Vec<_>>();
        let Some(add) = inside.iter().copied().find(|call| {
            call_name(call) == "add"
                && !has_nested_control(view.file, call.control_region, loop_info.control_region)
        }) else {
            continue;
        };
        let Some(target) = call_receiver(add).and_then(simple_target) else {
            continue;
        };
        let argument = compact_arguments(add);
        if argument == compact(binding)
            || inside
                .iter()
                .any(|call| !contains_span(add.span, call.span))
            || assignments(view).any(|assignment| contains_span(loop_info.span, assignment.span))
        {
            continue;
        }
        let Some(initializer) = initializer_before(view, loop_info, &target) else {
            continue;
        };
        let value = initializer
            .value
            .as_deref()
            .map(compact)
            .unwrap_or_default();
        if !(value.starts_with("newArrayList<") && value.ends_with(">()")) {
            continue;
        }
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
                "same_list_contents",
                "process_fatal_allocation_only",
            ),
            facts,
        );
    }
}

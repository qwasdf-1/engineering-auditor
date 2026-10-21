use super::flow::{
    assignments, calls, has_nested_control, loop_has_only_primary_call, loop_is_single_append,
};
use super::syntax::{
    call_name, call_receiver, compact, contains_identifier, contains_span, simple_target,
    split_arguments, strip_call_delimiters,
};
use super::FunctionView;
use crate::structure_index::{StructureAssignment, StructureCall, StructureLoop};
use crate::Language;

pub(super) struct BulkExtensionCandidate<'a> {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) operation: &'static str,
    pub(super) call: &'a StructureCall,
}

pub(super) fn string_concatenation(assignment: &StructureAssignment, accumulator: &str) -> bool {
    let expression = compact(&assignment.expression);
    (expression.contains("+=") || expression.contains('+'))
        && contains_identifier(&expression, accumulator)
}

pub(super) fn initialized_as_string_before(
    view: &FunctionView<'_>,
    accumulator: &str,
    before_byte: usize,
) -> bool {
    assignments(view)
        .filter(|assignment| assignment.span.end_byte <= before_byte)
        .filter(|assignment| simple_target(&assignment.target).as_deref() == Some(accumulator))
        .last()
        .and_then(|assignment| assignment.value.as_deref())
        .is_some_and(|value| {
            let compact = compact(value);
            matches!(
                compact.as_str(),
                "\"\"" | "''" | "String::new()" | "String()" | "string.Empty" | "String.Empty"
            )
        })
}

pub(super) fn is_string_keyword(identifier: &str) -> bool {
    matches!(identifier, "str" | "String" | "toString" | "to_string")
}

pub(super) fn spread_source_is_typed_collection(view: &FunctionView<'_>, expression: &str) -> bool {
    let Some(before_conversion) = expression.split(".toTypedArray()").next() else {
        return false;
    };
    let source = before_conversion
        .rsplit(['(', ',', '*'])
        .next()
        .and_then(simple_target);
    source.is_some_and(|source| {
        declared_with_any_type(view, &source, &["List<", "Collection<", "Iterable<"])
    })
}

pub(super) fn iterable_has_string_elements(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
) -> bool {
    let Some(iterable) = loop_info.iterable.as_deref().and_then(simple_target) else {
        return false;
    };
    let markers: &[&str] = match view.file.language {
        Language::Python => &["list[str]", "tuple[str"],
        Language::JavaScript | Language::TypeScript => &["string[]", "Array<string>"],
        Language::Go => &["[]string"],
        Language::Java | Language::Kotlin => &["List<String>", "Collection<String>"],
        Language::CSharp => super::csharp::CSHARP_STRING_SEQUENCES,
        _ => return false,
    };
    declared_with_any_type(view, &iterable, markers)
}

pub(super) fn bulk_sequence_types_are_proven(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    source: &str,
    target: &str,
) -> bool {
    let before = loop_info.span.start_byte;
    match view.file.language {
        Language::Rust => {
            declared_with_any_type(view, source, &["Vec<", "&[", "slice"])
                && declared_with_any_type(view, target, &["Vec<"])
        }
        Language::Python => {
            declared_with_any_type(view, source, &["list[", "List["])
                && initialized_with(view, target, before, |value| compact(value) == "[]")
        }
        Language::JavaScript => {
            initialized_with(view, source, before, is_array_literal)
                && initialized_with(view, target, before, |value| compact(value) == "[]")
        }
        Language::TypeScript => {
            (declared_with_any_type(view, source, &["[]", "Array<"])
                || initialized_with(view, source, before, is_array_literal))
                && (declared_with_any_type(view, target, &["[]", "Array<"])
                    || initialized_with(view, target, before, |value| compact(value) == "[]"))
        }
        Language::Java => {
            declared_with_any_type(view, source, JAVA_COLLECTION_TYPES)
                && declared_with_any_type(view, target, JAVA_COLLECTION_TYPES)
        }
        // `AddRange` takes any sequence, into a `List<T>` (not an `IList<T>`).
        Language::CSharp => {
            (declared_with_any_type(view, source, super::csharp::CSHARP_COLLECTION_TYPES)
                || declared_with_any_type(view, source, &["IEnumerable<", "[]"]))
                && super::csharp::declared_as_list(view, target)
        }
        _ => false,
    }
}

/// Declared Java collection types whose `addAll` takes another collection.
pub(super) const JAVA_COLLECTION_TYPES: &[&str] =
    &["List<", "Collection<", "Set<", "ArrayList<", "LinkedList<"];

pub(super) fn declared_with_any_type(
    view: &FunctionView<'_>,
    name: &str,
    markers: &[&str],
) -> bool {
    view.file.symbols.iter().any(|symbol| {
        symbol.parent == Some(view.owner)
            && symbol.name == name
            && symbol.declaration.as_deref().is_some_and(|declaration| {
                markers.iter().any(|marker| declaration.contains(marker))
            })
    })
}

pub(super) fn call_argument_is_typed(
    view: &FunctionView<'_>,
    call: &StructureCall,
    markers: &[&str],
) -> bool {
    call.arguments
        .as_deref()
        .map(strip_call_delimiters)
        .and_then(simple_target)
        .is_some_and(|argument| declared_with_any_type(view, &argument, markers))
}

pub(super) fn bulk_extension_candidate<'a>(
    view: &FunctionView<'a>,
    loop_info: &'a StructureLoop,
) -> Option<BulkExtensionCandidate<'a>> {
    if view.file.language == Language::Go {
        return go_bulk_extension_candidate(view, loop_info);
    }
    let [binding] = loop_info.binding.as_slice() else {
        return None;
    };
    let source = loop_info.iterable.as_deref().and_then(simple_target)?;
    let candidates = calls(view)
        .filter(|call| {
            contains_span(loop_info.span, call.span)
                && matches!(call_name(call).as_str(), "push" | "append" | "add" | "Add")
                && !has_nested_control(view.file, call.control_region, loop_info.control_region)
        })
        .collect::<Vec<_>>();
    let &[call] = candidates.as_slice() else {
        return None;
    };
    let target = call_receiver(call).and_then(simple_target)?;
    (source != target
        && bulk_sequence_types_are_proven(view, loop_info, &source, &target)
        && loop_has_only_primary_call(view, loop_info, call))
    .then_some(())?;
    let argument = call
        .arguments
        .as_deref()
        .map(strip_call_delimiters)
        .map(compact)
        .unwrap_or_default();
    let binding = compact(binding);
    // Rust's `extend` of cloned items equals pushing `item.clone()`; a Java
    // `clone()` is a copy `addAll` would not make.
    let cloned = view.file.language == Language::Rust && argument == format!("{binding}.clone()");
    (argument == binding || cloned).then_some(())?;
    let operation = match view.file.language {
        Language::Rust | Language::Python => "extend",
        Language::JavaScript | Language::TypeScript => "push_spread",
        Language::Java => "add_all",
        Language::CSharp => "add_range",
        _ => return None,
    };
    Some(BulkExtensionCandidate {
        source,
        target,
        operation,
        call,
    })
}

/// `for _, item := range source { target = append(target, item) }` into a
/// slice the caller passed in, which `append(target, source...)` replaces. A
/// fresh empty slice filled this way is a copy, owned by
/// `known_size_preallocation` (golangci-lint `prealloc`).
fn go_bulk_extension_candidate<'a>(
    view: &FunctionView<'a>,
    loop_info: &'a StructureLoop,
) -> Option<BulkExtensionCandidate<'a>> {
    let binding = go_range_value(loop_info)?;
    let source = loop_info.iterable.as_deref().and_then(simple_target)?;
    let call = go_single_append(view, loop_info)?;
    let parts = split_arguments(strip_call_delimiters(call.arguments.as_deref()?));
    let [target, value] = parts.as_slice() else {
        return None;
    };
    let target = simple_target(target)?;
    (source != target
        && compact(value) == binding
        && declared_with_any_type(view, &source, &["[]"])
        && is_slice_parameter(view, &target)
        && loop_is_single_append(view, loop_info, call, &target))
    .then_some(BulkExtensionCandidate {
        source,
        target,
        operation: "append_spread",
        call,
    })
}

fn is_slice_parameter(view: &FunctionView<'_>, name: &str) -> bool {
    view.file.symbols.iter().any(|symbol| {
        symbol.kind == auditor_ast::SymbolKind::Parameter
            && symbol.parent == Some(view.owner)
            && symbol.name == name
            && symbol
                .declaration
                .as_deref()
                .is_some_and(|declaration| declaration.contains("[]"))
    })
}

/// The value binding of a Go `range` loop (`item` in `for _, item := range`).
pub(super) fn go_range_value(loop_info: &StructureLoop) -> Option<String> {
    match loop_info.binding.as_slice() {
        [_, value] if value != "_" => Some(compact(value)),
        _ => None,
    }
}

/// The one `append` builtin call directly inside a loop.
pub(super) fn go_single_append<'a>(
    view: &FunctionView<'a>,
    loop_info: &StructureLoop,
) -> Option<&'a StructureCall> {
    let appends = calls(view)
        .filter(|call| {
            call.api.as_deref() == Some("append")
                && contains_span(loop_info.span, call.span)
                && !has_nested_control(view.file, call.control_region, loop_info.control_region)
        })
        .collect::<Vec<_>>();
    match appends.as_slice() {
        [call] => Some(*call),
        _ => None,
    }
}

fn initialized_with(
    view: &FunctionView<'_>,
    target: &str,
    before: usize,
    predicate: impl Fn(&str) -> bool,
) -> bool {
    assignments(view)
        .filter(|assignment| assignment.span.end_byte <= before)
        .filter(|assignment| simple_target(&assignment.target).as_deref() == Some(target))
        .last()
        .and_then(|assignment| assignment.value.as_deref())
        .is_some_and(predicate)
}

fn is_array_literal(value: &str) -> bool {
    let value = compact(value);
    value.starts_with('[') && value.ends_with(']')
}

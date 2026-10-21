pub(super) use super::bulk::bulk_extension_candidate;
use super::flow::assignments;
use super::syntax::{compact, contains_identifier, simple_target, strip_call_delimiters};
use super::FunctionView;
use crate::structure_index::{StructureAssignment, StructureCall, StructureLoop};
use crate::Language;

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
        Language::Cpp => &["vector<std::string>", "vector<string>"],
        _ => return false,
    };
    declared_with_any_type(view, &iterable, markers)
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

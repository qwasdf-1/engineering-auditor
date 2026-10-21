use std::collections::BTreeSet;

use auditor_ast::SymbolKind;

use super::collections::{call_argument_is_typed, declared_with_any_type};
use super::csharp::{
    csharp_materialization, csharp_pure_numeric_call, csharp_pure_string_call,
    csharp_regex_construction,
};
use super::flow::calls;
use super::language_forms::{
    java_collects_only_to_count, java_pure_numeric_call, java_pure_string_call,
    js_filter_materialization,
};
use super::syntax::{call_name, call_receiver, compact, contains_span, simple_target};
use super::FunctionView;
use crate::structure_index::{StructureCall, StructureFile};
use crate::{util, Language};

pub(super) fn cacheable_pure_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    match view.file.language {
        Language::Rust => {
            matches!(
                call_name(call).as_str(),
                "to_ascii_lowercase" | "to_ascii_uppercase" | "to_lowercase" | "to_uppercase"
            ) && call_receiver(call)
                .is_some_and(|receiver| declared_with_any_type(view, receiver, &["str", "String"]))
        }
        Language::Python => {
            matches!(call_name(call).as_str(), "abs" | "len")
                && call_argument_is_typed(view, call, &["int", "float", "str", "list", "tuple"])
        }
        Language::JavaScript | Language::TypeScript => js_pure_math_call(view, call),
        Language::Go => {
            go_pure_numeric_call(view, call)
                || call
                    .api
                    .as_deref()
                    .is_some_and(|api| GO_PURE_STRING_APIS.contains(&api))
                    && call_argument_is_typed(view, call, &["string"])
        }
        Language::Java => java_pure_numeric_call(view, call) || java_pure_string_call(call),
        Language::CSharp => csharp_pure_numeric_call(view, call) || csharp_pure_string_call(call),
        Language::Cpp => super::cpp::cpp_pure_numeric_call(view, call),
        _ => false,
    }
}

/// `Math.abs(x)` and its rounding siblings on a number.
fn js_pure_math_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    call_receiver(call) == Some("Math")
        && matches!(call_name(call).as_str(), "abs" | "floor" | "ceil" | "round")
        && call_argument_is_typed(view, call, &["number"])
}

/// `math.Abs(x)` and its rounding siblings on a Go number.
fn go_pure_numeric_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    call.api
        .as_deref()
        .is_some_and(|api| GO_PURE_NUMERIC_APIS.contains(&api))
        && call_argument_is_typed(view, call, GO_NUMERIC_TYPES)
}

/// Go standard-library functions that are pure in their argument.
const GO_PURE_NUMERIC_APIS: &[&str] = &["math.Abs", "math.Floor", "math.Ceil", "math.Round"];
const GO_PURE_STRING_APIS: &[&str] = &["strings.ToLower", "strings.ToUpper"];
const GO_NUMERIC_TYPES: &[&str] = &["float64", "float32", "int"];

pub(super) fn general_invariant_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    match view.file.language {
        Language::Rust => {
            matches!(call_name(call).as_str(), "abs" | "count_ones")
                && call_receiver(call).is_some_and(|receiver| {
                    declared_with_any_type(
                        view,
                        receiver,
                        &[
                            "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize",
                        ],
                    )
                })
        }
        Language::Python => {
            call_receiver(call).is_none()
                && call_name(call) == "abs"
                && call_argument_is_typed(view, call, &["int", "float"])
        }
        Language::JavaScript | Language::TypeScript => js_pure_math_call(view, call),
        Language::Go => go_pure_numeric_call(view, call),
        Language::Java => java_pure_numeric_call(view, call),
        Language::CSharp => csharp_pure_numeric_call(view, call),
        Language::Cpp => super::cpp::cpp_pure_numeric_call(view, call),
        _ => false,
    }
}

pub(super) fn recognized_regex_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    let expression = call.expression.to_ascii_lowercase();
    match view.file.language {
        Language::Rust => expression.contains("regex::new") && file_imports(view.file, "regex"),
        Language::Python => expression.contains("re.compile") && file_imports(view.file, "re"),
        Language::JavaScript | Language::TypeScript => {
            expression.contains("new regexp")
                && !view
                    .file
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == "RegExp")
        }
        Language::Java => {
            expression.contains("pattern.compile") && file_imports(view.file, "java.util.regex")
        }
        Language::Go => matches!(
            call.api.as_deref(),
            Some("regexp.Compile" | "regexp.MustCompile")
        ),
        Language::CSharp => csharp_regex_construction(call),
        Language::Cpp => super::cpp::cpp_regex_construction(call),
        _ => false,
    }
}

pub(super) fn format_call(call: &StructureCall) -> bool {
    if matches!(
        call.api.as_deref(),
        Some("fmt.Sprintf" | "java.lang.String.format" | "util.format" | "System.String.Format")
    ) {
        return true;
    }
    let expression = call.expression.to_ascii_lowercase();
    call_name(call) == "format"
        && (expression.contains("format!(")
            || expression.contains("format(")
            || expression.contains(".format("))
}

pub(super) fn format_operands_are_primitive(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    let parameters = view
        .file
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Parameter && symbol.parent == Some(view.owner))
        .filter(|symbol| {
            symbol.declaration.as_deref().is_some_and(|declaration| {
                [
                    "str", "String", "bool", "char", "i32", "i64", "u32", "u64", "usize",
                ]
                .iter()
                .any(|kind| declaration.contains(kind))
            })
        })
        .map(|symbol| symbol.name.as_str())
        .collect::<BTreeSet<_>>();
    expression_operands(call).iter().all(|operand| {
        parameters.contains(operand.as_str())
            || operand
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch == '_')
    })
}

pub(super) fn expression_operands(call: &StructureCall) -> Vec<String> {
    // The callee and a static receiver (`String` in `String.format`) are not
    // operands.
    let callee = util::identifiers(&call.callee)
        .into_iter()
        .chain(
            call.receiver
                .iter()
                .filter(|_| call.api.is_some())
                .flat_map(|receiver| util::identifiers(receiver)),
        )
        .collect::<BTreeSet<_>>();
    // Go passes format operands only as arguments, so a verb (`"%s"`) inside
    // its literal is not one; Rust's inline arguments (`"{name}"`) are.
    let expression = if call.api.is_some() {
        util::strip_string_literals(&call.expression)
    } else {
        call.expression.clone()
    };
    util::identifiers(&expression)
        .into_iter()
        .filter(|identifier| !callee.contains(identifier))
        .filter(|identifier| !matches!(identifier.as_str(), "format" | "unwrap" | "expect" | "new"))
        .collect()
}

pub(super) fn receiver_kind(call: &StructureCall) -> &'static str {
    if call_receiver(call).is_some() {
        "typed_receiver"
    } else {
        "free_function"
    }
}

pub(super) fn indexed_len_source(iterable: &str) -> Option<String> {
    let compact = compact(iterable);
    let suffix = ".len()";
    let prefix = compact.strip_prefix("0..")?.strip_suffix(suffix)?;
    simple_target(prefix)
}

pub(super) fn indexed_target(target: &str, binding: &str) -> Option<String> {
    let compact = compact(target);
    let suffix = format!("[{binding}]");
    simple_target(compact.strip_suffix(&suffix)?)
}

pub(super) fn pure_eager_fallback(fallback: &str) -> bool {
    let compact = compact(fallback);
    compact.starts_with("String::from(")
        || matches!(
            compact.as_str(),
            "String::new()" | "Vec::new()" | "Default::default()"
        )
}

pub(super) fn generator_materialization_is_safe(
    view: &FunctionView<'_>,
    call: &StructureCall,
    expression: &str,
) -> bool {
    identity_generator_iterable(expression).is_some_and(|iterable| {
        declared_with_any_type(
            view,
            &iterable,
            &["list[int]", "list[bool]", "tuple[int", "tuple[bool"],
        ) && calls(view)
            .filter(|candidate| contains_span(call.span, candidate.span))
            .count()
            == 1
    })
}

fn identity_generator_iterable(expression: &str) -> Option<String> {
    let prefix = ["sum([", "any([", "all([", "min([", "max(["]
        .iter()
        .find(|prefix| expression.starts_with(**prefix))?;
    let inner = expression.strip_prefix(prefix)?.strip_suffix("])")?;
    let (element, clause) = inner.split_once("for")?;
    let (binding, iterable) = clause.split_once("in")?;
    let element = simple_target(element)?;
    let binding = simple_target(binding)?;
    let iterable = simple_target(iterable)?;
    (element == binding).then_some(iterable)
}

pub(super) fn rust_map_value_is_pure(expression: &str) -> bool {
    let value = compact(expression);
    value.parse::<i128>().is_ok()
        || matches!(value.as_str(), "true" | "false")
        || ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with("String::from(\"") && value.ends_with("\").len()")))
}

pub(super) fn intermediate_materialization_operation(
    view: &FunctionView<'_>,
    call: &StructureCall,
) -> Option<&'static str> {
    let expression = compact(&call.expression);
    if view.file.language == Language::Rust
        && expression.contains(".collect::<Vec")
        && expression.contains(">().len()")
    {
        Some("collect_then_terminal")
    } else if view.file.language == Language::Python
        && generator_materialization_is_safe(view, call, &expression)
    {
        Some("comprehension_in_generator_terminal")
    } else if view.file.language == Language::Java && java_collects_only_to_count(&expression) {
        Some("collect_then_terminal")
    } else if matches!(
        view.file.language,
        Language::JavaScript | Language::TypeScript
    ) {
        js_filter_materialization(call)
    } else if view.file.language == Language::CSharp {
        csharp_materialization(call)
    } else if view.file.language == Language::Kotlin
        && expression.contains('*')
        && expression.contains(".toTypedArray()")
        && super::collections::spread_source_is_typed_collection(view, &expression)
    {
        Some("spread_typed_array")
    } else {
        None
    }
}

fn file_imports(file: &StructureFile, needle: &str) -> bool {
    file.imports.iter().any(|import| {
        [
            import.canonical.as_deref(),
            import.imported.as_deref(),
            import.module.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.contains(needle))
    })
}

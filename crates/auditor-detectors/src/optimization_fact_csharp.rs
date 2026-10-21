//! C# forms of the L5 facts: `System.Math` and `string` calls that are pure,
//! `Regex` construction, `GetValueOrDefault` defaults built eagerly, LINQ
//! results materialized only to count or test them, and the
//! `ContainsKey`-then-insert map shape.

use super::collections::call_argument_is_typed;
use super::language_forms::java_map_value_is_pure;
use super::map_entry::MapEntryForm;
use super::syntax::{call_name, compact, split_arguments, strip_call_delimiters};
use super::FunctionView;
use crate::structure_index::StructureCall;

const PURE_NUMERIC_APIS: &[&str] = &[
    "System.Math.Abs",
    "System.Math.Floor",
    "System.Math.Ceiling",
    "System.Math.Round",
];
const PURE_STRING_APIS: &[&str] = &[
    "System.String.ToLower",
    "System.String.ToUpper",
    "System.String.ToLowerInvariant",
    "System.String.ToUpperInvariant",
    "System.String.Trim",
];
const NUMERIC_TYPES: &[&str] = &["double ", "float ", "int ", "long ", "decimal "];

/// Declared C# collections with a `Count` and an `AddRange`-able sequence.
pub(super) const CSHARP_COLLECTION_TYPES: &[&str] = &[
    "List<",
    "IList<",
    "ICollection<",
    "IReadOnlyCollection<",
    "IReadOnlyList<",
    "HashSet<",
];
/// Sequences of strings a loop can iterate.
pub(super) const CSHARP_STRING_SEQUENCES: &[&str] = &[
    "List<string>",
    "IList<string>",
    "IEnumerable<string>",
    "IReadOnlyList<string>",
    "string[]",
];

/// Whether `name` is declared a concrete `List<T>`, the type with
/// `AddRange` (an `IList<T>` or `IReadOnlyList<T>` has none).
pub(super) fn declared_as_list(view: &FunctionView<'_>, name: &str) -> bool {
    view.file.symbols.iter().any(|symbol| {
        symbol.parent == Some(view.owner)
            && symbol.name == name
            && symbol.declaration.as_deref().is_some_and(|declaration| {
                declaration.match_indices("List<").any(|(index, _)| {
                    !declaration[..index].ends_with(|ch: char| ch.is_alphanumeric() || ch == '_')
                })
            })
    })
}

/// `Math.Abs(x)` and its rounding siblings on a numeric local or parameter.
pub(super) fn csharp_pure_numeric_call(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    resolved_in(call, PURE_NUMERIC_APIS) && call_argument_is_typed(view, call, NUMERIC_TYPES)
}

/// A case conversion or trim of a value the resolver proved is a `string`.
pub(super) fn csharp_pure_string_call(call: &StructureCall) -> bool {
    resolved_in(call, PURE_STRING_APIS)
}

pub(super) fn csharp_regex_construction(call: &StructureCall) -> bool {
    call.api.as_deref() == Some("System.Text.RegularExpressions.Regex..ctor")
}

fn resolved_in(call: &StructureCall, apis: &[&str]) -> bool {
    call.api.as_deref().is_some_and(|api| apis.contains(&api))
}

/// The default of `map.GetValueOrDefault(key, new List<T>())`: an empty
/// container built on every call, present key or not, where
/// `TryGetValue` builds it only when the key is missing.
pub(super) fn csharp_eager_fallback(call: &StructureCall) -> Option<String> {
    if call_name(call) != "GetValueOrDefault" {
        return None;
    }
    let arguments = split_arguments(strip_call_delimiters(call.arguments.as_deref()?));
    let [_, fallback] = arguments.as_slice() else {
        return None;
    };
    let value = compact(fallback);
    let constructed = value.strip_prefix("new")?.strip_suffix("()")?;
    let empty_container = constructed.is_empty()
        || ["List<", "Dictionary<", "HashSet<", "SortedDictionary<"]
            .iter()
            .any(|container| constructed.starts_with(container) && constructed.ends_with('>'))
        || constructed == "StringBuilder";
    empty_container.then(|| fallback.trim().to_string())
}

/// `xs.Where(p).ToList().Count` (or `.Count()`, `.Length`) and
/// `xs.Where(p).ToList().Any()`: a list built only to count or test it,
/// which `Count(p)` or `Any(p)` does without the list. A test stops early, so
/// its lambdas may not call anything whose effects it would skip.
pub(super) fn csharp_materialization(call: &StructureCall) -> Option<&'static str> {
    if !matches!(call_name(call).as_str(), "ToList" | "ToArray") {
        return None;
    }
    let consumer = call.consumer.as_deref()?;
    match consumer.strip_prefix(&compact(&call.expression))? {
        ".Count" | ".Length" | ".LongCount" => Some("materialize_then_count"),
        ".Any" | ".Count>0" | ".Count!=0" | ".Count==0" | ".Length>0" | ".Length==0"
            if lambdas_call_nothing(&call.expression) =>
        {
            Some("materialize_then_existence")
        }
        _ => None,
    }
}

fn lambdas_call_nothing(expression: &str) -> bool {
    expression
        .split("=>")
        .skip(1)
        .all(|body| !body.split(')').next().unwrap_or(body).contains('('))
}

/// `if (!map.ContainsKey(key)) { map[key] = value; }` (or `map.Add(key,
/// value)`) on a dictionary, which `TryAdd` does in one lookup.
pub(super) const CSHARP_MAP_ENTRY: MapEntryForm = MapEntryForm {
    guard: |condition| {
        condition
            .trim_start_matches('(')
            .strip_prefix('!')?
            .strip_suffix(')')?
            .split_once(".ContainsKey(")
    },
    lookups: &["ContainsKey"],
    insert: "Add",
    map_types: &["Dictionary<", "IDictionary<"],
    value_is_pure: |value| java_map_value_is_pure(&value.replace(".Length", ".length()")),
};

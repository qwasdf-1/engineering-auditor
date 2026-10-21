//! Collection operations spelled as package functions: Go's `sort` and
//! `slices`, Java's `Collections` and `Arrays`, and C++ `<algorithm>`, each
//! read as the method it performs on the collection it is given.

use std::collections::BTreeMap;

use super::semantics::{exact_identifier, trim_arguments};
use super::structure::{canonical, collection_identity, CallShape};
use crate::structure_index::StructureCall;

/// Go operates on slices through package functions, and Java on lists and
/// arrays through `Collections` and `Arrays`: `sort.Ints(xs)` and
/// `Collections.sort(xs)` are default sorts of `xs`, `slices.Contains(xs, v)`
/// a linear membership test. The method each API is read as; its first
/// argument is the collection it operates on. A sort given a comparator is
/// `sort_by`.
const PACKAGE_COLLECTION_APIS: &[(&str, &str)] = &[
    ("sort.Ints", "sort"),
    ("sort.Strings", "sort"),
    ("sort.Float64s", "sort"),
    ("slices.Sort", "sort"),
    ("sort.Slice", "sort_by"),
    ("sort.SliceStable", "sort_by"),
    ("slices.SortFunc", "sort_by"),
    ("slices.SortStableFunc", "sort_by"),
    ("slices.Reverse", "reverse"),
    ("slices.Contains", "contains"),
    ("slices.ContainsFunc", "any"),
    ("slices.IndexFunc", "position"),
    ("slices.Min", "min"),
    ("slices.Max", "max"),
    ("slices.Clone", "clone"),
    ("java.util.Collections.sort", "sort"),
    ("java.util.Arrays.sort", "sort"),
    ("java.util.Collections.reverse", "reverse"),
    ("java.util.Collections.shuffle", "shuffle"),
    ("java.util.Collections.min", "min"),
    ("java.util.Collections.max", "max"),
];

/// The method-call shape of a Go or Java collection API call, or `None` for
/// any other call. Only resolved standard-library identities qualify.
pub(super) fn package_collection_call_shape(
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
) -> Option<CallShape> {
    let api = call.api.as_deref()?;
    let method = PACKAGE_COLLECTION_APIS
        .iter()
        .find(|(name, _)| *name == api)?
        .1;
    let arguments = call
        .arguments
        .as_deref()
        .map(trim_arguments)
        .unwrap_or_default();
    let (receiver, rest) = arguments
        .split_once(',')
        .map_or((arguments.as_str(), ""), |(receiver, rest)| {
            (receiver, rest)
        });
    let receiver = receiver.trim();
    let root = canonical(&exact_identifier(receiver)?, aliases);
    let rest = rest.trim();
    let method = if method == "sort" && !rest.is_empty() && api.starts_with("java.") {
        "sort_by"
    } else {
        method
    };
    Some(CallShape {
        collection: collection_identity(receiver, aliases).unwrap_or_else(|| root.clone()),
        root,
        method: method.to_string(),
        arguments: rest.to_string(),
    })
}

/// C++ `<algorithm>` calls over a whole container, by the method each is
/// read as: `std::sort(v.begin(), v.end())` and `std::ranges::sort(v)` sort
/// `v`, and `std::find(v.begin(), v.end(), x)` tests it for `x`. The other
/// arguments (a comparator, value, or predicate) are the method's.
const CPP_ALGORITHMS: &[(&str, &str)] = &[
    ("sort", "sort"),
    ("stable_sort", "sort"),
    ("reverse", "reverse"),
    ("shuffle", "shuffle"),
    ("find", "contains"),
    ("any_of", "any"),
    ("all_of", "all"),
    ("none_of", "any"),
    ("count_if", "count"),
    ("find_if", "find"),
    ("min_element", "min"),
    ("max_element", "max"),
];

pub(super) fn cpp_algorithm_call_shape(
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
) -> Option<CallShape> {
    let callee = call.callee.replace(' ', "");
    let (ranges, name) = match callee.strip_prefix("std::ranges::") {
        Some(name) => (true, name),
        None => (false, callee.strip_prefix("std::")?),
    };
    let method = CPP_ALGORITHMS
        .iter()
        .find(|(algorithm, _)| *algorithm == name)?
        .1;
    let arguments = trim_arguments(call.arguments.as_deref()?);
    let arguments = crate::optimization_facts::split_arguments(&arguments);
    let (container, rest) = if ranges {
        (arguments.first()?.trim().to_string(), &arguments[1..])
    } else {
        let container = arguments.first()?.replace(' ', "");
        let container = container.strip_suffix(".begin()")?.to_string();
        let end = arguments.get(1)?.replace(' ', "");
        (end == format!("{container}.end()")).then_some(())?;
        (container, &arguments[2..])
    };
    let root = canonical(&exact_identifier(&container)?, aliases);
    let method = if method == "sort" && !rest.is_empty() {
        "sort_by"
    } else {
        method
    };
    Some(CallShape {
        collection: collection_identity(&container, aliases).unwrap_or_else(|| root.clone()),
        root,
        method: method.to_string(),
        arguments: rest
            .iter()
            .map(|argument| argument.trim())
            .collect::<Vec<_>>()
            .join(", "),
    })
}

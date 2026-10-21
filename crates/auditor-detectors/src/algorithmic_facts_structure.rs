use super::semantics::{exact_identifier, is_keyword, root_identifier, trim_arguments};
use super::*;

#[derive(Clone, Debug)]
pub(super) struct CallShape {
    pub(super) root: String,
    pub(super) collection: String,
    pub(super) method: String,
    pub(super) arguments: String,
}

pub(super) fn call_shape(
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
) -> Option<CallShape> {
    if let Some(shape) = package_collection_call_shape(call, aliases) {
        return Some(shape);
    }
    let method = util::identifiers(&call.callee).into_iter().next_back()?;
    let receiver = call.receiver.as_deref().unwrap_or_else(|| {
        call.callee
            .rfind(&method)
            .map(|index| &call.callee[..index])
            .unwrap_or_default()
    });
    let root = root_identifier(receiver)?;
    let root = canonical(&root, aliases);
    let arguments = call
        .arguments
        .as_deref()
        .map(trim_arguments)
        .unwrap_or_default();
    Some(CallShape {
        collection: collection_identity(receiver, aliases).unwrap_or_else(|| root.clone()),
        root,
        method: canonical_method(method, &arguments),
        arguments,
    })
}

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

pub(super) fn owner_calls(file: &StructureFile, owner: u32) -> Vec<&StructureCall> {
    let mut calls = file
        .calls
        .iter()
        .filter(|call| call.owner == Some(owner))
        .collect::<Vec<_>>();
    calls.sort_by_key(|call| (call.span.start_byte, call.span.end_byte));
    calls
}

pub(super) fn owner_loops(file: &StructureFile, owner: u32) -> Vec<&StructureLoop> {
    file.loops
        .iter()
        .filter(|loop_fact| loop_fact.owner == Some(owner))
        .collect()
}

pub(super) fn aliases_for(file: &StructureFile, owner: u32) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for assignment in file
        .assignments
        .iter()
        .filter(|assignment| assignment.owner == Some(owner))
    {
        let Some(target) = root_identifier(&assignment.target) else {
            continue;
        };
        let Some(source) = assignment.value.as_deref().and_then(exact_identifier) else {
            continue;
        };
        if target != source {
            aliases.insert(target, source);
        }
    }
    aliases
}

pub(super) fn canonical(value: &str, aliases: &BTreeMap<String, String>) -> String {
    let mut current = value.to_string();
    let mut seen = BTreeSet::new();
    while seen.insert(current.clone()) {
        let Some(next) = aliases.get(&current) else {
            break;
        };
        current = next.clone();
    }
    current
}

/// The first argument of `call` (which starts at its `(`) when that call
/// spans the whole remaining expression.
fn wrapped_argument(call: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut first_comma = None;
    for (index, character) in call.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let end = first_comma.unwrap_or(index);
                    return (index + 1 == call.len()).then(|| call[1..end].trim());
                }
            }
            ',' if depth == 1 && first_comma.is_none() => first_comma = Some(index),
            _ => {}
        }
    }
    None
}

pub(super) fn loop_collection(
    loop_fact: &StructureLoop,
    aliases: &BTreeMap<String, String>,
) -> Option<String> {
    loop_fact
        .iterable
        .as_deref()
        .and_then(|iterable| collection_identity(iterable, aliases))
}

/// Free-function wrappers that traverse their first argument unchanged.
const TRANSPARENT_WRAPPERS: &[&str] = &[
    "sorted",
    "reversed",
    "list",
    "tuple",
    "set",
    "frozenset",
    "enumerate",
    "iter",
];

pub(super) fn collection_identity(
    expression: &str,
    aliases: &BTreeMap<String, String>,
) -> Option<String> {
    let expression = expression.trim();
    if let Some(open) = expression.find('(') {
        // `range(..)` builds a fresh numeric sequence; two ranges are never
        // the same collection, even with identical bounds text.
        if matches!(expression[..open].trim(), "range" | "xrange") {
            return None;
        }
        if TRANSPARENT_WRAPPERS.contains(&expression[..open].trim()) {
            // `sorted(items)` traverses `items`. A wrapper around a composite
            // (`set(a) | set(b)`) has no single collection identity.
            return collection_identity(wrapped_argument(&expression[open..])?, aliases);
        }
    }
    let prefix = expression.split('(').next().unwrap_or(expression);
    let mut identifiers = util::identifiers(prefix)
        .into_iter()
        .filter(|identifier| !is_keyword(identifier))
        .collect::<Vec<_>>();
    while identifiers.last().is_some_and(|identifier| {
        matches!(
            identifier.as_str(),
            "iter"
                | "iter_mut"
                | "into_iter"
                | "values"
                | "keys"
                | "filter"
                | "map"
                | "enumerate"
                | "skip"
                | "take"
                | "stream"
                | "parallelStream"
        )
    }) {
        identifiers.pop();
    }
    let first = identifiers.first_mut()?;
    *first = canonical(first, aliases);
    Some(identifiers.join("."))
}

pub(super) fn declared_within(file: &StructureFile, name: &str, span: SourceSpan) -> bool {
    file.symbols.iter().any(|symbol| {
        symbol.kind == SymbolKind::Variable
            && symbol.name == name
            && span.start_byte <= symbol.span.start_byte
            && symbol.span.end_byte <= span.end_byte
    })
}

pub(super) fn operations_on_max_control_path(
    file: &StructureFile,
    operations: &[(String, Option<u32>)],
) -> Vec<String> {
    let mut direct = BTreeMap::<Option<u32>, Vec<String>>::new();
    for (operation, region) in operations {
        direct.entry(*region).or_default().push(operation.clone());
    }
    let mut children = BTreeMap::<Option<u32>, Vec<u32>>::new();
    for (index, control) in file.controls.iter().enumerate() {
        children
            .entry(control.parent)
            .or_default()
            .push(index as u32);
    }
    max_path_operations(file, None, &direct, &children)
}

fn max_path_operations(
    file: &StructureFile,
    region: Option<u32>,
    direct: &BTreeMap<Option<u32>, Vec<String>>,
    children: &BTreeMap<Option<u32>, Vec<u32>>,
) -> Vec<String> {
    let mut operations = direct.get(&region).cloned().unwrap_or_default();
    let Some(child_regions) = children.get(&region) else {
        return operations;
    };
    let mutually_exclusive = region
        .and_then(|index| file.controls.get(index as usize))
        .is_some_and(|control| {
            matches!(
                control.kind,
                auditor_ast::ControlRegionKind::Conditional | auditor_ast::ControlRegionKind::Match
            )
        });
    let mut best_branch = Vec::new();
    for &child in child_regions {
        let child_operations = max_path_operations(file, Some(child), direct, children);
        let is_branch = file
            .controls
            .get(child as usize)
            .is_some_and(|control| control.kind == auditor_ast::ControlRegionKind::Branch);
        if mutually_exclusive && is_branch {
            if child_operations.len() > best_branch.len() {
                best_branch = child_operations;
            }
        } else {
            operations.extend(child_operations);
        }
    }
    operations.extend(best_branch);
    operations
}

pub(super) fn loop_bindings(loop_fact: &StructureLoop) -> Vec<String> {
    loop_fact
        .binding
        .iter()
        .filter(|name| !is_keyword(name))
        .cloned()
        .collect()
}

pub(super) fn binding_referenced_within(
    file: &StructureFile,
    owner: u32,
    binding: &str,
    region: u32,
) -> bool {
    file.references.iter().any(|reference| {
        reference.owner == Some(owner)
            && reference.name == binding
            && region_within(file, reference.control_region, region)
    })
}

pub(super) fn nearest_loop(
    file: &StructureFile,
    owner: u32,
    region: Option<u32>,
) -> Option<&StructureLoop> {
    file.loops
        .iter()
        .filter(|loop_fact| loop_fact.owner == Some(owner))
        .filter(|loop_fact| region_within(file, region, loop_fact.control_region))
        .max_by_key(|loop_fact| loop_fact.span.start_byte)
}

pub(super) fn region_within(file: &StructureFile, mut region: Option<u32>, ancestor: u32) -> bool {
    let mut seen = BTreeSet::new();
    while let Some(index) = region {
        if index == ancestor {
            return true;
        }
        if !seen.insert(index) {
            return false;
        }
        region = file
            .controls
            .get(index as usize)
            .and_then(|control| control.parent);
    }
    false
}

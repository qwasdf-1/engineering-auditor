use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{SourceSpan, SymbolKind};

use super::*;

pub(super) fn collect_cost_facts(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    collect_loop_carried_copies(file, owner, function, aliases, facts);
    collect_invariant_collection_copies(file, owner, function, aliases, facts);
    collect_recursive_subproblems(file, owner, function, facts);
    collect_cross_collection_traversals(file, owner, function, aliases, facts);
}

fn collect_loop_carried_copies(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for assignment in file
        .assignments
        .iter()
        .filter(|assignment| assignment.owner == Some(owner))
    {
        let Some(loop_fact) = nearest_loop(file, owner, assignment.control_region) else {
            continue;
        };
        if assignment.control_region != Some(loop_fact.control_region) {
            continue;
        }
        let Some(accumulator) = exact_identifier(&assignment.target) else {
            continue;
        };
        let Some(value) = assignment.value.as_deref() else {
            continue;
        };
        let Some(binding) = loop_bindings(loop_fact)
            .into_iter()
            .find(|binding| contains_identifier(value, binding))
        else {
            continue;
        };
        let (collection_kind, _) = collection_shape(file, owner, &accumulator);
        if collection_kind != CollectionKind::LinearSequence
            || loop_collection(loop_fact, aliases).as_deref() == Some(&accumulator)
            || !contains_identifier(value, &accumulator)
            || !initialized_empty_before(file, owner, &accumulator, loop_fact.span.start_byte)
            || !copy_rebuild_expression(&file.language, value, &accumulator)
            || assignments_to_within(file, owner, &accumulator, loop_fact.span) != 1
        {
            continue;
        }
        push_fact(
            file,
            function,
            assignment.line,
            AlgorithmicFactKind::LoopCarriedCopy {
                accumulator,
                binding,
                loop_span: loop_fact.span,
                assignment_span: assignment.span,
                collection_kind,
            },
            facts,
        );
    }
}

fn collect_invariant_collection_copies(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    // Rust `clone`/`to_vec`, Python `list.copy()`.
    let copies: &[&str] = match file.language {
        crate::Language::Rust => &["clone", "to_vec"],
        crate::Language::Python => &["copy"],
        _ => return,
    };
    let calls = owner_calls(file, owner);
    for copy_call in &calls {
        let Some(shape) = call_shape(copy_call, aliases) else {
            continue;
        };
        if !copies.contains(&shape.method.as_str()) {
            continue;
        }
        let Some(loop_fact) = nearest_loop(file, owner, copy_call.control_region) else {
            continue;
        };
        let (collection_kind, cardinality) = collection_shape(file, owner, &shape.root);
        if collection_kind != CollectionKind::LinearSequence
            || cardinality != Cardinality::UnknownScalable
            || !primitive_sequence_declaration(file, owner, &shape.root)
            || loop_collection(loop_fact, aliases).as_deref() == Some(&shape.root)
            || !mutation_lines(
                file,
                MutationScope {
                    owner,
                    root: &shape.root,
                    aliases,
                    region: Some(loop_fact.control_region),
                    except: Some(copy_call.span),
                },
            )
            .is_empty()
        {
            continue;
        }
        let Some(terminal) = calls.iter().copied().find(|terminal| {
            terminal.span != copy_call.span
                && contains_span(terminal.span, copy_call.span)
                && (terminal.callee == "len"
                    || call_shape(terminal, aliases)
                        .is_some_and(|terminal| terminal.method == "len"))
        }) else {
            continue;
        };
        push_fact(
            file,
            function,
            copy_call.line,
            AlgorithmicFactKind::InvariantCollectionCopy {
                collection: shape.root,
                loop_span: loop_fact.span,
                copy_span: copy_call.span,
                terminal_span: terminal.span,
                collection_kind,
            },
            facts,
        );
    }
}

fn collect_recursive_subproblems(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    facts: &mut Vec<AlgorithmicFact>,
) {
    if has_memoization_signal(file, owner) {
        return;
    }
    let parameters = file
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Parameter && symbol.parent == Some(owner))
        .map(|symbol| symbol.name.as_str())
        .collect::<Vec<_>>();
    for parameter in parameters {
        let recursive_calls = owner_calls(file, owner)
            .into_iter()
            .filter(|call| {
                util::identifiers(&call.callee)
                    .last()
                    .is_some_and(|callee| callee == &function.name)
            })
            .filter_map(|call| {
                let arguments = call.arguments.as_deref().map(trim_arguments)?;
                recursive_step(&arguments, parameter).map(|_| (call, arguments))
            })
            .collect::<Vec<_>>();
        let operations = recursive_calls
            .iter()
            .map(|(call, _)| (call.span.start_byte.to_string(), call.control_region))
            .collect::<Vec<_>>();
        let feasible = operations_on_max_control_path(file, &operations)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let calls = recursive_calls
            .into_iter()
            .filter(|(call, _)| feasible.contains(&call.span.start_byte.to_string()))
            .map(|(call, arguments)| (arguments, call.span))
            .collect::<Vec<_>>();
        if calls.len() < 2
            || calls
                .iter()
                .map(|(arguments, _)| arguments)
                .collect::<BTreeSet<_>>()
                .len()
                < 2
        {
            continue;
        }
        push_fact(
            file,
            function,
            function.line,
            AlgorithmicFactKind::RepeatedRecursiveSubproblem {
                parameter: parameter.to_string(),
                function_span: function.span,
                calls,
            },
            facts,
        );
        break;
    }
}

fn collect_cross_collection_traversals(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    let loops = owner_loops(file, owner);
    for inner in &loops {
        let Some(inner_collection) = loop_collection(inner, aliases) else {
            continue;
        };
        let Some(inner_binding) = loop_bindings(inner).into_iter().next() else {
            continue;
        };
        let Some((outer, outer_collection, outer_binding)) = loops
            .iter()
            .filter(|outer| outer.control_region != inner.control_region)
            .filter(|outer| region_within(file, Some(inner.control_region), outer.control_region))
            .filter_map(|outer| {
                let collection = loop_collection(outer, aliases)?;
                let binding = loop_bindings(outer).into_iter().next()?;
                (collection != inner_collection).then_some((*outer, collection, binding))
            })
            .max_by_key(|(outer, _, _)| outer.span.start_byte)
        else {
            continue;
        };
        // An inner collection built inside the outer loop (`fieldNames =
        // names(field)`) is per element, not a second independent collection.
        if declared_within(file, &inner_collection, outer.span)
            || assignments_to_within(file, owner, &inner_collection, outer.span) > 0
        {
            continue;
        }
        let (outer_kind, outer_cardinality) = collection_shape(file, owner, &outer_collection);
        let (inner_kind, inner_cardinality) = collection_shape(file, owner, &inner_collection);
        if outer_kind != CollectionKind::LinearSequence
            || inner_kind != CollectionKind::LinearSequence
            || outer_cardinality != Cardinality::UnknownScalable
            || inner_cardinality != Cardinality::UnknownScalable
            || !binding_referenced_within(file, owner, &outer_binding, inner.control_region)
            || !binding_referenced_within(file, owner, &inner_binding, inner.control_region)
            || constructs_cartesian_output(CartesianOutputQuery {
                file,
                owner,
                inner,
                aliases,
                outer_binding: &outer_binding,
                inner_binding: &inner_binding,
            })
        {
            continue;
        }
        push_fact(
            file,
            function,
            inner.span.start_line,
            AlgorithmicFactKind::CrossCollectionTraversal {
                outer_collection,
                inner_collection,
                outer_binding,
                inner_binding,
                outer_span: outer.span,
                inner_span: inner.span,
                outer_kind,
                inner_kind,
            },
            facts,
        );
    }
}

fn initialized_empty_before(file: &StructureFile, owner: u32, target: &str, before: usize) -> bool {
    file.assignments
        .iter()
        .filter(|assignment| assignment.owner == Some(owner) && assignment.span.end_byte <= before)
        .rfind(|assignment| exact_identifier(&assignment.target).as_deref() == Some(target))
        .and_then(|assignment| assignment.value.as_deref())
        .is_some_and(|value| {
            let value = compact(value);
            matches!(
                value.as_str(),
                "[]" | "Vec::new()"
                    | "vec![]"
                    | "Array()"
                    | "newArray()"
                    | "List.of()"
                    | "newArrayList<>()"
                    | "Collections.emptyList()"
            ) || (value.starts_with("newList<") || value.starts_with("Array.Empty<"))
                && value.ends_with(">()")
        })
}

fn copy_rebuild_expression(
    language: &crate::Language,
    expression: &str,
    accumulator: &str,
) -> bool {
    let expression = compact(expression);
    match language {
        crate::Language::Rust => {
            expression.contains(".concat()") && expression.contains(accumulator)
        }
        crate::Language::Python => {
            expression.contains(&format!("{accumulator}+["))
                || expression.contains(&format!("[{accumulator}]+"))
        }
        crate::Language::JavaScript | crate::Language::TypeScript => {
            expression.starts_with("[...") && expression.contains(accumulator)
        }
        // `Stream.concat(acc.stream(), Stream.of(item))` rebuilds the list.
        crate::Language::Java => {
            expression.starts_with("Stream.concat(")
                && expression.contains(&format!("{accumulator}.stream()"))
        }
        // `acc.Concat(new[] { item }).ToList()`, or the spread `[..acc, item]`.
        crate::Language::CSharp => {
            expression.starts_with(&format!("{accumulator}.Concat("))
                && (expression.ends_with(".ToList()") || expression.ends_with(".ToArray()"))
                || expression.starts_with(&format!("[..{accumulator},"))
        }
        _ => false,
    }
}

fn assignments_to_within(
    file: &StructureFile,
    owner: u32,
    target: &str,
    span: SourceSpan,
) -> usize {
    file.assignments
        .iter()
        .filter(|assignment| assignment.owner == Some(owner))
        .filter(|assignment| contains_span(span, assignment.span))
        .filter(|assignment| exact_identifier(&assignment.target).as_deref() == Some(target))
        .count()
}

fn primitive_sequence_declaration(file: &StructureFile, owner: u32, name: &str) -> bool {
    file.symbols.iter().any(|symbol| {
        symbol.parent == Some(owner)
            && symbol.name == name
            && symbol.declaration.as_deref().is_some_and(|declaration| {
                let declaration = declaration.to_ascii_lowercase();
                [
                    "vec<i8",
                    "vec<i16",
                    "vec<i32",
                    "vec<i64",
                    "vec<u8",
                    "vec<u16",
                    "vec<u32",
                    "vec<u64",
                    "vec<usize",
                    "vec<bool",
                    "vec<char",
                    "list[",
                ]
                .iter()
                .any(|marker| declaration.contains(marker))
            })
    })
}

fn has_memoization_signal(file: &StructureFile, owner: u32) -> bool {
    file.symbols.iter().any(|symbol| {
        symbol.parent == Some(owner)
            && ["memo", "cache"]
                .iter()
                .any(|marker| symbol.name.to_ascii_lowercase().contains(marker))
    }) || owner_calls(file, owner).iter().any(|call| {
        ["memo", "cache"]
            .iter()
            .any(|marker| call.expression.to_ascii_lowercase().contains(marker))
    })
}

fn recursive_step(arguments: &str, parameter: &str) -> Option<usize> {
    let arguments = compact(arguments);
    let rest = arguments.strip_prefix(parameter)?.strip_prefix('-')?;
    let step = rest.parse::<usize>().ok()?;
    (step > 0).then_some(step)
}

struct CartesianOutputQuery<'a> {
    file: &'a StructureFile,
    owner: u32,
    inner: &'a StructureLoop,
    aliases: &'a BTreeMap<String, String>,
    outer_binding: &'a str,
    inner_binding: &'a str,
}

fn constructs_cartesian_output(query: CartesianOutputQuery<'_>) -> bool {
    owner_calls(query.file, query.owner)
        .into_iter()
        .any(|call| {
            contains_span(query.inner.span, call.span)
                && call_shape(call, query.aliases).is_some_and(|shape| {
                    matches!(shape.method.as_str(), "push" | "append" | "add" | "insert")
                        && contains_identifier(&shape.arguments, query.outer_binding)
                        && contains_identifier(&shape.arguments, query.inner_binding)
                })
        })
        || query.file.assignments.iter().any(|assignment| {
            assignment.owner == Some(query.owner)
                && contains_span(query.inner.span, assignment.span)
                && assignment.value.as_deref().is_some_and(|value| {
                    contains_identifier(value, query.outer_binding)
                        && contains_identifier(value, query.inner_binding)
                })
        })
}

fn contains_span(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && inner.end_byte <= outer.end_byte
}

fn compact(expression: &str) -> String {
    expression
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

//! Linear membership tests in loops (`contains`, `includes`, `in`) against
//! a sequence whose size scales with the input. Findings-free.

use super::*;

/// The shape of a collection searched linearly inside `loop_fact`: a linear
/// sequence of more than a handful of items that the loop leaves unmutated.
fn searched_sequence(
    file: &StructureFile,
    owner: u32,
    collection: &str,
    loop_fact: &StructureLoop,
    lookups: &Lookups<'_>,
) -> Option<(CollectionKind, Cardinality)> {
    let (collection_kind, cardinality) =
        declared_collection_shape(file, owner, collection, lookups.callees.types);
    let scope = |region| MutationScope {
        owner,
        root: collection,
        aliases: lookups.aliases,
        region,
        except: None,
    };
    // A literal bound once and never grown stays its size, however many
    // items it holds.
    let fixed = match cardinality {
        Cardinality::KnownBounded(size) => {
            size <= 8
                || mutating_calls(file, &scope(None)).is_empty()
                    && rebinding_assignments(file, &scope(None)).len() <= 1
        }
        Cardinality::UnknownScalable => false,
    };
    let searched = collection_kind == CollectionKind::LinearSequence
        && !fixed
        && mutation_lines(file, scope(Some(loop_fact.control_region))).is_empty();
    searched.then_some((collection_kind, cardinality))
}

pub(super) fn collect_linear_membership(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    lookups: &Lookups<'_>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, lookups.aliases) else {
            continue;
        };
        if !matches!(shape.method.as_str(), "contains" | "includes") {
            continue;
        }
        let Some((loop_fact, binding)) =
            dependent_loop_binding(file, owner, call, &shape.arguments)
        else {
            continue;
        };
        let Some((collection_kind, cardinality)) =
            searched_sequence(file, owner, &shape.root, loop_fact, lookups)
        else {
            continue;
        };
        push_fact(
            file,
            function,
            call.line,
            AlgorithmicFactKind::LinearMembership {
                collection: shape.root,
                needle: binding,
                loop_span: loop_fact.span,
                call_span: call.span,
                collection_kind,
                cardinality,
                appended: None,
            },
            facts,
        );
    }
}

/// Python `needle in collection` on a list inside a loop over the needles:
/// the operator calls `collection.__contains__`, a linear scan.
pub(super) fn collect_membership_tests(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    lookups: &Lookups<'_>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for membership in file
        .memberships
        .iter()
        .filter(|membership| membership.owner == Some(owner))
    {
        let Some(collection) = exact_identifier(&membership.collection) else {
            continue;
        };
        let collection = canonical(&collection, lookups.aliases);
        let Some(loop_fact) = owner_loops(file, owner)
            .into_iter()
            .filter(|loop_fact| {
                loop_fact.span.start_byte <= membership.span.start_byte
                    && membership.span.end_byte <= loop_fact.span.end_byte
                    && !loop_fact.iterable_span.is_some_and(|span| {
                        span.start_byte <= membership.span.start_byte
                            && membership.span.end_byte <= span.end_byte
                    })
            })
            .max_by_key(|loop_fact| loop_fact.span.start_byte)
        else {
            continue;
        };
        let Some(binding) = loop_bindings(loop_fact)
            .into_iter()
            .find(|binding| contains_identifier(&membership.needle, binding))
        else {
            continue;
        };
        let Some((collection_kind, cardinality)) =
            searched_sequence(file, owner, &collection, loop_fact, lookups)
        else {
            continue;
        };
        push_fact(
            file,
            function,
            membership.span.start_line,
            AlgorithmicFactKind::LinearMembership {
                collection,
                needle: binding,
                loop_span: loop_fact.span,
                call_span: membership.span,
                collection_kind,
                cardinality,
                appended: None,
            },
            facts,
        );
    }
}

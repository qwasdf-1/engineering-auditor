use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{SourceSpan, SymbolKind};

use crate::call_graph::CallGraph;
use crate::structure_index::{
    StructureAssignment, StructureCall, StructureFile, StructureIndex, StructureLoop,
    StructureSymbol,
};
use crate::util;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CollectionKind {
    LinearSequence,
    Set,
    Map,
    Unknown,
}

impl CollectionKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LinearSequence => "linear_sequence",
            Self::Set => "set",
            Self::Map => "map",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cardinality {
    UnknownScalable,
    KnownBounded(usize),
}

impl Cardinality {
    pub(crate) fn evidence(self) -> String {
        match self {
            Self::UnknownScalable => "unknown_scalable".to_string(),
            Self::KnownBounded(size) => format!("known_bounded:{size}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SelectionKind {
    Extremum { endpoint: String },
    TopK { k: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct AlgorithmicFact {
    pub(crate) path: String,
    pub(crate) function: String,
    pub(crate) line: usize,
    pub(crate) kind: AlgorithmicFactKind,
    /// The called functions, as `name (path:line)` joined by ` -> `, through
    /// which the operation is reached; `None` when the function performs it.
    pub(crate) via: Option<String>,
}

impl AlgorithmicFact {
    pub(crate) fn cost(&self) -> cost::AlgorithmicCost {
        cost::cost_for(&self.kind)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AlgorithmicFactKind {
    NestedTraversal {
        collection: String,
        outer_binding: String,
        inner_binding: String,
        outer_span: SourceSpan,
        inner_span: SourceSpan,
    },
    RepeatedSort {
        receiver: String,
        loop_span: SourceSpan,
        sort_span: SourceSpan,
        collection_kind: CollectionKind,
        /// Loop calls passed the collection itself whose effect on it is
        /// unresolved (a Rust `&mut` reborrow, a Python list argument).
        argument_calls: Vec<String>,
    },
    SortSelection {
        receiver: String,
        sort_span: SourceSpan,
        use_span: SourceSpan,
        selection: SelectionKind,
    },
    RepeatedTraversal {
        collection: String,
        operations: Vec<String>,
        collection_kind: CollectionKind,
        aliases: Vec<String>,
        function_span: SourceSpan,
    },
    LinearMembership {
        collection: String,
        needle: String,
        loop_span: SourceSpan,
        call_span: SourceSpan,
        collection_kind: CollectionKind,
        cardinality: Cardinality,
        /// The append the membership test guards, when the loop grows the
        /// searched collection with the element it missed (a deduplication).
        appended: Option<SourceSpan>,
    },
    QueryInLoop {
        receiver: String,
        method: String,
        binding: String,
        arguments: String,
        loop_span: SourceSpan,
        call_span: SourceSpan,
    },
    LoopCarriedCopy {
        accumulator: String,
        binding: String,
        loop_span: SourceSpan,
        assignment_span: SourceSpan,
        collection_kind: CollectionKind,
    },
    InvariantCollectionCopy {
        collection: String,
        loop_span: SourceSpan,
        copy_span: SourceSpan,
        terminal_span: SourceSpan,
        collection_kind: CollectionKind,
    },
    RepeatedRecursiveSubproblem {
        parameter: String,
        function_span: SourceSpan,
        calls: Vec<(String, SourceSpan)>,
    },
    FrontRemoval {
        receiver: String,
        method: String,
        loop_span: SourceSpan,
        call_span: SourceSpan,
    },
    CrossCollectionTraversal {
        outer_collection: String,
        inner_collection: String,
        outer_binding: String,
        inner_binding: String,
        outer_span: SourceSpan,
        inner_span: SourceSpan,
        outer_kind: CollectionKind,
        inner_kind: CollectionKind,
    },
}

pub(crate) struct AlgorithmicIndex {
    facts: Vec<AlgorithmicFact>,
}

impl AlgorithmicIndex {
    pub(crate) fn build(structure: &StructureIndex, graph: &CallGraph) -> Self {
        let mut facts = Vec::new();
        for file in structure.files().iter().filter(|file| file.emittable) {
            for (owner, function) in file.symbols.iter().enumerate().filter(|(_, symbol)| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) && !symbol.in_test
            }) {
                collect_function_facts(file, owner as u32, function, &mut facts);
            }
        }
        inherited::inherited_facts(structure, graph, &mut facts);
        facts.sort_by(|left, right| {
            (&left.path, left.line, &left.function).cmp(&(&right.path, right.line, &right.function))
        });
        Self { facts }
    }

    pub(crate) fn facts(&self) -> &[AlgorithmicFact] {
        &self.facts
    }
}

fn collect_function_facts(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    facts: &mut Vec<AlgorithmicFact>,
) {
    let aliases = aliases_for(file, owner);
    collect_nested_traversals(file, owner, function, &aliases, facts);
    collect_repeated_sorts(file, owner, function, &aliases, facts);
    collect_sort_selections(file, owner, function, &aliases, facts);
    collect_repeated_traversals(file, owner, function, &aliases, facts);
    collect_linear_membership(file, owner, function, &aliases, facts);
    collect_membership_tests(file, owner, function, &aliases, facts);
    collect_deduplicating_appends(file, owner, function, &aliases, facts);
    collect_queries_in_loops(file, owner, function, &aliases, facts);
    collect_cost_facts(file, owner, function, &aliases, facts);
    collect_front_removals(file, owner, function, &aliases, facts);
}

fn push_fact(
    file: &StructureFile,
    function: &StructureSymbol,
    line: usize,
    kind: AlgorithmicFactKind,
    facts: &mut Vec<AlgorithmicFact>,
) {
    facts.push(AlgorithmicFact {
        path: file.path.clone(),
        function: function.name.clone(),
        line,
        kind,
        via: None,
    });
}

fn collect_nested_traversals(
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
        let inner_bindings = loop_bindings(inner);
        let Some(inner_binding) = inner_bindings.first() else {
            continue;
        };
        let outer = loops
            .iter()
            .filter(|outer| outer.control_region != inner.control_region)
            .filter(|outer| region_within(file, Some(inner.control_region), outer.control_region))
            .filter(|outer| loop_collection(outer, aliases).as_deref() == Some(&inner_collection))
            .filter_map(|outer| {
                let outer_binding = loop_bindings(outer).into_iter().next()?;
                (outer_binding != *inner_binding
                    && !collection_rooted_in_binding(&inner_collection, &outer_binding))
                .then_some((outer, outer_binding))
            })
            .max_by_key(|(outer, _)| outer.span.start_byte);
        let Some((outer, outer_binding)) = outer else {
            continue;
        };
        if !binding_referenced_within(file, owner, &outer_binding, inner.control_region)
            || !binding_referenced_within(file, owner, inner_binding, inner.control_region)
        {
            continue;
        }
        push_fact(
            file,
            function,
            inner.span.start_line,
            AlgorithmicFactKind::NestedTraversal {
                collection: inner_collection,
                outer_binding,
                inner_binding: inner_binding.clone(),
                outer_span: outer.span,
                inner_span: inner.span,
            },
            facts,
        );
    }
}

fn collection_rooted_in_binding(collection: &str, binding: &str) -> bool {
    collection.split('.').next() == Some(binding)
}

fn collect_repeated_sorts(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, aliases) else {
            continue;
        };
        if !is_default_sort(&shape.method, &shape.arguments) {
            continue;
        }
        let Some(kind) = repeated_sort(file, owner, aliases, call, shape.root) else {
            continue;
        };
        push_fact(file, function, call.line, kind, facts);
    }
}

/// A sort of `root` at `call` that a loop repeats while `root` is
/// unchanged: not declared, rebound, iterated, or mutated in the loop.
fn repeated_sort(
    file: &StructureFile,
    owner: u32,
    aliases: &BTreeMap<String, String>,
    call: &StructureCall,
    root: String,
) -> Option<AlgorithmicFactKind> {
    let loop_fact = nearest_loop(file, owner, call.control_region)?;
    if declared_within(file, &root, loop_fact.span)
        || rebound_within(file, owner, &root, loop_fact.span)
        || loop_collection(loop_fact, aliases).as_deref() == Some(&root)
        || loop_bindings(loop_fact)
            .iter()
            .any(|binding| binding == &root)
    {
        return None;
    }
    let scope = MutationScope {
        owner,
        root: &root,
        aliases,
        region: Some(loop_fact.control_region),
        except: Some(call.span),
    };
    if !mutation_lines(file, scope.clone()).is_empty() {
        return None;
    }
    let argument_calls = calls_receiving_collection(file, &scope);
    Some(AlgorithmicFactKind::RepeatedSort {
        receiver: root,
        loop_span: loop_fact.span,
        sort_span: call.span,
        collection_kind: CollectionKind::LinearSequence,
        argument_calls,
    })
}

fn collect_sort_selections(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    let calls = owner_calls(file, owner);
    for (index, sort_call) in calls.iter().enumerate() {
        let Some(sort) = call_shape(sort_call, aliases) else {
            continue;
        };
        if !is_default_sort(&sort.method, &sort.arguments)
            || nearest_loop(file, owner, sort_call.control_region).is_some()
        {
            continue;
        }
        let Some((use_span, selection)) = sort_selection_after(
            file,
            SortUseQuery {
                owner,
                calls: &calls[index + 1..],
                aliases,
                sort: &sort,
                sort_call,
            },
        ) else {
            continue;
        };
        push_fact(
            file,
            function,
            sort_call.line,
            AlgorithmicFactKind::SortSelection {
                receiver: sort.root,
                sort_span: sort_call.span,
                use_span,
                selection,
            },
            facts,
        );
    }
    collect_sorted_subscripts(file, owner, function, facts);
}

/// Python `sorted(xs)[0]`, `sorted(xs)[-1]`, and `sorted(xs)[:k]`: a full sort
/// of a fresh copy read only at an endpoint or prefix.
fn collect_sorted_subscripts(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for subscript in file
        .subscripts
        .iter()
        .filter(|subscript| subscript.owner == Some(owner))
    {
        let Some(receiver) = subscript
            .operand
            .strip_prefix("sorted(")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(exact_identifier)
        else {
            continue;
        };
        let selection = match (&subscript.index, &subscript.bounds) {
            (Some(index), _) if index == "0" => SelectionKind::Extremum {
                endpoint: "first".to_string(),
            },
            (Some(index), _) if index == "-1" => SelectionKind::Extremum {
                endpoint: "last".to_string(),
            },
            (None, (start, end)) if matches!(start.as_str(), "" | "0") => {
                match end.parse::<usize>() {
                    Ok(k) if k > 1 => SelectionKind::TopK { k },
                    _ => continue,
                }
            }
            _ => continue,
        };
        push_fact(
            file,
            function,
            subscript.span.start_line,
            AlgorithmicFactKind::SortSelection {
                receiver,
                sort_span: subscript.span,
                use_span: subscript.span,
                selection,
            },
            facts,
        );
    }
}

/// Whether `root` is assigned inside `span`: each pass then works on a fresh
/// collection (`values = list(group)`), which Python does not declare.
fn rebound_within(file: &StructureFile, owner: u32, root: &str, span: SourceSpan) -> bool {
    file.assignments.iter().any(|assignment| {
        assignment.owner == Some(owner)
            && span.start_byte <= assignment.span.start_byte
            && assignment.span.end_byte <= span.end_byte
            && exact_identifier(&assignment.target).as_deref() == Some(root)
    })
}

fn collect_repeated_traversals(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    let mut traversals = BTreeMap::<String, Vec<(String, Option<u32>)>>::new();
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, aliases) else {
            continue;
        };
        // Python `list.count(x)` and `list.index(x)` take a value, not a callback.
        let value_search = file.language == crate::Language::Python
            && matches!(shape.method.as_str(), "count" | "index")
            && !shape.arguments.contains('(');
        if !value_search
            && (!is_terminal_traversal(&shape.method)
                || !pure_traversal_arguments(&shape.arguments))
        {
            continue;
        }
        traversals
            .entry(shape.collection)
            .or_default()
            .push((shape.method, call.control_region));
    }
    for (collection, traversals) in traversals {
        let operations = operations_on_max_control_path(file, &traversals);
        if operations.len() < 3
            || !mutation_lines(
                file,
                MutationScope {
                    owner,
                    root: &collection,
                    aliases,
                    region: None,
                    except: None,
                },
            )
            .is_empty()
        {
            continue;
        }
        let (collection_kind, _) = collection_shape(file, owner, &collection);
        let aliases = aliases
            .iter()
            .filter_map(|(alias, source)| {
                (canonical(source, aliases) == collection).then_some(alias.clone())
            })
            .collect();
        push_fact(
            file,
            function,
            function.line,
            AlgorithmicFactKind::RepeatedTraversal {
                collection,
                operations,
                collection_kind,
                aliases,
                function_span: function.span,
            },
            facts,
        );
    }
}

/// The shape of a collection searched linearly inside `loop_fact`: a linear
/// sequence of more than a handful of items that the loop leaves unmutated.
fn searched_sequence(
    file: &StructureFile,
    owner: u32,
    collection: &str,
    aliases: &BTreeMap<String, String>,
    loop_fact: &StructureLoop,
) -> Option<(CollectionKind, Cardinality)> {
    let (collection_kind, cardinality) = collection_shape(file, owner, collection);
    let scope = |region| MutationScope {
        owner,
        root: collection,
        aliases,
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

fn collect_linear_membership(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, aliases) else {
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
            searched_sequence(file, owner, &shape.root, aliases, loop_fact)
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
fn collect_membership_tests(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
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
        let collection = canonical(&collection, aliases);
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
            searched_sequence(file, owner, &collection, aliases, loop_fact)
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

fn collect_queries_in_loops(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for call in owner_calls(file, owner) {
        let Some(shape) = call_shape(call, aliases) else {
            continue;
        };
        let Some(arguments) = store_query_inputs(file, owner, &shape) else {
            continue;
        };
        let Some((loop_span, binding)) = per_item_scope(file, owner, call, &arguments) else {
            continue;
        };
        push_fact(
            file,
            function,
            call.line,
            AlgorithmicFactKind::QueryInLoop {
                receiver: shape.root,
                method: shape.method,
                binding,
                arguments,
                loop_span,
                call_span: call.span,
            },
            facts,
        );
    }
}

#[path = "algorithmic_facts_collections.rs"]
mod collections;
#[path = "algorithmic_facts_cost_collectors.rs"]
mod cost_collectors;
#[path = "algorithmic_facts_dedupe.rs"]
mod dedupe;
#[path = "algorithmic_facts_effects.rs"]
mod effects;
#[path = "algorithmic_facts_front_removal.rs"]
mod front_removal;
#[path = "algorithmic_facts_inherited.rs"]
mod inherited;
#[path = "algorithmic_facts_package_calls.rs"]
mod package_calls;
#[path = "algorithmic_facts_queries.rs"]
mod queries;
#[path = "algorithmic_facts_semantics.rs"]
mod semantics;
#[path = "algorithmic_facts_structure.rs"]
mod structure;

use collections::*;
use cost_collectors::*;
use dedupe::collect_deduplicating_appends;
use effects::*;
use front_removal::collect_front_removals;
use queries::{may_be_store_query, store_query_inputs};
use semantics::*;
use structure::*;

#[path = "algorithmic_cost.rs"]
mod cost;
pub(crate) use cost::{CostExpression, CostTermKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_classification_separates_extremum_and_top_k() {
        assert!(matches!(
            selection_kind("take", "1"),
            Some(SelectionKind::Extremum { .. })
        ));
        assert_eq!(
            selection_kind("slice", "0, 5"),
            Some(SelectionKind::TopK { k: 5 })
        );
        assert_eq!(selection_kind("slice", "1, 5"), None);
    }

    #[test]
    fn collection_kind_distinguishes_linear_and_indexed_membership() {
        assert_eq!(
            collection_kind_from_text("wanted: Vec<String>"),
            CollectionKind::LinearSequence
        );
        assert_eq!(
            collection_kind_from_text("wanted: Set<string>"),
            CollectionKind::Set
        );
    }

    #[test]
    fn traversal_effect_rejects_unknown_callback_calls() {
        assert!(pure_traversal_arguments("|item| item.ready"));
        assert!(pure_traversal_arguments("item => item.ready"));
        assert!(!pure_traversal_arguments("is_ready"));
        assert!(!pure_traversal_arguments("item => check(item)"));
    }
}

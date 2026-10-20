use auditor_ast::SourceSpan;
use auditor_core::{Confidence, Finding, Severity};

use crate::finding_via::through_calls;

use crate::algorithmic_facts::{
    AlgorithmicFact, AlgorithmicFactKind, AlgorithmicIndex, SelectionKind,
};

const REPEATED_TRAVERSAL_SPEC: FindingSpec = FindingSpec::new(
    "Unchanged collection is traversed repeatedly",
    "Fuse pure traversals or build a reusable index when the collection is material.",
    Severity::Low,
    "O(p * n)",
);
const LOOP_CARRIED_COPY_SPEC: FindingSpec = FindingSpec::new(
    "Loop rebuilds a growing collection on every iteration",
    "Append to the locally owned accumulator in place or use a builder with amortized growth.",
    Severity::Low,
    "O(n^2) copy/allocation -> O(n)",
);
const INVARIANT_COPY_SPEC: FindingSpec = FindingSpec::new(
    "Loop copies an unchanged collection for a read-only terminal",
    "Read the terminal property from the original primitive collection without cloning it.",
    Severity::Low,
    "O(m * n) copy/allocation -> O(m)",
);
const RECURSIVE_SUBPROBLEM_SPEC: FindingSpec = FindingSpec::new(
    "Recursive branches recompute overlapping subproblems",
    "Memoize by the decreasing parameter or replace the recurrence with bottom-up dynamic programming.",
    Severity::Medium,
    "O(b^n) repeated calls -> O(n)",
);
const CROSS_COLLECTION_SPEC: FindingSpec = FindingSpec::new(
    "Independent scalable collections are traversed as a nested product",
    "Build an index or use a sort/merge strategy when the pairwise product is not the required output.",
    Severity::Low,
    "O(m * n) -> O(m + n) or O(n log n)",
);

pub(crate) fn nested_quadratic_iteration_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| match &fact.kind {
        AlgorithmicFactKind::NestedTraversal {
            collection,
            outer_binding,
            inner_binding,
            outer_span,
            inner_span,
        } => {
            let message = format!(
                    "Function `{}` independently traverses `{collection}` for `{outer_binding}` and `{inner_binding}`.",
                    fact.function
                );
            let finding = finding(
                    "nested_quadratic_iteration",
                    "algorithmic",
                    fact,
                    FindingSpec::new(
                        "Pairwise nested iteration suggests quadratic work",
                        "Use an index, set, map, or sort/merge strategy when both traversals scale together.",
                        Severity::Low,
                        "O(n^2)",
                    ),
                    message,
                )
                .with_evidence("outer_collection", collection.clone())
                .with_evidence("inner_collection", collection.clone())
                .with_evidence("outer_binding", outer_binding.clone());
            Some(
                finding
                    .with_evidence("inner_binding", inner_binding.clone())
                    .with_evidence("outer_loop_span", span_text(*outer_span))
                    .with_evidence("inner_loop_span", span_text(*inner_span))
                    .with_evidence("dependence", "both_bindings_referenced_in_inner_loop"),
            )
        }
        _ => None,
    })
}

pub(crate) fn repeated_sort_in_loop_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| match &fact.kind {
        AlgorithmicFactKind::RepeatedSort {
            receiver,
            loop_span,
            sort_span,
            collection_kind,
            argument_calls,
        } => Some(
            finding(
                "repeated_sort_in_loop",
                "algorithmic",
                fact,
                FindingSpec::new(
                    "Loop-invariant collection is sorted repeatedly",
                    "Sort the collection once before entering the loop.",
                    Severity::Low,
                    "O(m * n log n)",
                ),
                format!(
                    "Function `{}` repeatedly sorts unchanged `{receiver}` inside a loop.",
                    fact.function
                ),
            )
            .with_loop_evidence(loop_evidence(
                ("receiver", receiver.clone(), collection_kind.as_str()),
                (*loop_span, "sort_span", *sort_span),
            ))
            .with_evidence("calls_receiving_collection", argument_calls.clone()),
        ),
        _ => None,
    })
}

pub(crate) fn quadratic_front_removal_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        match &fact.kind {
        AlgorithmicFactKind::FrontRemoval {
            receiver,
            method,
            loop_span,
            call_span,
        } => Some(with_loop_site(
            finding(
                "quadratic_front_removal",
                "algorithmic",
                fact,
                FindingSpec::new(
                    "Loop removes the first element of an array",
                    "Use a deque (`collections.deque`, `ArrayDeque`, `VecDeque`) or advance an index instead of removing the front element.",
                    Severity::Low,
                    "O(n^2) -> O(n)",
                ),
                format!(
                    "Function `{}` calls `{receiver}.{method}` on the front of an array inside a loop; each call shifts every remaining element.",
                    fact.function
                ),
            )
            .with_evidence("receiver", receiver.clone())
            .with_evidence("collection_kind", "linear_sequence"),
            receiver,
            *loop_span,
            ("removal_span", *call_span),
        )),
        _ => None,
    }
    })
}

pub(crate) fn sort_for_extremum_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        let (receiver, SelectionKind::Extremum { endpoint }) = sort_selection(fact)? else {
            return None;
        };
        let finding = finding(
            "sort_for_extremum",
            "algorithmic",
            fact,
            FindingSpec::new(
                "Full sort is used only to select an extremum",
                "Use a linear min/max operation instead of sorting the full collection.",
                Severity::Low,
                "O(n log n) -> O(n)",
            ),
            format!(
                "Function `{}` sorts `{receiver}` only to select its `{endpoint}` value.",
                fact.function
            ),
        );
        Some(with_sort_evidence(finding, fact).with_evidence("selection", endpoint.clone()))
    })
}

pub(crate) fn full_sort_for_top_k_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        let (receiver, SelectionKind::TopK { k }) = sort_selection(fact)? else {
            return None;
        };
        let finding = finding(
                "full_sort_for_top_k",
                "algorithmic",
                fact,
                FindingSpec::new(
                    "Full sort is used to select a bounded top-k prefix",
                    "Use a bounded heap or selection algorithm when k is materially smaller than the collection.",
                    Severity::Low,
                    "O(n log n) -> O(n log k)",
                ),
                format!(
                    "Function `{}` sorts all of `{receiver}` before selecting {k} value(s).",
                    fact.function
                ),
            );
        Some(
            with_sort_evidence(finding, fact)
                .with_evidence("k", *k)
                .with_identity("k", k.to_string()),
        )
    })
}

/// The receiver and selection of a sort-then-select fact.
fn sort_selection(fact: &AlgorithmicFact) -> Option<(&str, &SelectionKind)> {
    match &fact.kind {
        AlgorithmicFactKind::SortSelection {
            receiver,
            selection,
            ..
        } => Some((receiver, selection)),
        _ => None,
    }
}

/// The evidence every sort-then-select finding carries.
fn with_sort_evidence(finding: Finding, fact: &AlgorithmicFact) -> Finding {
    let AlgorithmicFactKind::SortSelection {
        receiver,
        sort_span,
        use_span,
        ..
    } = &fact.kind
    else {
        return finding;
    };
    finding
        .with_evidence("receiver", receiver.clone())
        .with_evidence("sort_span", span_text(*sort_span))
        .with_evidence("selection_span", span_text(*use_span))
        .with_evidence("direct_mutation_count", 0usize)
        .with_identity("receiver", receiver.clone())
}

pub(crate) fn repeated_collection_traversal_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| match &fact.kind {
        AlgorithmicFactKind::RepeatedTraversal {
            collection,
            operations,
            collection_kind,
            aliases,
            ..
        } => Some(
            counted_finding(
                "repeated_collection_traversal",
                fact,
                REPEATED_TRAVERSAL_SPEC,
                operations.len(),
                format!("pure terminal traversals of unchanged `{collection}`"),
            )
            .with_evidence("collection", collection.clone())
            .with_evidence("collection_kind", collection_kind.as_str())
            .with_evidence("operations", operations.clone())
            .with_evidence("aliases", aliases.clone())
            .with_evidence("side_effects", "pure_or_short_circuit")
            .with_evidence("direct_mutation_count", 0usize)
            .with_identity("collection", collection.clone()),
        ),
        _ => None,
    })
}

pub(crate) fn hot_path_linear_membership_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| match &fact.kind {
        AlgorithmicFactKind::LinearMembership {
            collection,
            needle,
            loop_span,
            call_span,
            collection_kind,
            cardinality,
        } => Some(
            finding(
                "hot_path_linear_membership",
                "algorithmic",
                fact,
                FindingSpec::new(
                    "Loop performs repeated linear membership",
                    "Build a set or index once and reuse it through the loop.",
                    Severity::Low,
                    "O(m * n)",
                ),
                membership_message(fact, needle, collection),
            )
            .with_evidence("cardinality", cardinality.evidence())
            .with_evidence("needle", needle.clone())
            .with_loop_evidence(loop_evidence(
                ("collection", collection.clone(), collection_kind.as_str()),
                (*loop_span, "lookup_span", *call_span),
            )),
        ),
        _ => None,
    })
}

pub(crate) fn n_plus_one_query_in_loop_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        match &fact.kind {
        AlgorithmicFactKind::QueryInLoop {
            receiver,
            method,
            binding,
            arguments,
            loop_span,
            call_span,
        } => Some(with_loop_site(
            finding(
                "n_plus_one_query_in_loop",
                "algorithmic",
                fact,
                FindingSpec::new(
                    "Loop issues a data-dependent read query",
                    "Fetch the required records in one batched query or preload an index before the loop.",
                    Severity::Medium,
                    "O(m * query_cost)",
                ),
                format!(
                    "Function `{}` calls `{receiver}.{method}` with loop-derived `{binding}`.",
                    fact.function
                ),
            )
            .with_evidence("effect", "database_read")
            .with_evidence("receiver", receiver.clone())
            .with_evidence("query_method", method.clone())
            .with_evidence("query_arguments", arguments.clone())
            .with_evidence("loop_binding", binding.clone()),
            receiver,
            *loop_span,
            ("query_span", *call_span),
        )),
        _ => None,
    }
    })
}

pub(crate) fn loop_carried_copy_accumulation_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        match &fact.kind {
        AlgorithmicFactKind::LoopCarriedCopy {
            accumulator,
            binding,
            loop_span,
            assignment_span,
            collection_kind,
        } => Some(
            finding(
                "loop_carried_copy_accumulation",
                "algorithmic",
                fact,
                LOOP_CARRIED_COPY_SPEC,
                format!(
                    "Function `{}` copies growing `{accumulator}` while adding loop value `{binding}`.",
                    fact.function
                ),
            )
            .with_evidence("loop_binding", binding.clone())
            .with_loop_evidence(loop_evidence(
                ("accumulator", accumulator.clone(), collection_kind.as_str()),
                (*loop_span, "assignment_span", *assignment_span),
            )),
        ),
        _ => None,
    }
    })
}

pub(crate) fn invariant_collection_copy_in_loop_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        match &fact.kind {
        AlgorithmicFactKind::InvariantCollectionCopy {
            collection,
            loop_span,
            copy_span,
            terminal_span,
            collection_kind,
        } => Some(
            finding(
                "invariant_collection_copy_in_loop",
                "algorithmic",
                fact,
                INVARIANT_COPY_SPEC,
                format!(
                    "Function `{}` clones unchanged `{collection}` only to read its length inside a loop.",
                    fact.function
                ),
            )
            .with_loop_evidence(loop_evidence(
                ("collection", collection.clone(), collection_kind.as_str()),
                (*loop_span, "copy_span", *copy_span),
            ))
            .with_evidence("terminal_span", span_text(*terminal_span)),
        ),
        _ => None,
    }
    })
}

pub(crate) fn repeated_recursive_subproblem_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| match &fact.kind {
        AlgorithmicFactKind::RepeatedRecursiveSubproblem {
            parameter,
            function_span,
            calls,
        } => Some(
            counted_finding(
                "repeated_recursive_subproblem",
                fact,
                RECURSIVE_SUBPROBLEM_SPEC,
                calls.len(),
                format!("distinct decreasing self-calls from `{parameter}` on one feasible path"),
            )
            .with_evidence("parameter", parameter.clone())
            .with_evidence("recursive_call_count", calls.len())
            .with_evidence(
                "recursive_arguments",
                calls
                    .iter()
                    .map(|(arguments, _)| arguments)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .with_evidence("function_span", span_text(*function_span))
            .with_evidence(
                "recursive_call_spans",
                calls
                    .iter()
                    .map(|(_, span)| span_text(*span))
                    .collect::<Vec<_>>(),
            )
            .with_identity("parameter", parameter.clone()),
        ),
        _ => None,
    })
}

pub(crate) fn nested_cross_collection_traversal_findings(index: &AlgorithmicIndex) -> Vec<Finding> {
    collect_findings(index, |fact| {
        match &fact.kind {
        AlgorithmicFactKind::CrossCollectionTraversal {
            outer_collection,
            inner_collection,
            outer_binding,
            inner_binding,
            outer_span,
            inner_span,
            outer_kind,
            inner_kind,
        } => Some(
            finding(
                "nested_cross_collection_traversal",
                "algorithmic",
                fact,
                CROSS_COLLECTION_SPEC,
                format!(
                    "Function `{}` nests `{inner_collection}` inside `{outer_collection}` with pair-dependent work.",
                    fact.function
                ),
            )
            .with_evidence("outer_collection", outer_collection.clone())
            .with_evidence("inner_collection", inner_collection.clone())
            .with_evidence("outer_binding", outer_binding.clone())
            .with_evidence("inner_binding", inner_binding.clone())
            .with_evidence("outer_collection_kind", outer_kind.as_str())
            .with_evidence("inner_collection_kind", inner_kind.as_str())
            .with_evidence("outer_loop_span", span_text(*outer_span))
            .with_evidence("inner_loop_span", span_text(*inner_span))
            .with_evidence("cartesian_output", false)
            .with_identity("outer_collection", outer_collection.clone())
            .with_identity("inner_collection", inner_collection.clone()),
        ),
        _ => None,
    }
    })
}

struct LoopEvidence {
    subject_key: &'static str,
    subject: String,
    collection_kind: &'static str,
    loop_span: SourceSpan,
    operation_key: &'static str,
    operation_span: SourceSpan,
}

struct FindingSpec {
    title: &'static str,
    recommendation: &'static str,
    severity: Severity,
    cost_model: &'static str,
}

impl FindingSpec {
    const fn new(
        title: &'static str,
        recommendation: &'static str,
        severity: Severity,
        cost_model: &'static str,
    ) -> Self {
        Self {
            title,
            recommendation,
            severity,
            cost_model,
        }
    }
}

fn loop_evidence(
    subject: (&'static str, String, &'static str),
    operation: (SourceSpan, &'static str, SourceSpan),
) -> LoopEvidence {
    LoopEvidence {
        subject_key: subject.0,
        subject: subject.1,
        collection_kind: subject.2,
        loop_span: operation.0,
        operation_key: operation.1,
        operation_span: operation.2,
    }
}

trait FindingLoopEvidence {
    fn with_loop_evidence(self, evidence: LoopEvidence) -> Self;
}

impl FindingLoopEvidence for Finding {
    fn with_loop_evidence(self, evidence: LoopEvidence) -> Self {
        self.with_evidence(evidence.subject_key, evidence.subject.clone())
            .with_evidence("collection_kind", evidence.collection_kind)
            .with_evidence("loop_span", span_text(evidence.loop_span))
            .with_evidence(evidence.operation_key, span_text(evidence.operation_span))
            .with_evidence("direct_mutation_count", 0usize)
            .with_identity(evidence.subject_key, evidence.subject)
    }
}

fn collect_findings(
    index: &AlgorithmicIndex,
    build: impl Fn(&AlgorithmicFact) -> Option<Finding>,
) -> Vec<Finding> {
    index.facts().iter().filter_map(build).collect()
}

fn finding(
    detector: &'static str,
    category: &'static str,
    fact: &AlgorithmicFact,
    spec: FindingSpec,
    message: String,
) -> Finding {
    let cost = fact.cost();
    let finding = auditor_core::finding!(
        detector,
        category,
        spec.title,
        message,
        spec.severity,
        Confidence::High,
        &fact.path,
        Some(fact.line),
    );
    through_calls(finding, fact.via.as_deref())
        .with_recommendation(spec.recommendation)
        .with_evidence("function", fact.function.clone())
        .with_evidence("cost_model", spec.cost_model)
        .with_evidence("cost_expression", cost.observed.normalized())
        .with_evidence("cost_terms", cost.observed.terms())
        .with_evidence("cost_composition", cost.observed.composition())
        .with_evidence("alternative_cost_expression", cost.alternative.normalized())
        .with_evidence("cost_confidence_boundary", cost.confidence_boundary)
}

/// Evidence for an operation a loop repeats on `receiver`: the loop's span,
/// the operation's span under `site_key`, and the receiver as identity.
fn with_loop_site(
    finding: Finding,
    receiver: &str,
    loop_span: SourceSpan,
    (site_key, site_span): (&'static str, SourceSpan),
) -> Finding {
    finding
        .with_evidence("loop_span", span_text(loop_span))
        .with_evidence(site_key, span_text(site_span))
        .with_identity("receiver", receiver.to_string())
}

fn span_text(span: SourceSpan) -> String {
    format!(
        "{}:{}-{}:{}",
        span.start_line, span.start_column, span.end_line, span.end_column
    )
}

fn membership_message(fact: &AlgorithmicFact, needle: &str, collection: &str) -> String {
    format!(
        "Function `{}` checks loop-derived `{needle}` against linear collection `{collection}`.",
        fact.function
    )
}

fn function_count_message(fact: &AlgorithmicFact, count: usize, action: String) -> String {
    format!("Function `{}` performs {count} {action}.", fact.function)
}

fn counted_finding(
    detector: &'static str,
    fact: &AlgorithmicFact,
    spec: FindingSpec,
    count: usize,
    action: String,
) -> Finding {
    finding(
        detector,
        "algorithmic",
        fact,
        spec,
        function_count_message(fact, count, action),
    )
}

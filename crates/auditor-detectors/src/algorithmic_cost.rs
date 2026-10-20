use auditor_ast::SourceSpan;

use super::{AlgorithmicFactKind, SelectionKind};

#[path = "algorithmic_cost_model.rs"]
mod model;
pub(crate) use model::{AlgorithmicCost, CostExpression, CostTermKind};

pub(super) fn cost_for(kind: &AlgorithmicFactKind) -> AlgorithmicCost {
    match kind {
        AlgorithmicFactKind::NestedTraversal { .. }
        | AlgorithmicFactKind::RepeatedSort { .. }
        | AlgorithmicFactKind::SortSelection { .. }
        | AlgorithmicFactKind::RepeatedTraversal { .. } => traversal_cost(kind),
        AlgorithmicFactKind::LinearMembership { .. }
        | AlgorithmicFactKind::QueryInLoop { .. }
        | AlgorithmicFactKind::LoopCarriedCopy { .. }
        | AlgorithmicFactKind::InvariantCollectionCopy { .. } => loop_operation_cost(kind),
        AlgorithmicFactKind::RepeatedRecursiveSubproblem { .. }
        | AlgorithmicFactKind::CrossCollectionTraversal { .. } => composed_cost(kind),
        AlgorithmicFactKind::FrontRemoval {
            receiver,
            loop_span,
            call_span,
            ..
        } => loop_product_cost(
            *loop_span,
            linear(receiver, *call_span),
            linear("loop", *loop_span),
            "array_backed_sequence_and_unbounded_loop",
        ),
    }
}

fn traversal_cost(kind: &AlgorithmicFactKind) -> AlgorithmicCost {
    match kind {
        AlgorithmicFactKind::NestedTraversal {
            collection,
            outer_span,
            inner_span,
            ..
        } => cost(
            product(
                linear(collection, *outer_span),
                linear(collection, *inner_span),
            ),
            linear(collection, *outer_span),
            "replacement_requires_index_or_ordering_contract",
        ),
        AlgorithmicFactKind::RepeatedSort {
            receiver,
            loop_span,
            sort_span,
            ..
        } => loop_product_cost(
            *loop_span,
            sort(receiver, *sort_span),
            CostExpression::sum(vec![sort(receiver, *sort_span), linear("loop", *loop_span)]),
            "default_total_order_and_no_intervening_mutation",
        ),
        AlgorithmicFactKind::SortSelection {
            receiver,
            sort_span,
            use_span,
            selection,
        } => {
            let alternative = match selection {
                SelectionKind::Extremum { .. } => linear(receiver, *use_span),
                SelectionKind::TopK { k } => product(
                    linear(receiver, *use_span),
                    CostExpression::term(
                        CostTermKind::Logarithmic,
                        format!("k={k}"),
                        Some(*use_span),
                    ),
                ),
            };
            cost(
                sort(receiver, *sort_span),
                alternative,
                "default_order_and_bounded_selection",
            )
        }
        AlgorithmicFactKind::RepeatedTraversal {
            collection,
            operations,
            function_span,
            ..
        } => repeated_linear_cost(
            collection,
            *function_span,
            format!("passes={}", operations.len()),
            "pure_or_short_circuit_traversals_without_mutation",
        ),
        _ => unreachable!("cost family is selected by cost_for"),
    }
}

fn loop_operation_cost(kind: &AlgorithmicFactKind) -> AlgorithmicCost {
    match kind {
        AlgorithmicFactKind::LinearMembership {
            collection,
            loop_span,
            call_span,
            ..
        } => loop_product_cost(
            *loop_span,
            linear(collection, *call_span),
            CostExpression::sum(vec![
                linear(collection, *call_span),
                linear("loop", *loop_span),
            ]),
            "linear_collection_and_scalable_cardinality",
        ),
        AlgorithmicFactKind::QueryInLoop {
            receiver,
            loop_span,
            call_span,
            ..
        } => loop_product_cost(
            *loop_span,
            CostExpression::term(CostTermKind::RepeatedCall, receiver, Some(*call_span)),
            CostExpression::term(CostTermKind::RepeatedCall, "batch_query", Some(*call_span)),
            "typed_read_query_with_loop_dependent_key",
        ),
        AlgorithmicFactKind::LoopCarriedCopy {
            accumulator,
            loop_span,
            assignment_span,
            ..
        } => loop_product_cost(
            *loop_span,
            CostExpression::term(
                CostTermKind::CopyAllocation,
                accumulator,
                Some(*assignment_span),
            ),
            linear("loop", *loop_span),
            "typed_growing_sequence_and_single_direct_reassignment",
        ),
        AlgorithmicFactKind::InvariantCollectionCopy {
            collection,
            loop_span,
            copy_span,
            terminal_span,
            ..
        } => loop_product_cost(
            *loop_span,
            CostExpression::term(CostTermKind::CopyAllocation, collection, Some(*copy_span)),
            CostExpression::sum(vec![
                linear("loop", *loop_span),
                CostExpression::term(CostTermKind::Constant, "terminal", Some(*terminal_span)),
            ]),
            "primitive_element_clone_and_terminal_length_only",
        ),
        _ => unreachable!("cost family is selected by cost_for"),
    }
}

fn composed_cost(kind: &AlgorithmicFactKind) -> AlgorithmicCost {
    match kind {
        AlgorithmicFactKind::RepeatedRecursiveSubproblem {
            parameter,
            function_span,
            calls,
        } => repeated_linear_cost(
            parameter,
            *function_span,
            format!("recursive_branches={}", calls.len()),
            "two_overlapping_decreasing_self_calls_without_memoization",
        ),
        AlgorithmicFactKind::CrossCollectionTraversal {
            outer_collection,
            inner_collection,
            outer_span,
            inner_span,
            ..
        } => cost(
            CostExpression::product(vec![
                linear(outer_collection, *outer_span),
                linear(inner_collection, *inner_span),
                CostExpression::max(vec![
                    CostExpression::term(
                        CostTermKind::Unknown,
                        "pair_dependent_work",
                        Some(*inner_span),
                    ),
                    CostExpression::term(CostTermKind::Constant, "branch_skip", Some(*inner_span)),
                ]),
            ]),
            CostExpression::sum(vec![
                linear(outer_collection, *outer_span),
                linear(inner_collection, *inner_span),
            ]),
            "independent_scalable_inputs_and_non_cartesian_pair_work",
        ),
        _ => unreachable!("cost family is selected by cost_for"),
    }
}

fn loop_product_cost(
    loop_span: SourceSpan,
    operation: CostExpression,
    alternative: CostExpression,
    confidence_boundary: &'static str,
) -> AlgorithmicCost {
    cost(
        product(linear("loop", loop_span), operation),
        alternative,
        confidence_boundary,
    )
}

fn repeated_linear_cost(
    subject: &str,
    span: SourceSpan,
    repeated_call: String,
    confidence_boundary: &'static str,
) -> AlgorithmicCost {
    cost(
        product(
            CostExpression::term(CostTermKind::RepeatedCall, repeated_call, Some(span)),
            linear(subject, span),
        ),
        linear(subject, span),
        confidence_boundary,
    )
}

fn cost(
    observed: CostExpression,
    alternative: CostExpression,
    confidence_boundary: &'static str,
) -> AlgorithmicCost {
    AlgorithmicCost {
        observed,
        alternative,
        confidence_boundary,
    }
}

fn linear(symbol: &str, span: SourceSpan) -> CostExpression {
    CostExpression::term(CostTermKind::Linear, symbol, Some(span))
}

fn sort(symbol: &str, span: SourceSpan) -> CostExpression {
    CostExpression::term(CostTermKind::Sort, symbol, Some(span))
}

fn product(left: CostExpression, right: CostExpression) -> CostExpression {
    CostExpression::product(vec![left, right])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(line: usize) -> SourceSpan {
        SourceSpan {
            start_byte: line,
            end_byte: line + 1,
            start_line: line,
            start_column: 1,
            end_line: line,
            end_column: 2,
        }
    }

    #[test]
    fn cost_composition_is_typed_and_deterministic() {
        let expression = CostExpression::product(vec![
            linear("outer", span(2)),
            CostExpression::max(vec![
                linear("left", span(3)),
                CostExpression::term(CostTermKind::Constant, "right", Some(span(4))),
            ]),
        ]);
        assert_eq!(expression.composition(), "loop_product");
        assert_eq!(expression.terms().len(), 3);
        assert_eq!(expression.normalized(), expression.normalized());
        assert!(expression.normalized().contains("max("));
    }

    #[test]
    fn cost_term_catalog_covers_required_classes() {
        let classes = [
            CostTermKind::Constant,
            CostTermKind::Linear,
            CostTermKind::Logarithmic,
            CostTermKind::Sort,
            CostTermKind::RepeatedCall,
            CostTermKind::CopyAllocation,
            CostTermKind::Unknown,
        ];
        assert_eq!(classes.len(), 7);
        assert!(classes.iter().all(|kind| !kind.as_str().is_empty()));
    }

    #[test]
    fn sequential_composition_uses_sum() {
        let expression =
            CostExpression::sum(vec![linear("left", span(1)), linear("right", span(2))]);
        assert_eq!(expression.composition(), "sum");
        assert!(expression.normalized().starts_with("sum("));
    }
}

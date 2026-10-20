use auditor_core::{Confidence, Finding, Severity};

use crate::optimization_facts::{OptimizationFactKind, OptimizationIndex, OptimizationProof};

pub(crate) fn cache_miss_opportunity_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::RepeatedCall {
                expression,
                callee,
                count,
            } => Some(with_proof(
                auditor_core::finding!(
                    "cache_miss_opportunity",
                    "optimization",
                    "Repeated pure computation can be reused",
                    format!(
                        "Function `{}` evaluates `{expression}` {count} times.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Compute the value once in the shared control-flow region and reuse it.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("call_expression", expression.clone())
                .with_evidence("call_count", *count)
                .with_identity("expression", expression.clone())
                .with_identity("callee", callee.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn repeated_format_allocation_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::LoopInvariantFormat {
                dynamic_identifiers,
            } => Some(with_proof(
                auditor_core::finding!(
                    "repeated_format_allocation",
                    "optimization",
                    "Loop-invariant formatting is allocated repeatedly",
                    format!(
                        "Function `{}` formats loop-invariant data inside a loop.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation("Hoist the formatted value outside the loop and reuse it.")
                .with_evidence("function", fact.function.clone())
                .with_evidence("dynamic_identifiers", dynamic_identifiers.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn loop_invariant_regex_compilation_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::LoopInvariantRegex { api } => Some(with_proof(
                auditor_core::finding!(
                    "loop_invariant_regex_compilation",
                    "optimization",
                    "Loop-invariant regular expression is compiled repeatedly",
                    format!(
                        "Function `{}` calls `{api}` with a loop-invariant pattern.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Cache the regular expression on first use so zero-iteration and first-failure behavior are preserved.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("compile_api", api.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn immutable_string_concatenation_in_loop_findings(
    index: &OptimizationIndex,
) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::StringAccumulation { accumulator } => Some(with_proof(
                auditor_core::finding!(
                    "immutable_string_concatenation_in_loop",
                    "optimization",
                    "Immutable string is rebuilt on each loop iteration",
                    format!(
                        "Function `{}` repeatedly concatenates into `{accumulator}`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Accumulate parts in a builder/list and join once after the loop.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("accumulator", accumulator.clone())
                .with_identity("accumulator", accumulator.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn manual_collection_copy_loop_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::ManualCollectionCopy {
                source,
                target,
                loop_line,
            } => Some(with_proof(
                auditor_core::finding!(
                    "manual_collection_copy_loop",
                    "optimization",
                    "Manual loop copies a collection",
                    format!(
                        "Function `{}` copies `{source}` into `{target}` one element at a time.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation("Use the language or collection bulk-copy operation.")
                .with_evidence("function", fact.function.clone())
                .with_evidence("source", source.clone())
                .with_evidence("target", target.clone())
                .with_evidence("loop_line", *loop_line)
                .with_identity("target", target.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn eager_fallback_computation_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::EagerFallback { api, fallback } => Some(with_proof(
                auditor_core::finding!(
                    "eager_fallback_computation",
                    "optimization",
                    "Fallback computation is evaluated eagerly",
                    format!(
                        "Function `{}` passes computed `{fallback}` to eager fallback API `{api}`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Use the lazy fallback variant or defer the computation with a closure.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("fallback_api", api.clone())
                .with_evidence("fallback_expression", fallback.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn intermediate_collection_materialization_findings(
    index: &OptimizationIndex,
) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::IntermediateMaterialization { operation } => Some(with_proof(
                auditor_core::finding!(
                    "intermediate_collection_materialization",
                    "optimization",
                    "Intermediate collection is materialized for a terminal operation",
                    format!(
                        "Function `{}` materializes a collection before `{operation}`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation("Apply the terminal iterator or sequence operation directly.")
                .with_evidence("function", fact.function.clone())
                .with_evidence("operation", operation.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn range_for_each_iteration_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::RangeForEach { range_expression } => Some(with_proof(
                auditor_core::finding!(
                    "range_for_each_iteration",
                    "optimization",
                    "Range iteration uses forEach",
                    format!(
                        "Function `{}` iterates range `{range_expression}` through `forEach`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation("Use a conventional for loop for range iteration.")
                .with_evidence("function", fact.function.clone())
                .with_evidence("range_expression", range_expression.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn loop_invariant_computation_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::LoopInvariantComputation {
                expression,
                callee,
                loop_line,
            } => Some(with_proof(
                auditor_core::finding!(
                    "loop_invariant_computation",
                    "optimization",
                    "Pure loop-invariant computation is repeated",
                    format!(
                        "Function `{}` evaluates `{expression}` on every loop iteration.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation("Compute the value once before the loop and reuse it.")
                .with_evidence("function", fact.function.clone())
                .with_evidence("expression", expression.clone())
                .with_evidence("callee", callee.clone())
                .with_evidence("loop_line", *loop_line),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn known_size_preallocation_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::KnownSizePreallocation {
                collection,
                target,
                size_expression,
            } => Some(with_proof(
                auditor_core::finding!(
                    "known_size_preallocation",
                    "optimization",
                    "Growable collection has a known final size",
                    format!(
                        "Function `{}` grows `{target}` once per item in `{collection}`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Initialize the collection with capacity from the proven size expression.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("collection", collection.clone())
                .with_evidence("target", target.clone())
                .with_evidence("size_expression", size_expression.clone())
                .with_identity("target", target.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn bulk_collection_extension_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::BulkCollectionExtension {
                source,
                target,
                operation,
                loop_line,
            } => Some(with_proof(
                auditor_core::finding!(
                    "bulk_collection_extension",
                    "optimization",
                    "Per-item append can use a bulk collection operation",
                    format!(
                        "Function `{}` appends every item from `{source}` to `{target}`.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Use the collection's bulk extension operation while preserving item order.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("source", source.clone())
                .with_evidence("target", target.clone())
                .with_evidence("bulk_operation", operation.clone())
                .with_evidence("loop_line", *loop_line),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn map_entry_opportunity_findings(index: &OptimizationIndex) -> Vec<Finding> {
    index
        .facts()
        .iter()
        .filter_map(|fact| match &fact.kind {
            OptimizationFactKind::MapEntryOpportunity {
                map,
                key,
                insertion,
            } => Some(with_proof(
                auditor_core::finding!(
                    "map_entry_opportunity",
                    "optimization",
                    "Map lookup and insertion can use the entry API",
                    format!(
                        "Function `{}` checks `{map}` for `{key}` before inserting it.",
                        fact.function
                    ),
                    Severity::Low,
                    Confidence::High,
                    &fact.path,
                    Some(fact.line),
                )
                .with_recommendation(
                    "Use the map entry API with a lazy value closure to preserve evaluation order.",
                )
                .with_evidence("function", fact.function.clone())
                .with_evidence("map", map.clone())
                .with_evidence("key", key.clone())
                .with_evidence("insertion", insertion.clone())
                .with_identity("map", map.clone())
                .with_identity("key", key.clone()),
                &fact.proof,
            )),
            _ => None,
        })
        .collect()
}

fn with_proof(finding: Finding, proof: &OptimizationProof) -> Finding {
    let finding = finding
        .with_evidence("proof_invariant_operands", proof.invariant_operands)
        .with_evidence("proof_purity", proof.purity)
        .with_evidence(
            "proof_no_intervening_mutation",
            proof.no_intervening_mutation,
        )
        .with_evidence("proof_evaluation_order", proof.evaluation_order)
        .with_evidence("proof_failure_behavior", proof.failure_behavior)
        .with_evidence("proof_receiver_kind", proof.receiver_kind)
        .with_evidence("proof_result_use", proof.result_use);
    if let Some(known_size) = &proof.known_size {
        finding.with_evidence("proof_known_size", known_size.clone())
    } else {
        finding
    }
}

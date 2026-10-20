use std::collections::BTreeSet;

use auditor_ast::SymbolKind;

use crate::algorithmic_facts::AlgorithmicIndex;
use crate::structure_index::{StructureFile, StructureIndex, StructureSymbol};

#[path = "optimization_fact_collections.rs"]
mod collections;
#[path = "optimization_fact_collectors.rs"]
mod collectors;
#[path = "optimization_fact_flow.rs"]
mod flow;
#[path = "optimization_fact_language_forms.rs"]
mod language_forms;
#[path = "optimization_fact_loop_forms.rs"]
mod loop_forms;
#[path = "optimization_fact_operations.rs"]
mod operations;
#[path = "optimization_fact_syntax.rs"]
mod syntax;
#[cfg(test)]
#[path = "optimization_fact_tests.rs"]
mod tests;

use collectors::collect_function_facts;
pub(crate) use syntax::{split_arguments, strip_call_delimiters};

#[derive(Clone, Debug)]
pub(crate) struct OptimizationIndex {
    facts: Vec<OptimizationFact>,
}

#[derive(Clone, Debug)]
pub(crate) struct OptimizationFact {
    pub(crate) path: String,
    pub(crate) function: String,
    pub(crate) line: usize,
    pub(crate) kind: OptimizationFactKind,
    pub(crate) proof: OptimizationProof,
}

#[derive(Clone, Debug)]
pub(crate) struct OptimizationProof {
    pub(crate) invariant_operands: bool,
    pub(crate) purity: &'static str,
    pub(crate) no_intervening_mutation: bool,
    pub(crate) evaluation_order: &'static str,
    pub(crate) failure_behavior: &'static str,
    pub(crate) receiver_kind: &'static str,
    pub(crate) known_size: Option<String>,
    pub(crate) result_use: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) enum OptimizationFactKind {
    RepeatedCall {
        expression: String,
        callee: String,
        count: usize,
    },
    LoopInvariantFormat {
        dynamic_identifiers: Vec<String>,
    },
    LoopInvariantRegex {
        api: String,
    },
    StringAccumulation {
        accumulator: String,
    },
    ManualCollectionCopy {
        source: String,
        target: String,
        loop_line: usize,
    },
    EagerFallback {
        api: String,
        fallback: String,
    },
    IntermediateMaterialization {
        operation: String,
    },
    RangeForEach {
        range_expression: String,
    },
    LoopInvariantComputation {
        expression: String,
        callee: String,
        loop_line: usize,
    },
    KnownSizePreallocation {
        collection: String,
        target: String,
        size_expression: String,
    },
    BulkCollectionExtension {
        source: String,
        target: String,
        operation: String,
        loop_line: usize,
    },
    MapEntryOpportunity {
        map: String,
        key: String,
        insertion: String,
    },
}

struct FunctionView<'a> {
    file: &'a StructureFile,
    owner: u32,
    function: &'a StructureSymbol,
}

impl OptimizationIndex {
    pub(crate) fn build(structure: &StructureIndex, algorithmic: &AlgorithmicIndex) -> Self {
        let algorithmic_lines = algorithmic
            .facts()
            .iter()
            .map(|fact| (fact.path.as_str(), fact.line))
            .collect::<BTreeSet<_>>();
        let mut facts = Vec::new();
        for file in structure.files().iter().filter(|file| file.emittable) {
            for (owner, function) in file.symbols.iter().enumerate().filter(|(_, symbol)| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) && !symbol.in_test
            }) {
                collect_function_facts(
                    FunctionView {
                        file,
                        owner: owner as u32,
                        function,
                    },
                    &algorithmic_lines,
                    &mut facts,
                );
            }
        }
        facts.sort_by(|left, right| {
            (&left.path, left.line, left.kind.rank(), &left.function).cmp(&(
                &right.path,
                right.line,
                right.kind.rank(),
                &right.function,
            ))
        });
        facts.dedup_by(|left, right| {
            left.path == right.path
                && left.line == right.line
                && left.kind.rank() == right.kind.rank()
        });
        Self { facts }
    }

    pub(crate) fn facts(&self) -> &[OptimizationFact] {
        &self.facts
    }
}

impl OptimizationFactKind {
    fn rank(&self) -> u8 {
        match self {
            Self::RepeatedCall { .. } => 0,
            Self::LoopInvariantFormat { .. } => 1,
            Self::LoopInvariantRegex { .. } => 2,
            Self::StringAccumulation { .. } => 3,
            Self::ManualCollectionCopy { .. } => 4,
            Self::EagerFallback { .. } => 5,
            Self::IntermediateMaterialization { .. } => 6,
            Self::RangeForEach { .. } => 7,
            Self::LoopInvariantComputation { .. } => 8,
            Self::KnownSizePreallocation { .. } => 9,
            Self::BulkCollectionExtension { .. } => 10,
            Self::MapEntryOpportunity { .. } => 11,
        }
    }
}

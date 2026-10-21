//! Loop facts in the structure index: each loop's variable, iterable, and
//! the counters its header assigns.

use super::*;

/// The plain names assigned within a loop's header.
fn header_counters(assignments: &[StructureAssignment], header: SourceSpan) -> Vec<String> {
    let mut counters = Vec::new();
    for assignment in assignments {
        let target = assignment.target.trim();
        if header.start_byte <= assignment.span.start_byte
            && assignment.span.end_byte <= header.end_byte
            && !target.is_empty()
            && target
                .chars()
                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
            && !counters.iter().any(|counter| counter == target)
        {
            counters.push(target.to_string());
        }
    }
    counters
}

pub(super) fn collect_loops(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
    assignments: &[StructureAssignment],
) -> Vec<StructureLoop> {
    facts
        .loops
        .iter()
        .map(|loop_fact| StructureLoop {
            owner: nearest_function_owner(symbols, loop_fact.owner),
            binding: loop_fact
                .binding
                .map(|span| util::identifiers(tree.text_for_span(span)))
                .unwrap_or_default(),
            counters: loop_fact
                .header
                .map(|header| header_counters(assignments, header))
                .unwrap_or_default(),
            iterable: loop_fact
                .iterable
                .map(|span| bounded_expression(tree.text_for_span(span))),
            iterable_span: loop_fact.iterable,
            span: loop_fact.span,
            control_region: loop_fact.control_region,
        })
        .collect()
}

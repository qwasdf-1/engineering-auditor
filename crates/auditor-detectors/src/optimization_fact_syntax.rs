use auditor_ast::SourceSpan;

use crate::structure_index::{StructureAssignment, StructureCall};
use crate::util;

pub(super) fn initializer_without_capacity(assignment: &StructureAssignment) -> bool {
    let value = assignment.value.as_deref().map(compact).unwrap_or_default();
    if value.starts_with("make(") {
        let Some(open) = value.find('(') else {
            return false;
        };
        let Some(arguments) = value[open + 1..].strip_suffix(')') else {
            return false;
        };
        return split_arguments(arguments).len() == 2;
    }
    value.contains("[]") || value.ends_with("{}")
}

pub(super) fn call_name(call: &StructureCall) -> String {
    call.callee
        .trim_end_matches('!')
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub(super) fn call_receiver(call: &StructureCall) -> Option<&str> {
    call.receiver.as_deref().or_else(|| {
        let method = call_name(call);
        call.callee
            .rfind(&method)
            .map(|index| call.callee[..index].trim_end_matches(['.', ':']))
            .filter(|receiver| !receiver.is_empty())
    })
}

pub(super) fn simple_target(expression: &str) -> Option<String> {
    let compact = compact(expression);
    let identifiers = util::identifiers(&compact);
    (identifiers.len() == 1 && identifiers[0] == compact).then(|| compact.to_string())
}

pub(super) fn contains_identifier(expression: &str, identifier: &str) -> bool {
    util::identifiers(expression)
        .iter()
        .any(|candidate| candidate == identifier)
}

pub(super) fn contains_span(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && inner.end_byte <= outer.end_byte
}

pub(super) fn compact(expression: &str) -> String {
    expression
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

/// A call's arguments without delimiters or whitespace.
pub(super) fn compact_arguments(call: &StructureCall) -> String {
    call.arguments
        .as_deref()
        .map(strip_call_delimiters)
        .map(compact)
        .unwrap_or_default()
}

pub(crate) fn strip_call_delimiters(arguments: &str) -> &str {
    let trimmed = arguments.trim();
    trimmed
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(trimmed)
}

pub(crate) fn split_arguments(arguments: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, ch) in arguments.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(arguments[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if start < arguments.len() {
        parts.push(arguments[start..].trim());
    }
    parts
}

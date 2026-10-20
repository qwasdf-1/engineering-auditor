use std::collections::BTreeSet;

use auditor_ast::{ControlRegionKind, SourceSpan};

use super::syntax::{compact, contains_span, simple_target};
use super::FunctionView;
use crate::structure_index::{StructureAssignment, StructureCall, StructureFile, StructureLoop};
use crate::util;

pub(super) fn calls<'a>(view: &FunctionView<'a>) -> impl Iterator<Item = &'a StructureCall> + 'a {
    let owner = view.owner;
    view.file
        .calls
        .iter()
        .filter(move |call| call.owner == Some(owner))
}

pub(super) fn assignments<'a>(
    view: &FunctionView<'a>,
) -> impl Iterator<Item = &'a StructureAssignment> + 'a {
    let owner = view.owner;
    view.file
        .assignments
        .iter()
        .filter(move |assignment| assignment.owner == Some(owner))
}

/// The last assignment to `target` before `loop_info`: where the loop's
/// accumulator starts.
pub(super) fn initializer_before<'a>(
    view: &FunctionView<'a>,
    loop_info: &StructureLoop,
    target: &str,
) -> Option<&'a StructureAssignment> {
    assignments(view)
        .filter(|assignment| assignment.span.end_byte <= loop_info.span.start_byte)
        .filter(|assignment| {
            super::syntax::simple_target(&assignment.target).as_deref() == Some(target)
        })
        .last()
}

pub(super) fn loops<'a>(view: &FunctionView<'a>) -> impl Iterator<Item = &'a StructureLoop> + 'a {
    let owner = view.owner;
    view.file
        .loops
        .iter()
        .filter(move |loop_info| loop_info.owner == Some(owner))
}

pub(super) fn containing_loop(
    file: &StructureFile,
    owner: u32,
    region: Option<u32>,
) -> Option<&StructureLoop> {
    file.loops
        .iter()
        .filter(|loop_info| loop_info.owner == Some(owner))
        .filter(|loop_info| region_has_ancestor(file, region, loop_info.control_region))
        .max_by_key(|loop_info| loop_info.span.start_byte)
}

/// A call guarded by a switch on, or an equality test of, the key of a loop
/// over a map runs at most once per loop: map keys are unique, so each
/// constant case matches one iteration at most and hoisting saves nothing.
pub(super) fn runs_once_per_key(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    call: &StructureCall,
) -> bool {
    let (Some(key), Some(iterable)) = (loop_info.binding.first(), loop_info.iterable.as_deref())
    else {
        return false;
    };
    if !super::collections::declared_with_any_type(view, iterable.trim(), &["map["]) {
        return false;
    }
    let mut region = call.control_region;
    while let Some(index) = region.filter(|index| *index != loop_info.control_region) {
        let Some(control) = view.file.controls.get(index as usize) else {
            return false;
        };
        let keyed = control.condition.as_deref().is_some_and(|condition| {
            let condition = compact(condition);
            condition == *key
                || (condition.contains("==") && util::identifiers(&condition).contains(key))
        });
        if keyed {
            return true;
        }
        region = control.parent;
    }
    false
}

fn region_has_ancestor(file: &StructureFile, mut region: Option<u32>, ancestor: u32) -> bool {
    while let Some(current) = region {
        if current == ancestor {
            return true;
        }
        region = file
            .controls
            .get(current as usize)
            .and_then(|control| control.parent);
    }
    false
}

pub(super) fn has_nested_control(
    file: &StructureFile,
    mut region: Option<u32>,
    loop_region: u32,
) -> bool {
    while let Some(current) = region {
        if current == loop_region {
            return false;
        }
        let Some(control) = file.controls.get(current as usize) else {
            return true;
        };
        if control.kind != ControlRegionKind::Branch {
            return true;
        }
        region = control.parent;
    }
    true
}

pub(super) fn operands_are_invariant(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    expression: &str,
    operation_span: SourceSpan,
) -> bool {
    let operands = util::identifiers(expression)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if loop_info
        .binding
        .iter()
        .any(|binding| operands.contains(binding))
    {
        return false;
    }
    !assignments(view).any(|assignment| {
        assignment.span != operation_span
            && contains_span(loop_info.span, assignment.span)
            && util::identifiers(&assignment.target)
                .iter()
                .any(|target| operands.contains(target))
    })
}

pub(super) fn operands_mutated_between(
    view: &FunctionView<'_>,
    expression: &str,
    calls: &[&StructureCall],
) -> bool {
    let operands = util::identifiers(expression)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let start = calls
        .first()
        .map(|call| call.span.end_byte)
        .unwrap_or_default();
    let end = calls
        .last()
        .map(|call| call.span.start_byte)
        .unwrap_or_default();
    assignments(view).any(|assignment| {
        span_is_between(assignment.span, start, end)
            && util::identifiers(&assignment.target)
                .iter()
                .any(|target| operands.contains(target))
    })
}

pub(super) fn loop_has_only_primary_call(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    primary: &StructureCall,
) -> bool {
    !calls(view).any(|call| {
        contains_span(loop_info.span, call.span) && !contains_span(primary.span, call.span)
    }) && !assignments(view).any(|assignment| contains_span(loop_info.span, assignment.span))
}

pub(super) fn loop_is_single_append(
    view: &FunctionView<'_>,
    loop_info: &StructureLoop,
    append: &StructureCall,
    target: &str,
) -> bool {
    if calls(view).any(|call| {
        contains_span(loop_info.span, call.span) && !contains_span(append.span, call.span)
    }) {
        return false;
    }
    let assignments = assignments(view)
        .filter(|assignment| contains_span(loop_info.span, assignment.span))
        .collect::<Vec<_>>();
    assignments.len() == 1
        && simple_target(&assignments[0].target).as_deref() == Some(target)
        && assignments[0]
            .value
            .as_deref()
            .is_some_and(|value| compact(value) == compact(&append.expression))
}

pub(super) fn map_is_touched_between(
    view: &FunctionView<'_>,
    scope: SourceSpan,
    contains_call: &StructureCall,
    insert_call: &StructureCall,
    map: &str,
) -> bool {
    let start = contains_call.span.end_byte;
    let end = insert_call.span.start_byte;
    assignments(view).any(|assignment| {
        contains_span(scope, assignment.span)
            && span_is_between(assignment.span, start, end)
            && util::identifiers(&assignment.target)
                .iter()
                .any(|name| name == map)
    }) || calls(view).any(|call| {
        contains_span(scope, call.span)
            && span_is_between(call.span, start, end)
            && util::identifiers(&call.expression)
                .iter()
                .any(|name| name == map)
    })
}

fn span_is_between(span: SourceSpan, start: usize, end: usize) -> bool {
    start <= span.start_byte && span.end_byte <= end
}

/// A compile already cached on first use: assigned to `x` under a guard
/// that `x` is unset (`if (!re) { re = new RegExp(..) }`).
pub(super) fn lazily_cached(view: &FunctionView<'_>, call: &StructureCall) -> bool {
    let mut region = call.control_region;
    let guard = loop {
        let Some(control) = region.and_then(|index| view.file.controls.get(index as usize)) else {
            return false;
        };
        match control.kind {
            ControlRegionKind::Branch => region = control.parent,
            ControlRegionKind::Conditional => break control,
            _ => return false,
        }
    };
    let condition = guard
        .condition
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<String>();
    let condition = condition.trim_start_matches('(').trim_end_matches(')');
    view.file.assignments.iter().any(|assignment| {
        let target = assignment.target.trim();
        contains_span(assignment.span, call.span)
            && [
                format!("!{target}"),
                format!("not{target}"),
                format!("{target}==null"),
                format!("{target}===null"),
                format!("{target}===undefined"),
                format!("{target}isNone"),
                format!("{target}==nil"),
            ]
            .contains(&condition.to_string())
    })
}

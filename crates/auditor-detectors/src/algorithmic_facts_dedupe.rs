//! The deduplicating append: inside a loop, an element joins a list only
//! when a linear search of that same list misses it
//! (`if x not in seen: seen.append(x)`, `if (!seen.includes(x)) seen.push(x)`),
//! so every step scans all the elements kept so far: O(n^2) where a set
//! keeps the same order of work O(n).

use std::collections::BTreeMap;

use auditor_ast::ControlRegionKind;

use crate::structure_index::StructureControl;

use super::*;

/// Methods that add one element to the end of a list.
const APPENDS: &[&str] = &["append", "push", "add", "push_back", "add_last", "addLast"];

/// A negated membership test: `needle not in collection`, or the call
/// `collection.contains(needle)` an `if` negates.
struct Probe {
    collection: String,
    needle: String,
    span: SourceSpan,
    /// The call's text, which the guarding condition must negate; `None`
    /// for a Python `not in`, negated by its operator.
    call: Option<String>,
}

pub(super) fn collect_deduplicating_appends(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    aliases: &BTreeMap<String, String>,
    facts: &mut Vec<AlgorithmicFact>,
) {
    for probe in probes(file, owner, aliases) {
        let Some(loop_fact) = enclosing_loop(file, owner, probe.span) else {
            continue;
        };
        let Some(needle) = loop_bindings(loop_fact)
            .into_iter()
            .find(|binding| contains_identifier(&probe.needle, binding))
        else {
            continue;
        };
        if collection_shape(file, owner, &probe.collection).0 != CollectionKind::LinearSequence {
            continue;
        }
        let Some(branch) = guarded_branch(file, &probe) else {
            continue;
        };
        let scope = MutationScope {
            owner,
            root: &probe.collection,
            aliases,
            region: Some(loop_fact.control_region),
            except: None,
        };
        let Some(appended) = only_guarded_append(file, &scope, branch, &needle) else {
            continue;
        };
        push_fact(
            file,
            function,
            probe.span.start_line,
            AlgorithmicFactKind::LinearMembership {
                collection: probe.collection,
                needle,
                loop_span: loop_fact.span,
                call_span: probe.span,
                collection_kind: CollectionKind::LinearSequence,
                // The collection grows by the loop's elements.
                cardinality: Cardinality::UnknownScalable,
                appended: Some(appended),
            },
            facts,
        );
    }
}

fn probes(file: &StructureFile, owner: u32, aliases: &BTreeMap<String, String>) -> Vec<Probe> {
    let calls = owner_calls(file, owner).into_iter().filter_map(|call| {
        let shape = call_shape(call, aliases)?;
        matches!(shape.method.as_str(), "contains" | "includes").then(|| Probe {
            collection: shape.root,
            needle: shape.arguments,
            span: call.span,
            call: Some(call.expression.clone()),
        })
    });
    let memberships = file
        .memberships
        .iter()
        .filter(|membership| membership.owner == Some(owner) && membership.negated)
        .filter_map(|membership| {
            Some(Probe {
                collection: canonical(&exact_identifier(&membership.collection)?, aliases),
                needle: membership.needle.clone(),
                span: membership.span,
                call: None,
            })
        });
    calls.chain(memberships).collect()
}

/// The innermost loop whose body holds `span`.
fn enclosing_loop(file: &StructureFile, owner: u32, span: SourceSpan) -> Option<&StructureLoop> {
    owner_loops(file, owner)
        .into_iter()
        .filter(|loop_fact| {
            within(span, loop_fact.span)
                && !loop_fact
                    .iterable_span
                    .is_some_and(|iterable| within(span, iterable))
        })
        .max_by_key(|loop_fact| loop_fact.span.start_byte)
}

/// The consequence branch of the `if` whose whole condition is the negated
/// probe.
fn guarded_branch(file: &StructureFile, probe: &Probe) -> Option<u32> {
    let (index, _) = file.controls.iter().enumerate().find(|(_, control)| {
        control.kind == ControlRegionKind::Conditional
            && control.condition_span.is_some_and(|condition| {
                within(probe.span, condition) && negates(control, probe, condition)
            })
    })?;
    file.controls
        .iter()
        .enumerate()
        .filter(|(_, control)| {
            control.kind == ControlRegionKind::Branch && control.parent == Some(index as u32)
        })
        .min_by_key(|(_, control)| control.span.start_byte)
        .map(|(branch, _)| branch as u32)
}

fn negates(control: &StructureControl, probe: &Probe, condition: SourceSpan) -> bool {
    match &probe.call {
        None => condition == probe.span,
        Some(call) => control.condition.as_deref().is_some_and(|text| {
            let text = compact(text);
            let text = text
                .strip_prefix('(')
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(&text);
            text.strip_prefix('!') == Some(compact(call).as_str())
        }),
    }
}

/// The span of the loop's only change to the collection, when that change
/// adds `needle` inside `branch`: a call to an append method, or Go's
/// `seen = append(seen, x)`.
fn only_guarded_append(
    file: &StructureFile,
    scope: &MutationScope<'_>,
    branch: u32,
    needle: &str,
) -> Option<SourceSpan> {
    let assignments = rebinding_assignments(file, scope);
    // A call that is a rebinding's value (`append(seen, x)`) is that
    // assignment's change, not a second one.
    let calls = mutating_calls(file, scope)
        .into_iter()
        .filter(|call| {
            !assignments
                .iter()
                .any(|assignment| within(call.span, assignment.span))
        })
        .collect::<Vec<_>>();
    match (calls.as_slice(), assignments.as_slice()) {
        ([call], []) => {
            let appends = call_shape(call, scope.aliases)
                .is_some_and(|shape| APPENDS.contains(&shape.method.as_str()));
            (appends
                && region_within(file, call.control_region, branch)
                && call
                    .arguments
                    .as_deref()
                    .is_some_and(|arguments| contains_identifier(arguments, needle)))
            .then_some(call.span)
        }
        ([], [assignment]) => {
            let appends = assignment.value.as_deref().is_some_and(|value| {
                value.trim_start().starts_with("append(") && contains_identifier(value, needle)
            });
            (appends && region_within(file, assignment.control_region, branch))
                .then_some(assignment.span)
        }
        _ => None,
    }
}

fn within(inner: SourceSpan, outer: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && inner.end_byte <= outer.end_byte
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

#[cfg(test)]
#[path = "algorithmic_facts_dedupe_tests.rs"]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{SourceSpan, SymbolKind};

use super::systems_effect_classification::{classify_call, CallSignature};
use super::systems_facts::{EffectKind, SystemEffect};
use crate::optimization_facts::{split_arguments, strip_call_delimiters};
use crate::structure_index::{StructureCall, StructureFile, StructureLoop};
use crate::util;

pub(super) fn extract_effects(file: &StructureFile) -> Vec<SystemEffect> {
    let aliases = assignment_aliases(file);
    let mut effects = file
        .calls
        .iter()
        .flat_map(|call| effects_for_call(file, call, &aliases))
        .collect::<Vec<_>>();
    effects.extend(indexed_cache_effects(file));
    effects.sort_by(|left, right| {
        (&left.path, left.line, left.kind, &left.operation).cmp(&(
            &right.path,
            right.line,
            right.kind,
            &right.operation,
        ))
    });
    effects.dedup_by(|left, right| {
        left.path == right.path
            && left.line == right.line
            && left.kind == right.kind
            && left.operation == right.operation
    });
    effects
}

fn effects_for_call(
    file: &StructureFile,
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
) -> Vec<SystemEffect> {
    let Some(owner) = call.owner else {
        return Vec::new();
    };
    let Some(function) = file.symbols.get(owner as usize) else {
        return Vec::new();
    };
    let output = call_output(file, call, aliases);
    let signature = CallSignature::from_call(call, &file.language);
    let in_async_callback = file
        .async_spans
        .iter()
        .any(|span| span.start_byte <= call.span.start_byte && call.span.end_byte <= span.end_byte);
    let enclosing_loop = enclosing_loop(file, call.control_region, call.span);
    let offloaded = is_offloaded(file, call);
    classify_call(&signature)
        .into_iter()
        .filter(|kind| !(offloaded && *kind == EffectKind::BlockingIo))
        .map(|kind| {
            // State built in a `return` is built once, however deep in a loop
            // (`while (..) { .. return new Regex(pattern); }`).
            let enclosing_loop = enclosing_loop
                .filter(|_| !(kind == EffectKind::ExpensiveInitialization && call.returned));
            let artifact = artifact_expression(kind, call);
            let input = loop_input_expression(kind, call, artifact.as_ref());
            SystemEffect {
                path: file.path.clone(),
                language: file.language.clone(),
                function: function.name.clone(),
                line: call.line,
                loop_line: enclosing_loop.map(|loop_fact| loop_fact.span.start_line),
                batched_loop: enclosing_loop.is_some_and(StructureLoop::walks_batches),
                kind,
                subject: effect_subject(kind, call, aliases, output.as_ref()),
                loop_variant: input.as_deref().is_some_and(|expression| {
                    enclosing_loop.is_some_and(|loop_fact| {
                        expression_depends_on_loop(file, call, loop_fact, expression)
                    })
                }),
                artifact,
                value_lineage: effect_value_lineage(kind, call, aliases, output.as_ref()),
                operation: signature.qualified.clone(),
                in_async: function.is_async || in_async_callback,
                in_hot_path: is_hot_path(&function.name),
                in_request_path: is_request_path(&function.name),
                owner: Some(owner),
                via: None,
            }
        })
        .collect()
}

/// The artifact facts an inherited effect carries, already bound to the
/// call site's arguments.
#[derive(Default)]
pub(super) struct BoundFacts {
    pub(super) artifact: Option<String>,
    pub(super) value_lineage: Option<String>,
    /// Call-site expressions the artifact depends on.
    pub(super) artifact_inputs: Vec<String>,
    /// The artifact varies inside the called function itself.
    pub(super) variant: bool,
}

/// An effect a call inherits from the repository function it invokes:
/// the caller's loop, async, and path context, with the callee's effect kind
/// and operation, and the callee's artifact facts bound to the call site.
pub(super) fn inherited_effect(
    file: &StructureFile,
    call: &StructureCall,
    (kind, operation): (EffectKind, &str),
    facts: BoundFacts,
    via: String,
) -> Option<SystemEffect> {
    if kind == EffectKind::BlockingIo && is_offloaded(file, call) {
        return None;
    }
    let owner = call.owner?;
    let function = file.symbols.get(owner as usize)?;
    let in_async_callback = file
        .async_spans
        .iter()
        .any(|span| span.start_byte <= call.span.start_byte && call.span.end_byte <= span.end_byte);
    let enclosing_loop = enclosing_loop(file, call.control_region, call.span);
    let loop_variant = facts.variant
        || enclosing_loop.is_some_and(|loop_fact| {
            facts
                .artifact_inputs
                .iter()
                .any(|input| expression_depends_on_loop(file, call, loop_fact, input))
        });
    Some(SystemEffect {
        path: file.path.clone(),
        language: file.language.clone(),
        function: function.name.clone(),
        line: call.line,
        loop_line: enclosing_loop.map(|loop_fact| loop_fact.span.start_line),
        batched_loop: enclosing_loop.is_some_and(StructureLoop::walks_batches),
        kind,
        subject: None,
        artifact: facts.artifact,
        value_lineage: facts.value_lineage,
        loop_variant,
        operation: operation.to_string(),
        in_async: function.is_async || in_async_callback,
        in_hot_path: is_hot_path(&function.name),
        in_request_path: is_request_path(&function.name),
        owner: Some(owner),
        via: Some(via),
    })
}

/// Calls that run their callback on another thread: blocking work handed to
/// them does not stall the async caller.
const BLOCKING_BOUNDARIES: &[&str] = &[
    "Task.Run",
    "Task.Factory.StartNew",
    "ThreadPool.QueueUserWorkItem",
    "spawn_blocking",
    "block_in_place",
    "to_thread",
    "run_in_executor",
];

/// `call` sits inside the arguments of a blocking boundary (`Task.Run(() =>
/// ..)`, `new Thread(() => ..)`, `tokio::task::spawn_blocking(..)`).
fn is_offloaded(file: &StructureFile, call: &StructureCall) -> bool {
    file.calls.iter().any(|boundary| {
        boundary.span.start_byte < call.span.start_byte
            && call.span.end_byte <= boundary.span.end_byte
            && (BLOCKING_BOUNDARIES.iter().any(|name| {
                boundary.callee == *name
                    || boundary.callee.ends_with(&format!(".{name}"))
                    || boundary.callee.ends_with(&format!("::{name}"))
            }) || boundary.expression.starts_with("new Thread("))
    })
}

/// Go grows a slice with `cache = append(cache, entry)`; Go and C# read the
/// latest entry by index (`cache[len(cache)-1]`, `cache[cache.Count - 1]`,
/// `cache[^1]`) and bound a cache by reslicing it into itself
/// (`cache = cache[1:]`, `cache = cache[1..]`). Only cache- or history-named
/// collections qualify, as for the method forms.
fn indexed_cache_effects(file: &StructureFile) -> Vec<SystemEffect> {
    if !matches!(file.language, crate::Language::Go | crate::Language::CSharp) {
        return Vec::new();
    }
    let is_cache = |name: &str| {
        let lower = name.to_ascii_lowercase();
        lower.contains("cache") || lower.contains("history")
    };
    let reassigned = |owner: Option<u32>, line: usize, target: &str| {
        file.assignments.iter().any(|assignment| {
            assignment.owner == owner && assignment.line == line && assignment.target == target
        })
    };
    let mut sites = Vec::new();
    for call in &file.calls {
        let Some(target) = (call.api.as_deref() == Some("append"))
            .then(|| {
                call_arguments(call)
                    .first()
                    .map(|first| first.trim().to_string())
            })
            .flatten()
        else {
            continue;
        };
        if is_cache(&target) && reassigned(call.owner, call.line, &target) {
            sites.push((call.owner, call.line, EffectKind::CacheInsert, target));
        }
    }
    for subscript in &file.subscripts {
        let operand = &subscript.operand;
        if !is_cache(operand) {
            continue;
        }
        let line = subscript.span.start_line;
        let latest = [format!("len({operand})-1"), format!("{operand}.Count-1")];
        let index = subscript.index.as_ref().map(|index| index.replace(' ', ""));
        let kind = match &index {
            Some(index) if latest.contains(index) || index == "^1" => EffectKind::CacheLatestRead,
            None if reassigned(subscript.owner, line, operand) => EffectKind::CacheBound,
            _ => continue,
        };
        sites.push((subscript.owner, line, kind, operand.clone()));
    }
    sites
        .into_iter()
        .filter_map(|(owner, line, kind, subject)| {
            let function = file.symbols.get(owner? as usize)?;
            Some(SystemEffect {
                path: file.path.clone(),
                language: file.language.clone(),
                function: function.name.clone(),
                line,
                loop_line: None,
                batched_loop: false,
                kind,
                subject: Some(subject.clone()),
                loop_variant: false,
                artifact: None,
                value_lineage: None,
                operation: subject,
                in_async: false,
                in_hot_path: is_hot_path(&function.name),
                in_request_path: is_request_path(&function.name),
                owner,
                via: None,
            })
        })
        .collect()
}

fn artifact_expression(kind: EffectKind, call: &StructureCall) -> Option<String> {
    let arguments = call_arguments(call);
    match kind {
        EffectKind::ArtifactRead => arguments
            .first()
            .map(|argument| (*argument).to_string())
            .or_else(|| call_receiver(call)),
        EffectKind::ArtifactWrite if arguments.len() >= 2 => {
            arguments.first().map(|argument| (*argument).to_string())
        }
        EffectKind::ArtifactWrite => call_receiver(call),
        _ => None,
    }
}

/// The expression whose dependence on the loop variable decides whether the
/// effect could move out of the loop: the artifact for reads and writes, the
/// constructor arguments for an initialization (or its receiver, when the
/// match is a chained call such as `Regex::new(p).unwrap()`).
fn loop_input_expression(
    kind: EffectKind,
    call: &StructureCall,
    artifact: Option<&String>,
) -> Option<String> {
    match kind {
        EffectKind::ExpensiveInitialization => Some(call_arguments(call).join(", "))
            .filter(|arguments| !arguments.is_empty())
            .or_else(|| call_receiver(call)),
        _ => artifact.cloned(),
    }
}

fn call_receiver(call: &StructureCall) -> Option<String> {
    call.receiver
        .clone()
        .or_else(|| receiver_from_callee(&call.callee))
}

fn effect_value_lineage(
    kind: EffectKind,
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
    output: Option<&String>,
) -> Option<String> {
    match kind {
        EffectKind::ArtifactRead => output.cloned(),
        EffectKind::ArtifactWrite => {
            let arguments = call_arguments(call);
            let value = if arguments.len() >= 2 {
                arguments.get(1)
            } else {
                arguments.first()
            }?;
            simple_subject(value).map(|subject| normalize_subject(&subject, aliases))
        }
        _ => None,
    }
}

pub(super) fn call_arguments(call: &StructureCall) -> Vec<&str> {
    call.arguments
        .as_deref()
        .map(strip_call_delimiters)
        .map(split_arguments)
        .unwrap_or_default()
}

fn expression_depends_on_loop(
    file: &StructureFile,
    call: &StructureCall,
    loop_fact: &StructureLoop,
    expression: &str,
) -> bool {
    let bindings = loop_fact
        .binding
        .iter()
        .chain(&loop_fact.counters)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut pending = util::identifiers(expression);
    let mut seen = BTreeSet::new();
    while let Some(subject) = pending.pop() {
        if bindings.contains(&subject) {
            return true;
        }
        if !seen.insert(subject.clone()) {
            continue;
        }
        for assignment in file.assignments.iter().filter(|assignment| {
            assignment.owner == call.owner
                && assignment.line <= call.line
                && loop_fact.span.start_byte <= assignment.span.start_byte
                && assignment.span.end_byte <= loop_fact.span.end_byte
                && simple_subject(&assignment.target).as_deref() == Some(subject.as_str())
        }) {
            if let Some(value) = &assignment.value {
                pending.extend(util::identifiers(value));
            }
        }
    }
    false
}

fn call_output(
    file: &StructureFile,
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
) -> Option<String> {
    file.assignments
        .iter()
        .find(|assignment| assignment.owner == call.owner && assignment.line == call.line)
        .map(|assignment| normalize_subject(&assignment.target, aliases))
        .or_else(|| {
            file.symbols
                .iter()
                .find(|symbol| {
                    symbol.kind == SymbolKind::Variable
                        && symbol.line == call.line
                        && symbol.parent == call.owner
                        && !symbol.name.is_empty()
                })
                .map(|symbol| normalize_subject(&symbol.name, aliases))
        })
}

fn effect_subject(
    kind: EffectKind,
    call: &StructureCall,
    aliases: &BTreeMap<String, String>,
    output: Option<&String>,
) -> Option<String> {
    match kind {
        // Go decodes into its pointer argument: `json.Unmarshal(data, &value)`.
        EffectKind::Parse
            if call
                .api
                .as_deref()
                .is_some_and(|api| api.ends_with(".Unmarshal")) =>
        {
            call.arguments
                .as_deref()
                .and_then(|arguments| arguments.trim().trim_start_matches('(').split(',').nth(1))
                .and_then(simple_subject)
                .map(|subject| normalize_subject(&subject, aliases))
        }
        EffectKind::Parse => output.cloned().or_else(|| call_subject(call, aliases)),
        EffectKind::CacheInsert | EffectKind::CacheLatestRead | EffectKind::CacheBound => call
            .receiver
            .clone()
            .or_else(|| receiver_from_callee(&call.callee))
            .map(|receiver| normalize_subject(&receiver, aliases)),
        _ => call_subject(call, aliases),
    }
}

pub(super) fn assignment_aliases(file: &StructureFile) -> BTreeMap<String, String> {
    file.assignments
        .iter()
        .filter_map(|assignment| {
            let value = assignment.value.as_deref()?;
            if value.contains('(') {
                return None;
            }
            let target = simple_subject(&assignment.target)?;
            let value = simple_subject(value)?;
            (target != value).then_some((target, value))
        })
        .collect()
}

pub(super) fn normalize_subject(value: &str, aliases: &BTreeMap<String, String>) -> String {
    let mut subject = simple_subject(value).unwrap_or_else(|| value.trim().to_string());
    for _ in 0..8 {
        let Some(next) = aliases.get(&subject) else {
            break;
        };
        subject = next.clone();
    }
    subject
}

pub(super) fn simple_subject(value: &str) -> Option<String> {
    crate::util::identifiers(value)
        .into_iter()
        .find(|identifier| {
            !matches!(
                identifier.as_str(),
                "self" | "mut" | "ref" | "let" | "const" | "var" | "await"
            )
        })
}

fn call_subject(call: &StructureCall, aliases: &BTreeMap<String, String>) -> Option<String> {
    // The receiver as written when the grammar gives none (`parsed.dump()`).
    call.arguments
        .as_deref()
        .and_then(simple_subject)
        .or_else(|| call_receiver(call).as_deref().and_then(simple_subject))
        .map(|subject| normalize_subject(&subject, aliases))
}

fn enclosing_loop(
    file: &StructureFile,
    mut region: Option<u32>,
    call_span: SourceSpan,
) -> Option<&StructureLoop> {
    while let Some(index) = region {
        if let Some(found) = file
            .loops
            .iter()
            .find(|loop_fact| loop_fact.control_region == index)
        {
            let evaluated_once = !found.binding.is_empty()
                && found.iterable_span.is_some_and(|iterable| {
                    call_span.start_byte >= iterable.start_byte
                        && call_span.end_byte <= iterable.end_byte
                });
            if !evaluated_once {
                return Some(found);
            }
        }
        region = file
            .controls
            .get(index as usize)
            .and_then(|control| control.parent);
    }
    None
}

fn receiver_from_callee(callee: &str) -> Option<String> {
    callee
        .rfind(['.', ':'])
        .map(|index| callee[..index].trim_end_matches(':').to_string())
        .filter(|receiver| !receiver.is_empty())
}

fn is_request_path(function: &str) -> bool {
    identifier_words(function).iter().any(|word| {
        matches!(
            word.as_str(),
            "request" | "handler" | "handle" | "endpoint" | "dispatch" | "serve"
        )
    })
}

fn is_hot_path(function: &str) -> bool {
    is_request_path(function)
        || identifier_words(function).iter().any(|word| {
            matches!(
                word.as_str(),
                "status" | "report" | "render" | "poll" | "tick" | "check"
            )
        })
}

fn identifier_words(identifier: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous_lowercase = false;
    for character in identifier.chars() {
        if !character.is_ascii_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word).to_ascii_lowercase());
            }
            previous_lowercase = false;
        } else {
            if character.is_ascii_uppercase() && previous_lowercase && !word.is_empty() {
                words.push(std::mem::take(&mut word).to_ascii_lowercase());
            }
            previous_lowercase = character.is_ascii_lowercase();
            word.push(character);
        }
    }
    if !word.is_empty() {
        words.push(word.to_ascii_lowercase());
    }
    words
}

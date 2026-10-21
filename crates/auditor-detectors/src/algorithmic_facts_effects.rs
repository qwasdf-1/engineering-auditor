use super::semantics::{is_mutator, root_identifier, selection_kind, trim_arguments};
use super::structure::{
    call_shape, collection_identity, loop_bindings, nearest_loop, package_collection_call_shape,
    region_within, CallShape,
};
use super::*;
use crate::Language;

#[derive(Clone)]
pub(super) struct MutationScope<'a> {
    pub(super) owner: u32,
    pub(super) root: &'a str,
    pub(super) aliases: &'a BTreeMap<String, String>,
    pub(super) region: Option<u32>,
    pub(super) except: Option<SourceSpan>,
}

pub(super) fn mutation_lines(file: &StructureFile, scope: MutationScope<'_>) -> Vec<usize> {
    let mut lines = Vec::new();
    for call in file
        .calls
        .iter()
        .filter(|call| call.owner == Some(scope.owner))
    {
        let in_scope = scope.except != Some(call.span)
            && scope
                .region
                .is_none_or(|region| region_within(file, call.control_region, region));
        if in_scope && call_mutates(file, call, &scope) {
            lines.push(call.line);
        }
    }
    for assignment in file
        .assignments
        .iter()
        .filter(|assignment| assignment.owner == Some(scope.owner))
    {
        if scope
            .region
            .is_some_and(|region| !region_within(file, assignment.control_region, region))
        {
            continue;
        }
        if collection_identity(&assignment.target, scope.aliases)
            .is_some_and(|target| target == scope.root)
        {
            lines.push(assignment.line);
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Calls in the scope that receive the collection itself as an argument and
/// are not recognized mutations, such as a Rust helper given a `&mut`
/// parameter by reborrow. Their effect on the collection is unresolved, so
/// findings report them instead of implying the collection is unchanged.
pub(super) fn calls_receiving_collection(
    file: &StructureFile,
    scope: &MutationScope<'_>,
) -> Vec<String> {
    file.calls
        .iter()
        .filter(|call| call.owner == Some(scope.owner) && scope.except != Some(call.span))
        .filter(|call| {
            scope
                .region
                .is_none_or(|region| region_within(file, call.control_region, region))
        })
        .filter(|call| {
            call.arguments.as_deref().is_some_and(|arguments| {
                trim_arguments(arguments)
                    .split(',')
                    .any(|argument| argument.trim() == scope.root)
            })
        })
        .map(|call| call.expression.trim().to_string())
        .collect()
}

/// Whether `call` may mutate the scope's collection: a mutator method on it,
/// or (in languages that pass references) the collection as an argument.
fn call_mutates(file: &StructureFile, call: &StructureCall, scope: &MutationScope<'_>) -> bool {
    // A Go or Java collection API is judged by its table entry alone.
    if let Some(shape) = package_collection_call_shape(call, scope.aliases) {
        return shape.collection == scope.root && is_mutator(&shape.method);
    }
    call.arguments
        .as_deref()
        .is_some_and(|arguments| direct_argument_may_mutate(&file.language, arguments, scope.root))
        || call_shape(call, scope.aliases)
            .is_some_and(|shape| shape.collection == scope.root && is_mutator(&shape.method))
}

fn direct_argument_may_mutate(language: &Language, arguments: &str, target: &str) -> bool {
    let compact_target = target
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    trim_arguments(arguments).split(',').any(|argument| {
        let compact = argument
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        match language {
            Language::Rust => compact.strip_prefix("&mut") == Some(compact_target.as_str()),
            // A Go slice passed by value still shares its elements; a Java
            // argument is a reference.
            Language::JavaScript | Language::TypeScript | Language::Go | Language::Java => {
                compact == compact_target
            }
            // A C# reference type, or a `ref`/`out` argument.
            Language::CSharp => ["", "ref", "out"]
                .iter()
                .any(|modifier| compact.strip_prefix(modifier) == Some(compact_target.as_str())),
            _ => false,
        }
    })
}

fn mutation_lines_between(
    file: &StructureFile,
    mut scope: MutationScope<'_>,
    start: SourceSpan,
    end: SourceSpan,
) -> Vec<usize> {
    scope.except = Some(start);
    mutation_lines(file, scope)
        .into_iter()
        .filter(|line| start.start_line <= *line && *line <= end.end_line)
        .collect()
}

pub(super) struct SortUseQuery<'a> {
    pub(super) owner: u32,
    pub(super) calls: &'a [&'a StructureCall],
    pub(super) aliases: &'a BTreeMap<String, String>,
    pub(super) sort: &'a CallShape,
    pub(super) sort_call: &'a StructureCall,
}

pub(super) fn sort_selection_after(
    file: &StructureFile,
    query: SortUseQuery<'_>,
) -> Option<(SourceSpan, SelectionKind)> {
    for &use_call in query.calls {
        if use_call.span.start_byte <= query.sort_call.span.end_byte {
            continue;
        }
        let Some(use_shape) = call_shape(use_call, query.aliases) else {
            continue;
        };
        if use_shape.root != query.sort.root {
            continue;
        }
        if !mutation_lines_between(
            file,
            MutationScope {
                owner: query.owner,
                root: &query.sort.root,
                aliases: query.aliases,
                region: None,
                except: None,
            },
            query.sort_call.span,
            use_call.span,
        )
        .is_empty()
        {
            return None;
        }
        if let Some(selection) = selection_kind(&use_shape.method, &use_shape.arguments) {
            return Some((use_call.span, selection));
        }
    }
    subscript_selection_after(file, &query)
}

/// `xs[0]`, `xs[len(xs)-1]`, or `xs[:k]` read after the sort with no
/// mutation in between (Go, Python, and C# select by indexing, not by
/// method).
fn subscript_selection_after(
    file: &StructureFile,
    query: &SortUseQuery<'_>,
) -> Option<(SourceSpan, SelectionKind)> {
    let root = &query.sort.root;
    let subscript = file
        .subscripts
        .iter()
        .filter(|subscript| {
            subscript.owner == Some(query.owner)
                && subscript.span.start_byte > query.sort_call.span.end_byte
                && canonical(&subscript.operand, query.aliases) == *root
        })
        .min_by_key(|subscript| subscript.span.start_byte)?;
    let between = mutation_lines_between(
        file,
        MutationScope {
            owner: query.owner,
            root,
            aliases: query.aliases,
            region: None,
            except: None,
        },
        query.sort_call.span,
        subscript.span,
    );
    if !between.is_empty() {
        return None;
    }
    let last = [
        format!("len({root})-1"),
        format!("{root}.Count-1"),
        format!("{root}.Length-1"),
    ];
    let index = subscript.index.as_ref().map(|index| index.replace(' ', ""));
    let selection = match (&index, &subscript.bounds) {
        (Some(index), _) if index == "0" => SelectionKind::Extremum {
            endpoint: "first".to_string(),
        },
        // Python and C# count from the end: `xs[-1]`, `xs[^1]`.
        (Some(index), _) if last.contains(index) || matches!(index.as_str(), "-1" | "^1") => {
            SelectionKind::Extremum {
                endpoint: "last".to_string(),
            }
        }
        (None, (start, end)) if matches!(start.as_str(), "" | "0") => {
            let k = end.parse::<usize>().ok().filter(|k| *k > 1)?;
            SelectionKind::TopK { k }
        }
        _ => return None,
    };
    Some((subscript.span, selection))
}

pub(super) fn dependent_loop_binding<'a>(
    file: &'a StructureFile,
    owner: u32,
    call: &StructureCall,
    arguments: &str,
) -> Option<(&'a StructureLoop, String)> {
    let loop_fact = nearest_loop(file, owner, call.control_region)?;
    let binding = loop_bindings(loop_fact)
        .into_iter()
        .find(|binding| contains_identifier(arguments, binding))?;
    Some((loop_fact, binding))
}

pub(super) fn assignment_root(assignment: &StructureAssignment) -> Option<String> {
    root_identifier(&assignment.target)
}

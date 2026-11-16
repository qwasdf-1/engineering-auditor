//! Import facts a claim states, checked against the repository map's import
//! edges. A claim that one file or module does, or does not, import another
//! states it as a fact the host can check, so a wrong one is rejected with
//! the edges the map holds instead of reaching the review.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::super::view::RepositoryView;

/// Edges a rejection names for one fact.
const SHOWN_EDGES: usize = 5;

/// That `from` imports `to`, or does not.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportFact {
    from: String,
    to: String,
    imports: bool,
}

/// What a fact's end names: a mapped file's path or `file:` ID, or a
/// module's name or `module:` ID; the files it stands for, and how to
/// say it.
fn files(view: &RepositoryView<'_>, name: &str) -> Option<(BTreeSet<usize>, String)> {
    let name = name.trim();
    if let Some(file) = view.file_by_path(name).or_else(|| view.file_by_id(name)) {
        return Some((
            BTreeSet::from([file]),
            format!("`{}`", view.file(file).path),
        ));
    }
    let module = view.modules.find(name)?;
    Some((
        view.modules.members(module).iter().copied().collect(),
        format!("module `{}`", view.modules.name(module)),
    ))
}

/// Every wrong or unknown fact among `claims`' (ref, facts), as one
/// rejection reason each.
pub(super) fn wrong_import_facts<'a>(
    claims: impl IntoIterator<Item = (&'a str, &'a [ImportFact])>,
    view: &RepositoryView<'_>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (reference, facts) in claims {
        for fact in facts {
            let (Some((from, from_name)), Some((to, to_name))) =
                (files(view, &fact.from), files(view, &fact.to))
            else {
                let unknown = [&fact.from, &fact.to]
                    .into_iter()
                    .filter(|name| files(view, name).is_none())
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>();
                errors.push(format!(
                    "claim `{reference}` states an import fact about {}, which names no mapped file or module; use a path or file: ID, or a module name or module: ID",
                    unknown.join(" and ")
                ));
                continue;
            };
            let edges = edges(view, &from, &to);
            match (fact.imports, edges.is_empty()) {
                (true, true) => errors.push(format!(
                    "claim `{reference}` states that {from_name} imports {to_name}, but the map has no import from it to it; correct the fact and the proposition, or drop the fact if a read shows an import the map misses"
                )),
                (false, false) => errors.push(format!(
                    "claim `{reference}` states that {from_name} does not import {to_name}, but the map has {}: {}",
                    if edges.len() == 1 { "one" } else { "these" },
                    edges.join(", ")
                )),
                _ => {}
            }
        }
    }
    errors
}

/// Up to [`SHOWN_EDGES`] import edges from a file in `from` to another file
/// in `to`, as `path:line -> path`, with a count of the rest.
fn edges(view: &RepositoryView<'_>, from: &BTreeSet<usize>, to: &BTreeSet<usize>) -> Vec<String> {
    let mut found = from
        .iter()
        .flat_map(|source| {
            view.edges_from(&view.file(*source).id, "imports")
                .into_iter()
                .filter_map(move |(_, edge)| {
                    let target = view.file_by_id(&edge.to)?;
                    (target != *source && to.contains(&target)).then_some((
                        &view.file(*source).path,
                        edge.line,
                        &view.file(target).path,
                    ))
                })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|(source, line, target)| match line {
            Some(line) => format!("{source}:{line} -> {target}"),
            None => format!("{source} -> {target}"),
        })
        .collect::<Vec<_>>();
    if found.len() > SHOWN_EDGES {
        let rest = found.len() - SHOWN_EDGES;
        found.truncate(SHOWN_EDGES);
        found.push(format!("and {rest} more"));
    }
    found
}

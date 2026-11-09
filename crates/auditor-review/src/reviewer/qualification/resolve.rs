//! Whether an evidence ID names something the review's inputs hold: a
//! finding in the report, a file, symbol, edge, or entry point in the map,
//! a module, dependency, or cycle the map's files make, a detector that
//! ran, a line range of a mapped file, or a file or co-change pair in the
//! history digest. Each prefix is the one the view issues.

use super::super::view::{co_change_id, source_span, RepositoryView};

type Resolver = fn(&RepositoryView<'_>, &str, &str) -> bool;

/// How each prefixed ID resolves: given the view, the whole ID, and the
/// part after the prefix.
const RESOLVERS: [(&str, Resolver); 10] = [
    ("finding", |view, _, rest| {
        view.findings.iter().any(|finding| finding.id == rest)
    }),
    ("edge", |view, _, rest| {
        index_below(rest, view.map.edges.len())
    }),
    ("entry", |view, _, rest| {
        index_below(rest, view.map.entry_points.len())
    }),
    ("module", |view, id, _| view.modules.find(id).is_some()),
    ("module-dependency", |view, _, rest| {
        view.holds_dependency(rest)
    }),
    ("module-cycle", |view, _, rest| {
        rest.parse::<usize>()
            .is_ok_and(|cycle| (1..=view.module_cycles.len()).contains(&cycle))
    }),
    ("detector", |view, _, rest| {
        view.detectors.iter().any(|run| run.id == rest)
    }),
    ("source-span", |view, id, _| view.holds_span(id)),
    ("history", |view, _, rest| {
        view.file_by_path(rest)
            .is_some_and(|file| view.file_history(file).is_some())
    }),
    ("co-change", |view, id, rest| view.holds_co_change(id, rest)),
];

impl RepositoryView<'_> {
    pub(super) fn holds_evidence(&self, id: &str) -> bool {
        if self.file_by_id(id).is_some() || self.symbol_by_id(id).is_some() {
            return true;
        }
        let Some((prefix, rest)) = id.split_once(':') else {
            return false;
        };
        RESOLVERS
            .iter()
            .find(|(known, _)| *known == prefix)
            .is_some_and(|(_, resolve)| resolve(self, id, rest))
    }

    fn holds_dependency(&self, pair: &str) -> bool {
        let Some((from, to)) = pair.split_once("->") else {
            return false;
        };
        match (self.modules.find(from), self.modules.find(to)) {
            (Some(from), Some(to)) => self.module_imports.contains_key(&(from, to)),
            _ => false,
        }
    }

    /// A source span within the mapped file's lines.
    fn holds_span(&self, id: &str) -> bool {
        let Some((path, start, end)) = source_span(id) else {
            return false;
        };
        self.file_by_path(path)
            .is_some_and(|file| start >= 1 && start <= end && end <= self.file(file).lines.max(1))
    }

    /// A pair the digest lists as changing together, in canonical order.
    fn holds_co_change(&self, id: &str, pair: &str) -> bool {
        let Some((left, right)) = pair.split_once('+') else {
            return false;
        };
        let partner = |path: &str, other: &str| {
            self.file_by_path(path)
                .and_then(|file| self.file_history(file))
                .is_some_and(|history| history.co_changes.iter().any(|change| change.path == other))
        };
        co_change_id(left, right) == id && (partner(left, right) || partner(right, left))
    }
}

fn index_below(text: &str, length: usize) -> bool {
    text.parse::<usize>().is_ok_and(|index| index < length)
}

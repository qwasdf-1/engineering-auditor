//! The kind and cardinality of a collection, from its declaration or the
//! value assigned to it.

use super::effects::assignment_root;
use super::*;
use crate::Language;

pub(super) fn collection_shape(
    file: &StructureFile,
    owner: u32,
    collection: &str,
) -> (CollectionKind, Cardinality) {
    if let Some(declaration) = file.symbols.iter().find_map(|symbol| {
        (matches!(symbol.kind, SymbolKind::Parameter | SymbolKind::Variable)
            && symbol.parent == Some(owner)
            && symbol.name == collection)
            .then_some(symbol.declaration.as_deref())
            .flatten()
    }) {
        return (
            value_kind(&file.language, declaration),
            Cardinality::UnknownScalable,
        );
    }
    if let Some(value) = file.assignments.iter().find_map(|assignment| {
        (assignment.owner == Some(owner)
            && assignment_root(assignment).as_deref() == Some(collection))
        .then_some(assignment.value.as_deref())
        .flatten()
    }) {
        let kind = value_kind(&file.language, value);
        let cardinality = literal_cardinality(value)
            .map(Cardinality::KnownBounded)
            .unwrap_or(Cardinality::UnknownScalable);
        return (kind, cardinality);
    }
    (CollectionKind::Unknown, Cardinality::UnknownScalable)
}

fn value_kind(language: &Language, text: &str) -> CollectionKind {
    python_display_kind(language, text).unwrap_or_else(|| collection_kind_from_text(text))
}

/// A Python set or dict display or comprehension, by its outer braces:
/// `{(a, b) for a in rows.get("k", [])}` is a set whatever it contains.
/// `text` may carry the assignment (`allow = {..}`).
fn python_display_kind(language: &Language, text: &str) -> Option<CollectionKind> {
    if *language != Language::Python {
        return None;
    }
    let value = text.trim();
    let value = match top_level(value, |ch, next| ch == '=' && next != Some('=')) {
        Some(index) if !value.starts_with('{') => value[index + 1..].trim(),
        _ => value,
    };
    let body = value.strip_prefix('{')?.strip_suffix('}')?;
    let pairs = body.trim().is_empty() || top_level(body, |ch, _| ch == ':').is_some();
    Some(if pairs {
        CollectionKind::Map
    } else {
        CollectionKind::Set
    })
}

/// The byte index of the first character outside brackets and quotes that
/// `matches` (with the next character).
fn top_level(text: &str, matches: impl Fn(char, Option<char>) -> bool) -> Option<usize> {
    let mut quotes = crate::util::QuoteState::default();
    let mut depth = 0usize;
    let mut characters = text.char_indices().peekable();
    while let Some((index, ch)) = characters.next() {
        if quotes.consumes(ch) {
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 && matches(ch, characters.peek().map(|(_, next)| *next)) => {
                return Some(index)
            }
            _ => {}
        }
    }
    None
}

pub(super) fn collection_kind_from_text(text: &str) -> CollectionKind {
    let lower = text.to_ascii_lowercase();
    if ["hashset", "btreeset", "set<", "set[", "new set", "set("]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        CollectionKind::Set
    } else if ["hashmap", "btreemap", "map<", "map[", "new map", "dict"]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        CollectionKind::Map
    } else if [
        "vec<", "vec::", "vec!", "list<", "list[", "array<", "[]", "slice", "sequence",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || lower.trim_start().starts_with('[')
    {
        CollectionKind::LinearSequence
    } else {
        CollectionKind::Unknown
    }
}

fn literal_cardinality(value: &str) -> Option<usize> {
    let compact = value.trim();
    let body = if let Some(body) = compact.strip_prefix('[').and_then(|v| v.split(']').next()) {
        body
    } else {
        compact
            .split_once("vec![")
            .and_then(|(_, rest)| rest.split(']').next())?
    };
    if body.contains("...") || body.contains("..") {
        return None;
    }
    Some(if body.trim().is_empty() {
        0
    } else {
        body.split(',').count()
    })
}

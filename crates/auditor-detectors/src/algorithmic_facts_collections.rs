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
    let declaration = file.symbols.iter().find_map(|symbol| {
        (matches!(symbol.kind, SymbolKind::Parameter | SymbolKind::Variable)
            && symbol.parent == Some(owner)
            && symbol.name == collection)
            .then_some(symbol.declaration.as_deref())
            .flatten()
    });
    let assigned = file.assignments.iter().find_map(|assignment| {
        (assignment.owner == Some(owner)
            && assignment_root(assignment).as_deref() == Some(collection))
        .then_some(assignment.value.as_deref())
        .flatten()
    });
    // `var allowed = new[] { A, B };` declares its literal; a declaration's
    // text may stop at its name, leaving the literal to the assignment.
    let initializer = declaration.and_then(|declaration| {
        top_level(declaration, |ch, next| ch == '=' && next != Some('='))
            .map(|index| &declaration[index + 1..])
    });
    let cardinality = initializer
        .or(assigned)
        .and_then(literal_cardinality)
        .map(Cardinality::KnownBounded)
        .unwrap_or(Cardinality::UnknownScalable);
    match declaration.or(assigned) {
        Some(text) => (value_kind(&file.language, text), cardinality),
        None => (CollectionKind::Unknown, Cardinality::UnknownScalable),
    }
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
        "vec<", "vec::", "vec!", "list<", "list[", "array<", "[]", "slice", "sequence", "vector<",
        "deque<",
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

/// The elements of a whole-value collection initializer: C# and Java
/// `new[] { .. }`, `new string[] { .. }`, `new List<T> { .. }`, and Java and
/// Kotlin `List.of(..)`, `Arrays.asList(..)`, `listOf(..)`, `arrayOf(..)`.
fn initializer_body(value: &str) -> Option<&str> {
    const FACTORIES: &[&str] = &["List.of(", "Arrays.asList(", "listOf(", "arrayOf("];
    let (open, close) = if value.starts_with("new ") || value.starts_with("new[") {
        ('{', '}')
    } else if FACTORIES.iter().any(|factory| value.starts_with(factory)) {
        ('(', ')')
    } else {
        return None;
    };
    let start = value.find(open)?;
    value
        .ends_with(close)
        .then(|| &value[start + 1..value.len() - 1])
        .filter(|body| !body.contains(open))
}

fn literal_cardinality(value: &str) -> Option<usize> {
    let compact = value.trim().trim_end_matches(';').trim_end();
    let body = if let Some(body) = compact.strip_prefix('[').and_then(|v| v.split(']').next()) {
        body
    } else if let Some((_, rest)) = compact.split_once("vec![") {
        rest.split(']').next()?
    } else {
        initializer_body(compact)?
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

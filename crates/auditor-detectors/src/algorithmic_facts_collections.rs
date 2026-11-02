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
    let (declaration, value) = local_value(file, owner, collection);
    let cardinality = value
        .and_then(literal_cardinality)
        .map(Cardinality::KnownBounded)
        .unwrap_or(Cardinality::UnknownScalable);
    match declaration.or(value) {
        Some(text) => (value_kind(&file.language, text), cardinality),
        None => (CollectionKind::Unknown, Cardinality::UnknownScalable),
    }
}
/// [`collection_shape`], with a constant declared elsewhere, read directly
/// or through a local (`List<String> names = NAMES`), taken as its literal
/// when no code can grow it (`static final List<String> NAMES = List.of(..)`).
pub(super) fn declared_collection_shape(
    file: &StructureFile,
    owner: u32,
    collection: &str,
    types: crate::type_facts::TypeScope<'_>,
) -> (CollectionKind, Cardinality) {
    match collection_shape(file, owner, collection) {
        (kind, Cardinality::UnknownScalable) => {
            let source = local_initializer(file, owner, collection).unwrap_or(collection);
            match types
                .resolve_text(owner as usize, source)
                .constant
                .as_deref()
                .and_then(constant_collection)
            {
                Some((constant_kind, bounded)) if kind == CollectionKind::Unknown => {
                    (constant_kind, bounded)
                }
                Some((_, bounded)) => (kind, bounded),
                None => (kind, Cardinality::UnknownScalable),
            }
        }
        shape => shape,
    }
}

/// The value a local of `owner` is bound to (`List<String> names = NAMES`).
pub(super) fn local_initializer<'f>(
    file: &'f StructureFile,
    owner: u32,
    collection: &str,
) -> Option<&'f str> {
    local_value(file, owner, collection).1
}

/// A local's declaration and the value bound to it: the declaration's
/// initializer (`var allowed = new[] { A, B };`), or, where a declaration's
/// text stops at its name, the value assigned to it.
fn local_value<'f>(
    file: &'f StructureFile,
    owner: u32,
    collection: &str,
) -> (Option<&'f str>, Option<&'f str>) {
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
    let initializer = declaration.and_then(|declaration| {
        top_level(declaration, |ch, next| ch == '=' && next != Some('='))
            .map(|index| declaration[index + 1..].trim())
    });
    (declaration, initializer.or(assigned))
}

/// The shape of a constant's initializer when it is a collection no code
/// can grow: an array, `List.of(..)`, `Arrays.asList(..)`, Kotlin
/// `listOf(..)`/`arrayOf(..)`, or a `[..]` literal. `new List<T> { .. }` and
/// other growable initializers stay unknown, since any method may add to them.
pub(super) fn constant_collection(initializer: &str) -> Option<(CollectionKind, Cardinality)> {
    let value = initializer.trim();
    let growable = value.starts_with("new ")
        && !value
            .split('{')
            .next()
            .is_some_and(|head| head.contains('['));
    if growable {
        return None;
    }
    let size = literal_cardinality(value)?;
    Some((
        CollectionKind::LinearSequence,
        Cardinality::KnownBounded(size),
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_collections_no_code_can_grow_are_bounded_constants() {
        for fixed in [
            "List.of(\"a\", \"b\")",
            "Arrays.asList(A, B, C)",
            "new[] { \"a\" }",
            "new String[] { a, b }",
            "[\"a\", \"b\"]",
            "listOf(1, 2)",
        ] {
            assert!(
                matches!(
                    constant_collection(fixed),
                    Some((_, Cardinality::KnownBounded(_)))
                ),
                "{fixed}"
            );
        }
        for growable in [
            "new ArrayList<>()",
            "new List<string> { \"a\" }",
            "new HashSet<String>(names)",
            "loadNames()",
        ] {
            assert_eq!(constant_collection(growable), None, "{growable}");
        }
    }
}

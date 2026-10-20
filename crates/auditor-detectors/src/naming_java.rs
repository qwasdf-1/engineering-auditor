use super::*;

pub(super) fn findings(text: &str, relative: &str) -> Vec<Finding> {
    text.lines()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let code = util::strip_comments(line);
            let name = static_final_name(&code)?;
            (!is_upper_snake(name)).then(|| {
                naming_finding(
                    NamingKind::Constant,
                    "UPPER_SNAKE_CASE",
                    name,
                    relative,
                    line_number,
                )
            })
        })
        .collect()
}

fn static_final_name(code: &str) -> Option<&str> {
    if !code.contains("final ") || !code.contains("static ") || code.contains(['(', '{']) {
        return None;
    }
    let (declaration, _) = code.split_once('=')?;
    let name = declaration.split_whitespace().next_back()?;
    // Java serialization requires the exact name `serialVersionUID`.
    (name.len() > 2 && name != "serialVersionUID" && !holds_mutable_state(declaration))
        .then_some(name)
}

/// Google Java Style §5.2.4: a constant is a `static final` field whose
/// contents are deeply immutable, so a mutable collection, array, builder,
/// atomic, or logger in a `static final` field keeps lowerCamelCase.
fn holds_mutable_state(declaration: &str) -> bool {
    const MUTABLE_TYPES: &[&str] = &[
        "Collection",
        "List",
        "Set",
        "Map",
        "Queue",
        "Deque",
        "SortedMap",
        "SortedSet",
        "NavigableMap",
        "NavigableSet",
        "ConcurrentMap",
        "ConcurrentHashMap",
        "HashMap",
        "HashSet",
        "ArrayList",
        "LinkedList",
        "TreeMap",
        "TreeSet",
        "LinkedHashMap",
        "LinkedHashSet",
        "ArrayDeque",
        "EnumMap",
        "Vector",
        "Hashtable",
        "CopyOnWriteArrayList",
        "Properties",
        "StringBuilder",
        "Logger",
    ];
    const MODIFIERS: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "final",
        "transient",
        "volatile",
    ];
    let Some(type_name) = declaration
        .split_whitespace()
        .find(|token| !MODIFIERS.contains(token) && !token.starts_with('@'))
    else {
        return false;
    };
    let base = type_name.split(['<', '[']).next().unwrap_or(type_name);
    let base = base.rsplit('.').next().unwrap_or(base);
    declaration.contains("[]") || base.starts_with("Atomic") || MUTABLE_TYPES.contains(&base)
}

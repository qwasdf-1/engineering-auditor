use super::*;

pub(super) fn is_default_sort(method: &str, arguments: &str) -> bool {
    // Java's `list.sort(null)` sorts by natural order.
    matches!(method, "sort" | "sort_unstable") && matches!(arguments.trim(), "" | "null")
}

pub(super) fn is_terminal_traversal(method: &str) -> bool {
    matches!(
        method,
        "any"
            | "all"
            | "find"
            | "position"
            | "count"
            | "sum"
            | "min"
            | "max"
            | "some"
            | "every"
            | "reduce"
            | "anyMatch"
            | "allMatch"
            | "noneMatch"
    )
}

pub(super) fn pure_traversal_arguments(arguments: &str) -> bool {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return true;
    }
    // A Go function literal (`func(i Item) bool { return i.ready }`) or a C++
    // lambda (`[](const Item& i) { return i.ready; }`): judge its body.
    if let Some(body) = trimmed
        .strip_prefix("func(")
        .or_else(|| trimmed.strip_prefix('['))
        .and_then(|rest| rest.split_once('{'))
        .and_then(|(_, body)| body.trim_end().strip_suffix('}'))
    {
        let expression = body.trim().strip_prefix("return ").unwrap_or(body).trim();
        let expression = expression.strip_suffix(';').unwrap_or(expression);
        return !expression.contains(['\n', ';'])
            && !expression.contains(" = ")
            && !expression.contains("<-")
            && !contains_nested_call(expression);
    }
    if !trimmed.contains('|') && !trimmed.contains("=>") && !trimmed.contains("->") {
        return false;
    }
    let normalized = trimmed.replace("=>", "").replace("->", "");
    !normalized.contains("await ")
        && !normalized.contains("yield ")
        && !normalized.contains("++")
        && !normalized.contains("--")
        && !normalized.contains(" = ")
        && !contains_nested_call(&normalized)
}

fn contains_nested_call(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == b'('
            && index > 0
            && (bytes[index - 1].is_ascii_alphanumeric() || matches!(bytes[index - 1], b'_' | b')'))
    })
}

/// C# `List<T>` and LINQ methods, by the name the other languages' tables
/// use, and whether the C# form needs arguments to mean it: `Any()` and
/// `Count()` without a predicate read one element or a stored count, and
/// `First(p)` searches rather than selects. `Remove(x)` removes a value
/// (`discard`); `RemoveAt(i)` removes by index, as Java's `remove(i)` does.
const CSHARP_METHOD_ALIASES: &[(&str, &str, Option<bool>)] = &[
    ("Sort", "sort", None),
    ("Reverse", "reverse", None),
    ("Add", "add", None),
    ("AddRange", "addAll", None),
    ("Insert", "insert", None),
    ("InsertRange", "addAll", None),
    ("Remove", "discard", None),
    ("RemoveAt", "remove", None),
    ("RemoveAll", "removeIf", None),
    ("RemoveRange", "removeAll", None),
    ("Clear", "clear", None),
    ("Contains", "contains", None),
    ("Any", "any", Some(true)),
    ("All", "all", None),
    ("Exists", "any", None),
    ("TrueForAll", "all", None),
    ("Count", "count", Some(true)),
    ("Sum", "sum", None),
    ("Min", "min", None),
    ("Max", "max", None),
    ("Aggregate", "reduce", None),
    ("Find", "find", None),
    ("FindIndex", "position", None),
    ("First", "first", Some(false)),
    ("Last", "last", Some(false)),
    ("Take", "take", None),
    ("GetRange", "slice", None),
];

/// The method name the collection tables use for `method`.
pub(super) fn canonical_method(method: String, arguments: &str) -> String {
    CSHARP_METHOD_ALIASES
        .iter()
        .find(|(name, _, needs_arguments)| {
            *name == method
                && needs_arguments.is_none_or(|needed| needed != arguments.trim().is_empty())
        })
        .map_or(method, |(_, canonical, _)| canonical.to_string())
}

/// Java and C++ names for the selections other languages spell `first`,
/// `last`, and `slice`.
const JAVA_SELECTION_ALIASES: &[(&str, &str)] = &[
    ("getFirst", "first"),
    ("getLast", "last"),
    ("subList", "slice"),
    ("front", "first"),
    ("back", "last"),
];

pub(super) fn selection_kind(method: &str, arguments: &str) -> Option<SelectionKind> {
    let method = JAVA_SELECTION_ALIASES
        .iter()
        .find(|(alias, _)| *alias == method)
        .map_or(method, |(_, canonical)| canonical);
    match method {
        "first" => Some(SelectionKind::Extremum {
            endpoint: "first".to_string(),
        }),
        "last" => Some(SelectionKind::Extremum {
            endpoint: "last".to_string(),
        }),
        "get" => indexed_selection(arguments),
        "at" => match arguments.trim() {
            "0" => Some(SelectionKind::Extremum {
                endpoint: "first".to_string(),
            }),
            "-1" => Some(SelectionKind::Extremum {
                endpoint: "last".to_string(),
            }),
            _ => None,
        },
        "take" => positive_integer(arguments).map(|k| {
            if k == 1 {
                SelectionKind::Extremum {
                    endpoint: "first".to_string(),
                }
            } else {
                SelectionKind::TopK { k }
            }
        }),
        "slice" => {
            let mut parts = arguments.split(',').map(str::trim);
            let start = parts.next()?;
            let k = positive_integer(parts.next()?)?;
            (start == "0" && k > 1).then_some(SelectionKind::TopK { k })
        }
        _ => None,
    }
}

/// Java lists select by index: `get(0)` is the first, `get(xs.size() - 1)`
/// the last element.
fn indexed_selection(arguments: &str) -> Option<SelectionKind> {
    let index = arguments.replace(' ', "");
    let endpoint = if index == "0" {
        "first"
    } else if index.ends_with(".size()-1") {
        "last"
    } else {
        return None;
    };
    Some(SelectionKind::Extremum {
        endpoint: endpoint.to_string(),
    })
}

fn positive_integer(text: &str) -> Option<usize> {
    let value = text.trim().parse::<usize>().ok()?;
    (value > 0).then_some(value)
}

pub(super) fn is_mutator(method: &str) -> bool {
    matches!(
        method,
        "push"
            | "append"
            | "insert"
            | "remove"
            | "clear"
            | "extend"
            | "retain"
            | "set"
            | "add"
            | "sort"
            | "sort_by"
            | "sort_unstable"
            | "reverse"
            | "shuffle"
            | "addAll"
            | "removeAll"
            | "retainAll"
            | "removeIf"
            | "replaceAll"
            | "discard"
            // C++ containers.
            | "push_back"
            | "emplace_back"
            | "push_front"
            | "pop_back"
            | "pop_front"
            | "emplace"
            | "erase"
            | "resize"
            | "assign"
            | "swap"
    )
}

pub(super) fn root_identifier(text: &str) -> Option<String> {
    util::identifiers(text)
        .into_iter()
        .find(|identifier| !is_keyword(identifier))
}

pub(super) fn exact_identifier(text: &str) -> Option<String> {
    let identifiers = util::identifiers(text)
        .into_iter()
        .filter(|identifier| !is_keyword(identifier))
        .collect::<Vec<_>>();
    (identifiers.len() == 1 && !text.contains(['.', '(', '[', '{']) && !text.contains("::"))
        .then(|| identifiers[0].clone())
}

pub(super) fn contains_identifier(text: &str, identifier: &str) -> bool {
    util::identifiers(text)
        .iter()
        .any(|item| item == identifier)
}

pub(super) fn trim_arguments(arguments: &str) -> String {
    let trimmed = arguments.trim();
    trimmed
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

pub(super) fn is_keyword(identifier: &str) -> bool {
    matches!(
        identifier,
        "_" | "for"
            | "while"
            | "let"
            | "mut"
            | "ref"
            | "const"
            | "var"
            | "in"
            | "of"
            | "await"
            | "self"
            | "this"
    )
}

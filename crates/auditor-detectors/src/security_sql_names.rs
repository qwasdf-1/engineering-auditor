//! Whether a built SQL query's dynamic parts only shape its structure. A
//! value spliced where SQL expects a table, schema, or column name
//! (`"FROM " + table`, `"\"%s\".tgt_%s".formatted(schema, name)`) cannot be a
//! bound parameter, and one spliced where SQL expects a clause or a list
//! (`" WHERE " + buildCondition()`, `"FROM (%s)"`, `" (" + columns + ")"`)
//! is a fragment a query builder composed; unless it comes from request
//! input, it is not injection. A local whose every reaching value is a
//! literal (`final String filters = """..."""`) is a constant; a number
//! (`strconv.Itoa(n)`, `int.Parse(s)`, a `long` parameter, a `%d`
//! placeholder, which every format call rejects or mangles for text), a
//! generated placeholder list (`ids.map(() => '?').join(',')`), or a quoted
//! identifier (`pq.QuoteIdentifier(f)`) is safe; and a local built once is read through
//! (`where = "id = " + id; .. " WHERE " + where`). A slot in value position
//! (inside `'..'`, after `=`, `LIKE`, `IN`, `LIMIT`, or `ORDER BY`), or any
//! slot taken from the request, keeps a query an injection site.

use auditor_ast::{Assigned, AstNode, FunctionFlow, Origin};

use crate::dataflow_sources::{callee_name, from_request};
use crate::security_sql_templates::{
    concatenation, format_template, percent_template, placeholder_length, Format, STRING_KINDS,
};

/// Stands for one runtime value in the query's text.
const SLOT: char = '\u{1}';

/// SQL keywords whose next word names a table or schema.
const NAME_KEYWORDS: &[&str] = &[
    "from", "join", "into", "insert", "update", "table", "truncate", "exists",
];

/// SQL keywords whose next word is a value: a pattern, a list, a count, or
/// a sort key.
const VALUE_KEYWORDS: &[&str] = &[
    "like", "ilike", "regexp", "in", "limit", "offset", "by", "between", "top",
];

/// Calls whose result cannot carry SQL: numbers converted to text and
/// identifiers or literals quoted for SQL.
const SAFE_CALLS: &[&str] = &[
    "Itoa",
    "FormatInt",
    "FormatUint",
    "FormatFloat",
    "FormatBool",
    "int",
    "float",
    "len",
    "QuoteIdentifier",
    "QuoteLiteral",
    "quote_ident",
    "quote_literal",
    "quote_identifier",
    "quoteIdentifier",
    "escapeId",
    "escapeIdentifier",
    "escape_identifier",
    "QuoteName",
    "nameof",
    "parseInt",
    "parseLong",
    "parseDouble",
    "Atoi",
    "ParseInt",
    "ParseUint",
    "ParseFloat",
    "ToInt32",
    "ToInt64",
];

/// Types whose `Parse` result is a number or a GUID, never SQL text.
const PARSED_TYPES: &[&str] = &[
    "int", "long", "short", "uint", "ulong", "decimal", "double", "Int32", "Int64", "Guid",
];

/// Declared types whose values render as a number, a boolean, or a GUID.
const NUMERIC_TYPES: &[&str] = &[
    "int", "long", "short", "byte", "uint", "ulong", "ushort", "double", "float", "decimal",
    "bool", "boolean", "Integer", "Long", "Short", "Byte", "Double", "Float", "Boolean", "Guid",
    "int32", "int64", "uint32", "uint64", "float64", "usize", "isize", "i32", "i64", "u32", "u64",
];

/// Calls that join a list into a query fragment.
const JOIN_CALLS: &[&str] = &["join", "Join", "Repeat", "repeat"];

/// Conversions of a `%` placeholder that only accept numbers.
const NUMERIC_CONVERSIONS: &str = "dixXobeEfFgG";

/// How many locals deep a query's text is read through.
const MAX_INLINE_DEPTH: usize = 4;

/// Children holding a string literal's text.
const CONTENT_KINDS: &[&str] = &[
    "string_fragment",
    "multiline_string_fragment",
    "string_content",
    "string_literal_content",
    "interpreted_string_literal_content",
    "raw_string_literal_content",
    "escape_sequence",
];

/// Children holding an interpolated value.
const INTERPOLATION_KINDS: &[&str] = &[
    "template_substitution",
    "interpolation",
    "interpolated_identifier",
    "interpolated_expression",
];

/// The query's text with a [`SLOT`] per runtime value, and the
/// expressions each slot may hold: the value itself, or a format call's
/// arguments for its placeholders.
#[derive(Default)]
struct Skeleton<'a> {
    text: String,
    slots: Vec<Vec<AstNode<'a>>>,
    /// Locals being read through.
    depth: usize,
}

impl<'a> Skeleton<'a> {
    fn slot(&mut self, values: Vec<AstNode<'a>>) {
        self.text.push(SLOT);
        self.slots.push(values);
    }
}

/// Whether every runtime value in `query` only shapes its structure: in a
/// name, clause, or list position and not from request input. True too when
/// none remains once constants and safe conversions are read as text.
pub(crate) fn structure_only(query: AstNode<'_>) -> bool {
    let mut skeleton = Skeleton::default();
    build(query, Format::Plain, &[], &mut skeleton);
    skeleton.structural()
}

/// [`structure_only`] for a query held in a local, given the values bound
/// to it: each whole value with every part appended after it.
pub(crate) fn assigned_structure_only(parts: &[Assigned<'_>]) -> bool {
    let appended = parts
        .iter()
        .filter_map(|part| match part {
            Assigned::Appended(value) => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let wholes = parts
        .iter()
        .filter(|part| !matches!(part, Assigned::Appended(_)))
        .map(Assigned::node)
        .collect::<Vec<_>>();
    let starts = if wholes.is_empty() {
        vec![None]
    } else {
        wholes.into_iter().map(Some).collect()
    };
    starts.into_iter().all(|whole| {
        let mut skeleton = Skeleton::default();
        whole
            .into_iter()
            .chain(appended.iter().copied())
            .for_each(|part| build(part, Format::Plain, &[], &mut skeleton));
        skeleton.structural()
    })
}

impl Skeleton<'_> {
    fn structural(&self) -> bool {
        self.text
            .match_indices(SLOT)
            .zip(&self.slots)
            .all(|((at, _), values)| {
                is_structure_position(&self.text, at)
                    && !values.iter().any(|value| from_request(*value))
            })
    }
}

/// Appends `node`'s query text to `out`. `arguments` are the values a
/// format template's placeholders take.
fn build<'a>(node: AstNode<'a>, format: Format, arguments: &[AstNode<'a>], out: &mut Skeleton<'a>) {
    if STRING_KINDS.contains(&node.kind()) {
        string_text(node, format, arguments, out);
    } else if let Some((template, format, arguments)) = format_template(node) {
        build(template, format, &arguments, out);
    } else if let Some((left, right)) = concatenation(node) {
        build(left, format, arguments, out);
        build(right, format, arguments, out);
    } else if let Some((template, values)) = percent_template(node) {
        build(template, Format::Percent, &values, out);
    } else if let Some(value) = node
        .named_children()
        .last()
        .filter(|_| matches!(node.kind(), "argument" | "value_argument"))
    {
        build(value, format, arguments, out);
    } else if matches!(
        node.kind(),
        "parenthesized_expression" | "concatenated_string"
    ) {
        node.named_children()
            .for_each(|child| build(child, format, arguments, out));
    } else if is_fixed(node) {
        out.text.push('c');
    } else if let Some(parts) = single_assignment(node).filter(|_| out.depth < MAX_INLINE_DEPTH) {
        out.depth += 1;
        parts
            .into_iter()
            .for_each(|part| build(part, format, arguments, out));
        out.depth -= 1;
    } else {
        out.slot(vec![node]);
    }
}

/// The values a local is built from, in order, when one whole value reaches
/// it (`where = "id = " + id`, then any `where += ..`).
fn single_assignment(node: AstNode<'_>) -> Option<Vec<AstNode<'_>>> {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return None;
    }
    let parts = FunctionFlow::enclosing(node).assigned(node);
    match parts.first() {
        Some(Assigned::Whole(_))
            if parts[1..]
                .iter()
                .all(|part| matches!(part, Assigned::Appended(_))) =>
        {
            Some(parts.iter().map(Assigned::node).collect())
        }
        _ => None,
    }
}

/// A value that cannot carry SQL text: a literal, a constant, a number, or
/// a generated placeholder list.
fn is_fixed(node: AstNode<'_>) -> bool {
    !is_runtime_value(node)
        || is_constant(node)
        || is_safe_call(node)
        || is_numeric(node)
        || is_placeholder_list(node)
}

fn is_call(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "call" | "call_expression" | "method_invocation" | "invocation_expression"
    )
}

/// A call whose result is a number's text or a quoted identifier.
fn is_safe_call(node: AstNode<'_>) -> bool {
    if !is_call(node) {
        return false;
    }
    let name = callee_name(node);
    SAFE_CALLS.contains(&name)
        || matches!(name, "Parse" | "parse")
            && crate::dataflow_sources::callee_text(node)
                .rsplit_once('.')
                .is_some_and(|(owner, _)| PARSED_TYPES.contains(&owner))
}

/// A name every reaching binding declares with a numeric type (`long
/// sessionId`, `int count = ..`).
fn is_numeric(node: AstNode<'_>) -> bool {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    let bindings = FunctionFlow::enclosing(node).reaching(node);
    !bindings.is_empty()
        && bindings.iter().all(|binding| {
            let declared = binding.node.child_by_field_name("type").or_else(|| {
                binding
                    .node
                    .parent()
                    .and_then(|declaration| declaration.child_by_field_name("type"))
            });
            declared.is_some_and(|declared| {
                NUMERIC_TYPES.contains(&declared.text().trim_end_matches('?'))
            })
        })
}

/// A list of bind placeholders joined into the query (`ids.map(() =>
/// '?').join(',')`, `string.Join(", ", ids.Select((_, i) => $"@p{i}"))`,
/// `strings.Repeat("?,", n)`): its only text is placeholders and separators.
fn is_placeholder_list(node: AstNode<'_>) -> bool {
    if !is_call(node) || !JOIN_CALLS.contains(&callee_name(node)) {
        return false;
    }
    let mut texts = Vec::new();
    literal_texts(node, &mut texts);
    texts.iter().any(|text| text.contains(['?', '@']))
        && texts.iter().all(|text| {
            text.chars().all(|character| {
                character.is_whitespace() || "?@$:,0123456789".contains(character)
            }) || text.starts_with('@') && text[1..].chars().all(char::is_alphanumeric)
        })
}

/// The literal text of every string in `node`, interpolations left out.
fn literal_texts<'a>(node: AstNode<'a>, out: &mut Vec<&'a str>) {
    if CONTENT_KINDS.contains(&node.kind()) {
        out.push(node.text());
    }
    if !INTERPOLATION_KINDS.contains(&node.kind()) {
        node.children().for_each(|child| literal_texts(child, out));
    }
}

fn string_text<'a>(
    node: AstNode<'a>,
    format: Format,
    arguments: &[AstNode<'a>],
    out: &mut Skeleton<'a>,
) {
    for child in node.named_children() {
        if INTERPOLATION_KINDS.contains(&child.kind()) {
            let value = child
                .named_children()
                .find(|part| !part.kind().contains("brace") && !part.kind().contains("format"))
                .unwrap_or(child);
            build(value, Format::Plain, &[], out);
        } else if child.kind() == "escape_sequence" {
            out.text.extend(child.text().chars().nth(1));
        } else if CONTENT_KINDS.contains(&child.kind()) {
            push_text(child.text(), format, arguments, out);
        }
    }
}

/// Copies `text`, replacing each format placeholder with a [`SLOT`] that
/// may hold any of `arguments`.
fn push_text<'a>(text: &str, format: Format, arguments: &[AstNode<'a>], out: &mut Skeleton<'a>) {
    let (open, escaped) = match format {
        Format::Plain => return out.text.push_str(text),
        Format::Percent => ('%', "%%"),
        Format::Brace => ('{', "{{"),
    };
    let mut rest = text;
    while let Some(at) = rest.find(open) {
        out.text.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with(escaped) {
            out.text.push(open);
            rest = &rest[2..];
            continue;
        }
        let length = placeholder_length(rest, format);
        let numeric = format == Format::Percent
            && rest[..length].ends_with(|conversion| NUMERIC_CONVERSIONS.contains(conversion));
        if length > 0 && numeric {
            out.text.push('c');
        } else if length > 0 {
            out.slot(arguments.to_vec());
        } else {
            out.text.push(open);
        }
        rest = &rest[length.max(1)..];
    }
    out.text.push_str(rest);
}

/// A name whose every reaching value is a literal (`final String filters =
/// """..."""`, `clause = escape ? " ESCAPE '\\'" : ""`).
fn is_constant(node: AstNode<'_>) -> bool {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    FunctionFlow::enclosing(node).only_from(node, |origin| matches!(origin, Origin::Literal(_)))
}

/// Whether the slot at byte `at` of `skeleton` shapes the query rather
/// than supplying a value: not inside `'..'` and not after a comparison or
/// a keyword that takes a value. Names (quoted as identifiers, joined to a
/// name like `tgt_%s`, or after `FROM`) are structure even after `BY`.
fn is_structure_position(skeleton: &str, at: usize) -> bool {
    let before = &skeleton[..at];
    let after = &skeleton[at + SLOT.len_utf8()..];
    let word =
        |character: char| character.is_alphanumeric() || character == '_' || character == SLOT;
    let open = before.trim_end_matches(word).chars().next_back();
    let close = after.trim_start_matches(word).chars().next();
    if matches!(
        (open, close),
        (Some('"'), Some('"')) | (Some('`'), Some('`')) | (Some('['), Some(']'))
    ) {
        return true;
    }
    if before.matches('\'').count() % 2 == 1 {
        return false;
    }
    let joined = |character: Option<char>| {
        character.is_some_and(|character| word(character) || character == '.')
    };
    if joined(before.chars().next_back()) || joined(after.chars().next()) {
        return true;
    }
    let trimmed = before.trim_end();
    let keyword = trimmed[trimmed.trim_end_matches(word).len()..].to_ascii_lowercase();
    if NAME_KEYWORDS.contains(&keyword.as_str()) {
        return true;
    }
    match trimmed.chars().next_back() {
        Some('=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%') => false,
        Some('(') => {
            let inner = trimmed[..trimmed.len() - 1].trim_end();
            let keyword = &inner[inner.trim_end_matches(word).len()..];
            !VALUE_KEYWORDS.contains(&keyword.to_ascii_lowercase().as_str())
        }
        _ => !VALUE_KEYWORDS.contains(&keyword.as_str()),
    }
}

/// An operand that is not a literal, a constant (`TABLE`, `Columns.ID`), or
/// a nested concatenation.
pub(crate) fn is_runtime_value(operand: auditor_ast::AstNode<'_>) -> bool {
    if operand.kind().contains("string")
        || operand.kind().ends_with("literal")
        || matches!(
            operand.kind(),
            "binary_expression" | "parenthesized_expression" | "number"
        )
    {
        return false;
    }
    let last = operand.text().rsplit('.').next().unwrap_or_default();
    !(last.chars().any(char::is_alphabetic)
        && last.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        }))
}

#[cfg(test)]
#[path = "security_sql_names_tests.rs"]
mod tests;

//! Whether a built SQL query's dynamic parts are only configured names. A
//! value spliced where SQL expects a table, schema, or column name
//! (`"FROM " + table`, `"\"%s\".tgt_%s".formatted(schema, name)`) cannot be a
//! bound parameter; unless it comes from request input, it is configuration.
//! A local whose every reaching value is a literal (`final String filters =
//! """..."""`) is a constant. A slot in value position (after `=`, inside
//! `'..'`, in a list), or a name taken from the request, keeps a query an
//! injection site.

use auditor_ast::{call_arguments, AstNode, CallFlow, FunctionFlow, Origin};

use crate::ast_scan;
use crate::dataflow_sources::from_request;

/// Stands for one runtime value in the query's text.
const SLOT: char = '\u{1}';

/// SQL keywords whose next word names a table or schema.
const NAME_KEYWORDS: &[&str] = &[
    "from", "join", "into", "update", "table", "truncate", "exists",
];

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

/// Literal string kinds, which may hold interpolations.
const STRING_KINDS: &[&str] = &[
    "string",
    "string_literal",
    "template_string",
    "interpolated_string_expression",
    "interpreted_string_literal",
    "raw_string_literal",
    "verbatim_string_literal",
];

/// How a string's text marks the values a format call substitutes.
#[derive(Clone, Copy, PartialEq)]
enum Format {
    Plain,
    /// `%s`, `%1$s`, `%(name)s`.
    Percent,
    /// `{}`, `{0}`, `{name}`.
    Brace,
}

/// The query's text with a [`SLOT`] per runtime value, and the
/// expressions each slot may hold: the value itself, or a format call's
/// arguments for its placeholders.
#[derive(Default)]
struct Skeleton<'a> {
    text: String,
    slots: Vec<Vec<AstNode<'a>>>,
}

impl<'a> Skeleton<'a> {
    fn slot(&mut self, values: Vec<AstNode<'a>>) {
        self.text.push(SLOT);
        self.slots.push(values);
    }
}

/// Whether every runtime value in `query` is a configured name: in a name
/// position and not from request input. True too when none remains once
/// literal-bound locals are read as constants.
pub(crate) fn names_only(query: AstNode<'_>) -> bool {
    let mut skeleton = Skeleton::default();
    build(query, Format::Plain, &[], &mut skeleton);
    skeleton
        .text
        .match_indices(SLOT)
        .zip(&skeleton.slots)
        .all(|((at, _), values)| {
            is_name_position(&skeleton.text, at) && !values.iter().any(|value| from_request(*value))
        })
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
    } else if !is_runtime_value(node) || is_constant(node) {
        out.text.push('c');
    } else {
        out.slot(vec![node]);
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
            out.slot(vec![value]);
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
        if length > 0 {
            out.slot(arguments.to_vec());
        } else {
            out.text.push(open);
        }
        rest = &rest[length.max(1)..];
    }
    out.text.push_str(rest);
}

/// The byte length of the placeholder opening `text`, or 0 for none.
fn placeholder_length(text: &str, format: Format) -> usize {
    if format == Format::Brace {
        return text.find('}').map_or(0, |close| close + 1);
    }
    let mut length = 1;
    if text[1..].starts_with('(') {
        let Some(close) = text.find(')') else {
            return 0;
        };
        length = close + 1;
    }
    length += text[length..]
        .chars()
        .take_while(|character| "0123456789$-+# .".contains(*character))
        .count();
    match text[length..].chars().next() {
        Some(conversion) if "sdifxXoegcbrSaA".contains(conversion) => length + 1,
        _ => 0,
    }
}

/// A format call's template, its placeholder style, and the values it
/// substitutes: Java `"..".formatted(..)` and `String.format("..", ..)`,
/// Python and Kotlin `"..".format(..)`, C# `string.Format("..", ..)`, Go
/// `fmt.Sprintf("..", ..)`.
fn format_template(node: AstNode<'_>) -> Option<(AstNode<'_>, Format, Vec<AstNode<'_>>)> {
    let (path, receiver) = match node.kind() {
        "method_invocation" => {
            let receiver = node.child_by_field_name("object");
            let name = node.child_by_field_name("name")?.text();
            let owner = receiver.map(|receiver| receiver.text()).unwrap_or_default();
            (format!("{owner}.{name}"), receiver)
        }
        "call" | "call_expression" | "invocation_expression" => {
            let callee = node
                .child_by_field_name("function")
                .or(node.named_child(0))?;
            let receiver = callee.child_by_field_name("object").or_else(|| {
                (callee.kind() == "navigation_expression").then(|| callee.named_child(0))?
            });
            (callee.text().to_string(), receiver)
        }
        _ => return None,
    };
    let template = receiver.filter(|receiver| STRING_KINDS.contains(&receiver.kind()));
    let brace = node.language() == auditor_ast::AstLang::Python;
    let arguments = call_arguments(node);
    let after_template = || arguments.get(1..).unwrap_or_default().to_vec();
    match (path.rsplit('.').next().unwrap_or_default(), template) {
        ("formatted", Some(template)) => Some((template, Format::Percent, arguments.clone())),
        ("format", Some(template)) if brace => Some((template, Format::Brace, arguments.clone())),
        ("format", Some(template)) => Some((template, Format::Percent, arguments.clone())),
        _ if matches!(
            path.as_str(),
            "String.format" | "java.lang.String.format" | "fmt.Sprintf"
        ) =>
        {
            Some((first_argument(node)?, Format::Percent, after_template()))
        }
        _ if matches!(path.as_str(), "string.Format" | "String.Format") => {
            Some((first_argument(node)?, Format::Brace, after_template()))
        }
        _ => None,
    }
}

/// The operands of a `+` concatenation.
fn concatenation(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    if !matches!(
        node.kind(),
        "binary_expression" | "binary_operator" | "additive_expression"
    ) || operator(node) != Some("+")
    {
        return None;
    }
    Some((node.named_child(0)?, node.named_child(1)?))
}

/// Python's `template % values`, with the values substituted.
fn percent_template(node: AstNode<'_>) -> Option<(AstNode<'_>, Vec<AstNode<'_>>)> {
    if node.kind() != "binary_operator" || operator(node) != Some("%") {
        return None;
    }
    let values = node.named_child(1)?;
    let values = if values.kind() == "tuple" {
        values.named_children().collect()
    } else {
        vec![values]
    };
    Some((node.named_child(0)?, values))
}

fn operator<'a>(node: AstNode<'a>) -> Option<&'a str> {
    node.child_by_field_name("operator")
        .or_else(|| node.children().find(|child| !child.is_named()))
        .map(|operator| operator.text())
}

fn first_argument(call: AstNode<'_>) -> Option<AstNode<'_>> {
    let arguments = call
        .child_by_field_name("arguments")
        .or_else(|| call.children().find(|child| child.kind() == "call_suffix"))?;
    let first = ast_scan::nodes_of_kind(arguments, &["argument", "value_argument"])
        .into_iter()
        .min_by_key(|argument| argument.start_byte())
        .or_else(|| arguments.named_children().next())?;
    if matches!(first.kind(), "argument" | "value_argument") {
        first.named_children().last()
    } else {
        Some(first)
    }
}

/// A name whose every reaching value is a literal (`final String filters =
/// """..."""`, `clause = escape ? " ESCAPE '\\'" : ""`).
fn is_constant(node: AstNode<'_>) -> bool {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    let origins = FunctionFlow::enclosing(node).origins(node, &|_| CallFlow::Opaque);
    !origins.is_empty()
        && origins
            .iter()
            .all(|origin| matches!(origin, Origin::Literal(_)))
}

/// Whether the slot at byte `at` of `skeleton` names a table, schema, or
/// column: quoted as an identifier, joined to a name (`tgt_%s`, `s.%s`), or
/// after a keyword that takes a table name.
fn is_name_position(skeleton: &str, at: usize) -> bool {
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
    let keyword = &trimmed[trimmed.trim_end_matches(word).len()..];
    NAME_KEYWORDS.contains(&keyword.to_ascii_lowercase().as_str())
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

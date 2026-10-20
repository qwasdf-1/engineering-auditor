//! Whether a built SQL query's dynamic parts are only names. A value spliced
//! where SQL expects a table, schema, or column name (`"FROM " + table`,
//! `"\"%s\".tgt_%s".formatted(schema, name)`) cannot be a bound parameter,
//! and it is configuration, not request input, in the code that does it; a
//! local bound to a string literal (`final String filters = """..."""`) is a
//! constant. Only a slot in value position (after `=`, inside `'..'`, in a
//! list) keeps a query an injection site.

use auditor_ast::AstNode;

use crate::ast_scan;

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

/// Functions and methods, the scope a local's bindings are read in.
const FUNCTION_KINDS: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "function_declaration",
    "function_definition",
    "method_definition",
    "local_function_statement",
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

/// Whether every runtime value in `query` sits in a name position, or none
/// remains once literal-bound locals are read as constants.
pub(crate) fn names_only(query: AstNode<'_>) -> bool {
    let mut skeleton = String::new();
    build(query, Format::Plain, &mut skeleton);
    skeleton
        .match_indices(SLOT)
        .all(|(at, _)| is_name_position(&skeleton, at))
}

/// Appends `node`'s query text to `out`, with a [`SLOT`] per runtime value.
fn build(node: AstNode<'_>, format: Format, out: &mut String) {
    if STRING_KINDS.contains(&node.kind()) {
        string_text(node, format, out);
    } else if let Some((template, format)) = format_template(node) {
        build(template, format, out);
    } else if let Some((left, right)) = concatenation(node) {
        build(left, format, out);
        build(right, format, out);
    } else if let Some(template) = percent_template(node) {
        build(template, Format::Percent, out);
    } else if let Some(value) = node
        .named_children()
        .last()
        .filter(|_| matches!(node.kind(), "argument" | "value_argument"))
    {
        build(value, format, out);
    } else if matches!(
        node.kind(),
        "parenthesized_expression" | "concatenated_string"
    ) {
        node.named_children()
            .for_each(|child| build(child, format, out));
    } else if !is_runtime_value(node) || is_literal_local(node) {
        out.push('c');
    } else {
        out.push(SLOT);
    }
}

fn string_text(node: AstNode<'_>, format: Format, out: &mut String) {
    for child in node.named_children() {
        if INTERPOLATION_KINDS.contains(&child.kind()) {
            out.push(SLOT);
        } else if child.kind() == "escape_sequence" {
            out.extend(child.text().chars().nth(1));
        } else if CONTENT_KINDS.contains(&child.kind()) {
            push_text(child.text(), format, out);
        }
    }
}

/// Copies `text`, replacing each format placeholder with a [`SLOT`].
fn push_text(text: &str, format: Format, out: &mut String) {
    let (open, escaped) = match format {
        Format::Plain => return out.push_str(text),
        Format::Percent => ('%', "%%"),
        Format::Brace => ('{', "{{"),
    };
    let mut rest = text;
    while let Some(at) = rest.find(open) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with(escaped) {
            out.push(open);
            rest = &rest[2..];
            continue;
        }
        let length = placeholder_length(rest, format);
        out.push(if length > 0 { SLOT } else { open });
        rest = &rest[length.max(1)..];
    }
    out.push_str(rest);
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

/// A format call's template and its placeholder style: Java
/// `"..".formatted(..)` and `String.format("..", ..)`, Python and Kotlin
/// `"..".format(..)`, C# `string.Format("..", ..)`, Go `fmt.Sprintf("..", ..)`.
fn format_template(node: AstNode<'_>) -> Option<(AstNode<'_>, Format)> {
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
    match (path.rsplit('.').next().unwrap_or_default(), template) {
        ("formatted", Some(template)) => Some((template, Format::Percent)),
        ("format", Some(template)) if brace => Some((template, Format::Brace)),
        ("format", Some(template)) => Some((template, Format::Percent)),
        _ if matches!(
            path.as_str(),
            "String.format" | "java.lang.String.format" | "fmt.Sprintf"
        ) =>
        {
            Some((first_argument(node)?, Format::Percent))
        }
        _ if matches!(path.as_str(), "string.Format" | "String.Format") => {
            Some((first_argument(node)?, Format::Brace))
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

/// Python's `template % values`.
fn percent_template(node: AstNode<'_>) -> Option<AstNode<'_>> {
    (node.kind() == "binary_operator" && operator(node) == Some("%"))
        .then(|| node.named_child(0))?
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

/// An identifier whose every binding in its function is a string literal
/// without interpolation.
fn is_literal_local(node: AstNode<'_>) -> bool {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    let Some(scope) = std::iter::successors(node.parent(), AstNode::parent)
        .find(|ancestor| FUNCTION_KINDS.contains(&ancestor.kind()))
    else {
        return false;
    };
    let bindings = ast_scan::nodes_of_kind(
        scope,
        &[
            "variable_declarator",
            "assignment",
            "assignment_expression",
            "augmented_assignment",
        ],
    )
    .into_iter()
    .filter(|binding| binding_name(*binding).is_some_and(|name| name.text() == node.text()))
    .collect::<Vec<_>>();
    !bindings.is_empty()
        && bindings.iter().all(|binding| {
            binding.kind() != "augmented_assignment"
                && binding_value(*binding).is_some_and(is_literal_text)
        })
}

/// A string literal without interpolation, or a choice between such
/// literals (`escape ? " ESCAPE '\\'" : ""`).
fn is_literal_text(value: AstNode<'_>) -> bool {
    if matches!(
        value.kind(),
        "ternary_expression" | "conditional_expression"
    ) {
        let branches = ["consequence", "alternative"]
            .iter()
            .filter_map(|field| value.child_by_field_name(field))
            .collect::<Vec<_>>();
        return branches.len() == 2 && branches.into_iter().all(is_literal_text);
    }
    STRING_KINDS.contains(&value.kind())
        && !value
            .named_children()
            .any(|part| INTERPOLATION_KINDS.contains(&part.kind()))
}

fn binding_name(binding: AstNode<'_>) -> Option<AstNode<'_>> {
    binding
        .child_by_field_name("name")
        .or_else(|| binding.child_by_field_name("left"))
        .or_else(|| binding.named_child(0))
}

fn binding_value(binding: AstNode<'_>) -> Option<AstNode<'_>> {
    binding
        .child_by_field_name("value")
        .or_else(|| binding.child_by_field_name("right"))
        .or_else(|| binding.named_children().last())
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

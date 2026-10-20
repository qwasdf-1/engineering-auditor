//! Format templates and concatenations a SQL query is built with: where a
//! template's text is and which values its placeholders take. Findings-free.

use auditor_ast::{call_arguments, AstNode};

use crate::ast_scan;

/// Literal string kinds, which may hold interpolations.
pub(crate) const STRING_KINDS: &[&str] = &[
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
pub(crate) enum Format {
    Plain,
    /// `%s`, `%1$s`, `%(name)s`.
    Percent,
    /// `{}`, `{0}`, `{name}`.
    Brace,
}

/// The byte length of the placeholder opening `text`, or 0 for none.
pub(crate) fn placeholder_length(text: &str, format: Format) -> usize {
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
/// `fmt.Sprintf("..", ..)`, and Rust `format!("..", ..)`.
pub(crate) fn format_template(
    node: AstNode<'_>,
) -> Option<(AstNode<'_>, Format, Vec<AstNode<'_>>)> {
    if node.kind() == "macro_invocation" {
        return rust_format(node);
    }
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

/// Rust's `format!("..", ..)`: its template and the tokens after it.
fn rust_format(node: AstNode<'_>) -> Option<(AstNode<'_>, Format, Vec<AstNode<'_>>)> {
    if node.child_by_field_name("macro")?.text() != "format" {
        return None;
    }
    let tokens = node
        .named_children()
        .find(|child| child.kind() == "token_tree")?;
    let mut values = tokens.named_children();
    let template = values
        .next()
        .filter(|first| first.kind() == "string_literal")?;
    Some((template, Format::Brace, values.collect()))
}

/// The operands of a `+` concatenation.
pub(crate) fn concatenation(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
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
pub(crate) fn percent_template(node: AstNode<'_>) -> Option<(AstNode<'_>, Vec<AstNode<'_>>)> {
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

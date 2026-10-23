//! Values spliced into a built SQL query that cannot carry SQL text:
//! literals and constants, numbers (`int.Parse(s)`, a `long` parameter),
//! identifiers quoted for SQL, and generated bind-placeholder lists; and the
//! template a joined list's callback returns, read as its elements' text.

use auditor_ast::{AstNode, FunctionFlow, Origin};

use crate::ast_scan;
use crate::dataflow_sources::callee_name;
use crate::security_sql_templates::{CONTENT_KINDS, INTERPOLATION_KINDS, STRING_KINDS};

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
    // A catalog object's own name, quoted for SQL (DBeaver, Liquibase).
    "getFullyQualifiedName",
    "getQuotedIdentifier",
    "escapeObjectName",
    "correctObjectName",
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

/// A value that cannot carry SQL text: a literal, a constant, a number, or
/// a generated placeholder list.
pub(super) fn is_fixed(node: AstNode<'_>) -> bool {
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
            text.chars()
                .all(|character| character.is_whitespace() || "?@$:,0123456789".contains(character))
                || text.starts_with('@') && text[1..].chars().all(char::is_alphanumeric)
        })
}

/// The string a joined list's callback returns, read as the text of every
/// element: `rows.map(([id]) => `WHEN '${id}' ..`).join(' ')` or
/// `string.Join(" ", rows.Select(r => $"('{r.Name}')"))`.
pub(super) fn mapped_template(node: AstNode<'_>) -> Option<AstNode<'_>> {
    if !is_call(node) || !JOIN_CALLS.contains(&callee_name(node)) {
        return None;
    }
    let callback = ast_scan::nodes_of_kind(
        node,
        &[
            "arrow_function",
            "lambda_expression",
            "lambda",
            "func_literal",
        ],
    )
    .into_iter()
    .next()?;
    let body = callback.child_by_field_name("body")?;
    if STRING_KINDS.contains(&body.kind()) {
        return Some(body);
    }
    let returns = ast_scan::nodes_of_kind(body, &["return_statement"]);
    match returns.as_slice() {
        [only] => only
            .named_children()
            .find(|value| STRING_KINDS.contains(&value.kind())),
        _ => None,
    }
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

/// A name whose every reaching value is a literal (`final String filters =
/// """..."""`, `clause = escape ? " ESCAPE '\\'" : ""`).
fn is_constant(node: AstNode<'_>) -> bool {
    if !matches!(node.kind(), "identifier" | "simple_identifier") {
        return false;
    }
    FunctionFlow::enclosing(node).only_from(node, |origin| matches!(origin, Origin::Literal(_)))
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

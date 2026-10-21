//! The C# and C++ regex pattern sites `catastrophic_backtracking_regex`
//! reads: .NET `Regex` constructions, static methods, and
//! `[GeneratedRegex]`, and C++ `std::regex` constructions and declarations.

use auditor_ast::AstNode;

use super::unescape_string;
use crate::ast_scan;
use crate::csharp_api_identity::{csharp_arguments, verbatim_content, CSharpScope};

const CSHARP_REGEX: &str = "System.Text.RegularExpressions.Regex";
const CSHARP_STATIC_REGEX_METHODS: &[&str] = &[
    "IsMatch",
    "Match",
    "Matches",
    "Replace",
    "Split",
    "Count",
    "EnumerateMatches",
];

/// `new Regex(pattern, ..)`, the static `Regex.IsMatch(input, pattern, ..)`
/// family, and `[GeneratedRegex(pattern)]`. An instance call
/// (`regex.Replace(input, replacement)`) takes no pattern.
pub(super) fn csharp_regexes(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    let mut regexes = CSharpScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            let position = if api == format!("{CSHARP_REGEX}..ctor") {
                0
            } else {
                let method = api.strip_prefix(CSHARP_REGEX)?.strip_prefix('.')?;
                let receiver = call
                    .child_by_field_name("function")?
                    .child_by_field_name("expression")?;
                let static_call = receiver.text() == "Regex" || receiver.text() == CSHARP_REGEX;
                (static_call && CSHARP_STATIC_REGEX_METHODS.contains(&method)).then_some(1)?
            };
            csharp_regex_site(call, position)
        })
        .collect::<Vec<_>>();
    for attribute in ast_scan::nodes_of_kind(root, &["attribute"]) {
        let generated = attribute.child_by_field_name("name").is_some_and(|name| {
            matches!(name.text(), "GeneratedRegex" | "GeneratedRegexAttribute")
        });
        let pattern = attribute
            .named_children()
            .find(|child| child.kind() == "attribute_argument_list")
            .and_then(|list| list.named_child(0))
            .and_then(|argument| argument.named_child(0));
        let non_backtracking = attribute.text().contains("NonBacktracking");
        if let (true, Some(pattern), false) = (generated, pattern, non_backtracking) {
            if let Some(pattern) = csharp_pattern(pattern) {
                regexes.push((attribute, pattern));
            }
        }
    }
    regexes
}

fn csharp_regex_site(call: AstNode<'_>, position: usize) -> Option<(AstNode<'_>, String)> {
    let arguments = csharp_arguments(call);
    if arguments
        .iter()
        .any(|(_, value)| value.text().contains("NonBacktracking"))
    {
        return None;
    }
    let (_, literal) = arguments.get(position)?;
    Some((call, csharp_pattern(*literal)?))
}

/// A C# string literal's pattern: a regular string unescaped, a verbatim
/// string with `""` for a quote, a raw string as written.
fn csharp_pattern(literal: AstNode<'_>) -> Option<String> {
    let text = literal.text();
    match literal.kind() {
        "string_literal" => Some(unescape_string(text.strip_prefix('"')?.strip_suffix('"')?)),
        "verbatim_string_literal" => Some(verbatim_content(text)?.replace("\"\"", "\"")),
        "raw_string_literal" => literal
            .named_children()
            .find(|child| child.kind() == "raw_string_content")
            .map(|content| content.text().to_string()),
        _ => None,
    }
}

const CPP_REGEX_TYPES: &[&str] = &["std::regex", "std::wregex", "std::basic_regex"];

/// `std::regex(pattern)` and the declaration `std::regex re(pattern)` (or
/// `{pattern}`): every `std::regex` grammar backtracks.
pub(super) fn cpp_regexes(root: AstNode<'_>) -> Vec<(AstNode<'_>, String)> {
    let is_regex_type = |node: AstNode<'_>| {
        let name = node
            .text()
            .split('<')
            .next()
            .unwrap_or_default()
            .replace(' ', "");
        CPP_REGEX_TYPES.contains(&name.as_str())
    };
    let mut regexes = Vec::new();
    for call in ast_scan::nodes_of_kind(root, &["call_expression"]) {
        if call
            .child_by_field_name("function")
            .is_some_and(is_regex_type)
        {
            regexes.extend(cpp_first_pattern(
                call,
                call.child_by_field_name("arguments"),
            ));
        }
    }
    for declaration in ast_scan::nodes_of_kind(root, &["declaration"]) {
        if !declaration
            .child_by_field_name("type")
            .is_some_and(is_regex_type)
        {
            continue;
        }
        for declarator in declaration
            .named_children()
            .filter(|child| child.kind() == "init_declarator")
        {
            regexes.extend(cpp_first_pattern(
                declarator,
                declarator.child_by_field_name("value"),
            ));
        }
    }
    regexes
}

/// The pattern in the first argument of a construction's argument or
/// initializer list.
fn cpp_first_pattern<'a>(
    site: AstNode<'a>,
    arguments: Option<AstNode<'a>>,
) -> Option<(AstNode<'a>, String)> {
    let literal = arguments?.named_child(0)?;
    let text = literal.text();
    let pattern = match literal.kind() {
        "string_literal" => unescape_string(text.strip_prefix('"')?.strip_suffix('"')?),
        // `R"(..)"` or `R"delim(..)delim"`.
        "raw_string_literal" => {
            let body = text.strip_prefix("R\"")?.strip_suffix('"')?;
            let open = body.find('(')?;
            let close = body.rfind(')')?;
            body.get(open + 1..close)?.to_string()
        }
        _ => return None,
    };
    Some((site, pattern))
}

//! The names a C or C++ file uses as values. A function so used (a callback
//! argument, a dispatch-table entry, `return name;`, `&name`) has the
//! signature of the function-pointer type it is used as.

use std::collections::BTreeSet;

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::Language;

/// Every identifier that is neither the function position of a call nor a
/// name being declared; empty for other languages.
pub(super) fn c_value_references(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> BTreeSet<String> {
    if !matches!(file.language, Language::C | Language::Cpp) {
        return BTreeSet::new();
    }
    let mut names = ast_scan::nodes_of_kind(tree.root_node(), &["identifier"])
        .into_iter()
        .filter(|identifier| is_value_use(*identifier))
        .map(|identifier| identifier.text().to_string())
        .collect::<BTreeSet<_>>();
    // A declaration quarantined for a syntax error is not in the tree, so
    // its uses are read from its code: an identifier not followed by `(`.
    for quarantined in tree.quarantined() {
        names.extend(uncalled_identifiers(&c_code(&quarantined.text)));
    }
    names
}

/// `text` with its comments, string and character literals, and
/// preprocessor lines blanked, so a name in `/* see f */` is not a use.
fn c_code(text: &str) -> String {
    let mut code = String::with_capacity(text.len());
    let mut index = 0;
    let mut line_start = true;
    while let Some(ch) = text[index..].chars().next() {
        if let Some(length) = blanked_length(&text[index..], line_start) {
            code.push(' ');
            index += length;
            line_start = false;
        } else {
            code.push(ch);
            index += ch.len_utf8();
            line_start = ch == '\n' || (line_start && ch.is_whitespace());
        }
    }
    code
}

/// The length of the comment, literal, or preprocessor line `rest` opens
/// with; a line comment or directive leaves its newline.
fn blanked_length(rest: &str, line_start: bool) -> Option<usize> {
    if rest.starts_with("//") || (line_start && rest.starts_with('#')) {
        Some(rest.find('\n').unwrap_or(rest.len()))
    } else if let Some(body) = rest.strip_prefix("/*") {
        Some(body.find("*/").map_or(rest.len(), |end| end + 4))
    } else if rest.starts_with(['"', '\'']) {
        Some(literal_length(rest.as_bytes()))
    } else {
        None
    }
}

/// The length of the quoted literal `bytes` opens with, escapes included.
fn literal_length(bytes: &[u8]) -> usize {
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == bytes[0] {
            return index + 1;
        }
    }
    bytes.len()
}

fn uncalled_identifiers(text: &str) -> BTreeSet<String> {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = text.as_bytes();
    let mut names = BTreeSet::new();
    let mut start = 0;
    while start < bytes.len() {
        if !is_word(bytes[start]) {
            start += 1;
            continue;
        }
        let end = bytes[start..]
            .iter()
            .position(|byte| !is_word(*byte))
            .map_or(bytes.len(), |length| start + length);
        if !bytes[start].is_ascii_digit() && !text[end..].trim_start().starts_with('(') {
            names.insert(text[start..end].to_string());
        }
        start = end;
    }
    names
}

fn is_value_use(identifier: AstNode<'_>) -> bool {
    // `Type::name` is one expression: judge its outermost qualifier.
    let mut node = identifier;
    while let Some(parent) = node.parent() {
        if parent.kind() != "qualified_identifier" {
            break;
        }
        node = parent;
    }
    let Some(parent) = node.parent() else {
        return true;
    };
    let is_field = |field: &str| {
        parent
            .child_by_field_name(field)
            .is_some_and(|child| child == node)
    };
    let declared = is_field("declarator")
        || parent.kind() == "parenthesized_declarator"
        // A macro's or conditional's name (`#define name`, `#ifdef name`).
        || parent.kind().starts_with("preproc_")
        || (parent.kind() == "enumerator" && is_field("name"));
    !(declared || parent.kind() == "call_expression" && is_field("function"))
}

//! Queries over syntax nodes shared by the AST detectors: descendants by
//! kind, TODO markers, literal content, declared names, and Rust test
//! ancestry.

use auditor_ast::{AstNode, AstNodeKind};

/// `node` and its named descendants, depth first.
pub(crate) fn descendants(node: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    let mut stack = vec![node];
    std::iter::from_fn(move || {
        let next = stack.pop()?;
        stack.extend(next.named_children());
        Some(next)
    })
}

/// A node's source on one line, cut at 80 characters, for a finding
/// identity.
pub(crate) fn source_summary(node: AstNode<'_>) -> String {
    let text = node.text().split_whitespace().collect::<Vec<_>>().join(" ");
    text.chars().take(80).collect()
}

pub(crate) fn nodes_of_kind<'a>(root: AstNode<'a>, kinds: &[&str]) -> Vec<AstNode<'a>> {
    kinds
        .iter()
        .flat_map(|kind| root.descendants_of_kind(&AstNodeKind::from(*kind)))
        .collect()
}

/// Comments of `kinds` that open with a TODO marker.
pub(crate) fn todo_comments<'a>(root: AstNode<'a>, kinds: &[&str]) -> Vec<AstNode<'a>> {
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter(|comment| {
            comment
                .text()
                .trim_start_matches(['/', '*', '#', ' '])
                .to_ascii_lowercase()
                .starts_with("todo")
        })
        .collect()
}

/// A stub's message saying the code is unfinished.
pub(crate) fn is_unimplemented_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "todo",
        "not implemented",
        "not yet implemented",
        "unimplemented",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

/// TODO comments of `kinds` and the unfinished-code `stubs`, in source order.
pub(crate) fn todo_nodes<'a>(
    root: AstNode<'a>,
    kinds: &[&str],
    stubs: impl IntoIterator<Item = AstNode<'a>>,
) -> Vec<AstNode<'a>> {
    let mut nodes = todo_comments(root, kinds);
    nodes.extend(stubs);
    nodes.sort_by_key(|node| node.start_byte());
    nodes
}

/// The content of a `string_literal` without interpolation or escapes (C,
/// C++, Kotlin); `None` for any other node.
pub(crate) fn string_literal_content(node: AstNode<'_>) -> Option<String> {
    if node.kind() != "string_literal" {
        return None;
    }
    let mut parts = node.named_children();
    match (parts.next(), parts.next()) {
        (None, _) => Some(String::new()),
        (Some(content), None) if content.kind() == "string_content" => {
            Some(content.text().to_string())
        }
        _ => None,
    }
}

/// The only named child of `kind`, if there is exactly one.
pub(crate) fn sole_child_of_kind<'a>(node: AstNode<'a>, kind: &str) -> Option<AstNode<'a>> {
    let mut matches = node.named_children().filter(|child| child.kind() == kind);
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// The `name` field of every node of `kinds` in `root`.
pub(crate) fn declared_names<'a>(
    root: AstNode<'a>,
    kinds: &'a [&'a str],
) -> impl Iterator<Item = String> + 'a {
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter_map(|declaration| declaration.child_by_field_name("name"))
        .map(|name| name.text().to_string())
}

/// Whether `node` is, or contains, an identifier of `kind` named in `names`.
pub(crate) fn mentions_any(node: AstNode<'_>, kind: &str, names: &[&str]) -> bool {
    std::iter::once(node)
        .chain(nodes_of_kind(node, &[kind]))
        .any(|identifier| names.contains(&identifier.text()))
}

/// A block whose only statement is `return true;`.
pub(crate) fn returns_only_true(body: AstNode<'_>) -> bool {
    let mut statements = body
        .named_children()
        .filter(|child| !child.kind().contains("comment"));
    match (statements.next(), statements.next()) {
        (Some(statement), None) => {
            statement.kind() == "return_statement"
                && statement
                    .named_child(0)
                    .is_some_and(|value| value.text() == "true")
        }
        _ => false,
    }
}

pub(crate) fn callee(node: AstNode<'_>) -> &str {
    node.child_by_field_name("function")
        .or_else(|| node.child_by_field_name("callee"))
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0))
        .map(|child| child.text())
        .unwrap_or_default()
}

pub(crate) fn is_inside_rust_test(mut node: AstNode<'_>) -> bool {
    loop {
        if matches!(node.kind(), "function_item" | "mod_item") {
            let header = node
                .text()
                .split_once('{')
                .map_or(node.text(), |part| part.0);
            if header.contains("#[test]")
                || header.contains("cfg(test)")
                || preceding_test_attribute(node)
            {
                return true;
            }
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn preceding_test_attribute(node: AstNode<'_>) -> bool {
    preceding_attributes(node).any(|attribute| {
        attribute.text().contains("#[test]") || attribute.text().contains("cfg(test)")
    })
}

/// The run of Rust `attribute_item` siblings directly before `node`.
pub(crate) fn preceding_attributes(node: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    let mut run = Vec::new();
    if let Some(parent) = node.parent() {
        for sibling in parent.named_children() {
            if sibling.start_byte() == node.start_byte() && sibling.end_byte() == node.end_byte() {
                break;
            }
            if sibling.kind() == "attribute_item" {
                run.push(sibling);
            } else {
                run.clear();
            }
        }
    }
    run.into_iter()
}

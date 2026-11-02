//! Per-language source spans and names the structure index attaches to
//! symbols and imports: Rust test, impl, and attribute-bound items, Python
//! `@overload` stubs and guarded imports, and the module a file's items are
//! qualified with; and the C/C++ names a file uses as values.

use std::collections::BTreeSet;

use auditor_ast::{AstNode, SourceSpan};

use crate::ast_scan;
use crate::Language;

/// The names a C or C++ file uses as values: an identifier that is neither
/// the function position of a call nor a name being declared. A function so
/// used (a callback argument, a dispatch-table entry, `return name;`,
/// `&name`) has the signature of the function-pointer type it is used as.
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
    let mut chars = text.chars().peekable();
    let mut line_start = true;
    while let Some(ch) = chars.next() {
        let skip_to = match (ch, chars.peek()) {
            ('#', _) if line_start => Some("\n"),
            ('/', Some('/')) => Some("\n"),
            ('/', Some('*')) => Some("*/"),
            ('"', _) => Some("\""),
            ('\'', _) => Some("'"),
            _ => None,
        };
        if !ch.is_whitespace() {
            line_start = false;
        }
        let Some(end) = skip_to else {
            line_start |= ch == '\n';
            code.push(ch);
            continue;
        };
        if ch == '/' {
            chars.next();
        }
        let mut previous = ' ';
        while let Some(next) = chars.next() {
            let escaped = previous == '\\' && matches!(ch, '"' | '\'');
            if !escaped && (end.ends_with(next) && (end.len() == 1 || previous == '*')) {
                break;
            }
            previous = if escaped { ' ' } else { next };
        }
        code.push(if end == "\n" { '\n' } else { ' ' });
        line_start = end == "\n";
    }
    code
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

pub(super) fn rust_implicitly_used_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    const BINDING_ATTRIBUTES: &[&str] = &["global_allocator", "no_mangle", "used", "export_name"];
    if file.language != Language::Rust {
        return Vec::new();
    }
    ast_scan::nodes_of_kind(
        tree.root_node(),
        &["type_item", "const_item", "static_item"],
    )
    .into_iter()
    .filter(|node| {
        let in_impl = node.parent().is_some_and(|list| {
            list.kind() == "declaration_list"
                && list
                    .parent()
                    .is_some_and(|owner| matches!(owner.kind(), "impl_item" | "trait_item"))
        });
        in_impl
            || ast_scan::preceding_attributes(*node).any(|attribute| {
                BINDING_ATTRIBUTES
                    .iter()
                    .any(|name| attribute.text().contains(name))
            })
    })
    .map(SourceSpan::from_node)
    .collect()
}

pub(super) fn python_overload_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    python_decorated_spans(
        file,
        tree,
        &[
            "overload",
            "typing.overload",
            "t.overload",
            "typing_extensions.overload",
        ],
    )
}

/// Python definitions whose results are cached per argument, so a call
/// repeats no work after the first: `@functools.cache`, `@lru_cache(..)`.
pub(super) fn python_memoized_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    python_decorated_spans(
        file,
        tree,
        &[
            "cache",
            "functools.cache",
            "lru_cache",
            "functools.lru_cache",
            "cached_property",
            "functools.cached_property",
        ],
    )
}

/// Python definitions with a decorator named in `names`, called or not.
fn python_decorated_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    names: &[&str],
) -> Vec<SourceSpan> {
    if file.language != Language::Python {
        return Vec::new();
    }
    ast_scan::nodes_of_kind(tree.root_node(), &["decorated_definition"])
        .into_iter()
        .filter(|node| {
            node.named_children().any(|decorator| {
                let text = decorator.text().trim_start_matches('@');
                decorator.kind() == "decorator"
                    && names.contains(&text.split('(').next().unwrap_or(text).trim())
            })
        })
        .filter_map(|node| node.child_by_field_name("definition"))
        .map(SourceSpan::from_node)
        .collect()
}

pub(super) fn rust_associated_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    if file.language != Language::Rust {
        return Vec::new();
    }
    ast_scan::nodes_of_kind(tree.root_node(), &["function_item"])
        .into_iter()
        .filter(|node| {
            node.parent().is_some_and(|list| {
                list.kind() == "declaration_list"
                    && list
                        .parent()
                        .is_some_and(|owner| owner.kind() == "impl_item")
            })
        })
        .map(SourceSpan::from_node)
        .collect()
}

pub(super) fn module_name(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Option<String> {
    let stem = file.relative.file_stem()?.to_str()?;
    let directory = || {
        file.relative
            .parent()
            .and_then(std::path::Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_string)
    };
    match file.language {
        Language::Go => ast_scan::nodes_of_kind(tree.root_node(), &["package_identifier"])
            .into_iter()
            .next()
            .map(|name| name.text().to_string())
            .filter(|name| name != "main"),
        Language::Rust => match stem {
            "lib" | "main" | "build" => None,
            "mod" => directory(),
            _ => Some(stem.to_string()),
        },
        Language::Python => match stem {
            "__main__" | "setup" | "conftest" => None,
            "__init__" => directory(),
            _ => Some(stem.to_string()),
        },
        _ => None,
    }
}

pub(super) fn rust_test_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    if file.language != Language::Rust {
        return Vec::new();
    }
    ast_scan::nodes_of_kind(tree.root_node(), &["function_item", "mod_item"])
        .into_iter()
        .filter(|node| ast_scan::is_inside_rust_test(*node))
        .map(SourceSpan::from_node)
        .collect()
}

/// Blocks where a deferred Python import is conventional: the body of a `try`
/// whose handler catches `ImportError` or `ModuleNotFoundError` (an optional
/// dependency) and the body of `if TYPE_CHECKING:`.
pub(super) fn python_guarded_import_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    if file.language != Language::Python {
        return Vec::new();
    }
    let root = tree.root_node();
    let optional = ast_scan::nodes_of_kind(root, &["try_statement"])
        .into_iter()
        .filter(|node| {
            node.named_children().any(|handler| {
                let header = handler.text().split(':').next().unwrap_or_default();
                handler.kind() == "except_clause"
                    && (header.contains("ImportError") || header.contains("ModuleNotFoundError"))
            })
        })
        .filter_map(|node| node.child_by_field_name("body"));
    optional
        .map(SourceSpan::from_node)
        .chain(python_type_checking_spans(file, tree))
        .collect()
}

/// The bodies of Python `if TYPE_CHECKING:` blocks, whose imports exist only
/// for the type checker and bind nothing at runtime.
pub(super) fn python_type_checking_spans(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> Vec<SourceSpan> {
    if file.language != Language::Python {
        return Vec::new();
    }
    ast_scan::nodes_of_kind(tree.root_node(), &["if_statement"])
        .into_iter()
        .filter(|node| {
            node.child_by_field_name("condition")
                .is_some_and(|condition| condition.text().trim().ends_with("TYPE_CHECKING"))
        })
        .filter_map(|node| node.child_by_field_name("consequence"))
        .map(SourceSpan::from_node)
        .collect()
}

/// A comment on the import's line or the three lines above it that states
/// why the import is deferred: `# Lazily import 'netrc'`, `# prevent
/// circular import`.
pub(super) fn import_has_documented_reason(tree: &auditor_ast::AstTree, line: usize) -> bool {
    const REASONS: &[&str] = &[
        "deferred",
        "lazy",
        "lazily",
        "optional dependency",
        "circular",
        "cycle",
    ];
    tree.source()
        .lines()
        .enumerate()
        .skip(line.saturating_sub(4))
        .take_while(|(index, _)| *index < line)
        .any(|(_, text)| crate::util::comment_gives_reason(text, REASONS))
}

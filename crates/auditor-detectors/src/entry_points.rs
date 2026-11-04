//! A repository's entry points: where control enters it from outside.
//! Findings-free.
//!
//! - `main`: a program's `main` (Go `package main`, a Rust binary, Java,
//!   Kotlin, C#, C, C++), C# top-level statements, a Python `__main__` guard
//!   and the functions it calls, a JavaScript script with a shebang;
//! - `cli_command`: click/typer commands, Django management commands,
//!   urfave/cli actions;
//! - `http_handler`: route handlers ([`crate::entry_points_http`]);
//! - `process_run`: `run` of a `multiprocessing.Process` subclass, through
//!   its bases in any file ([`crate::structure_index::hierarchy`]);
//! - `signal_handler`: functions registered for a process signal.
//!
//! Each file records what its syntax marks while the structure index is
//! built ([`local_entries`]); [`entry_points`] resolves those marks to
//! repository symbols, and a handler named in a registration
//! (`app.get('/users', users.list)`) through the call graph's lookup. A mark
//! inside test code (a Rust `#[cfg(test)]` module) is left out.

use std::collections::HashMap;
use std::ops::Range;

use auditor_ast::{AstNode, SymbolKind};

use crate::ast_scan::nodes_of_kind;
use crate::call_graph::{Lookup, NameRef, SymbolRef};
use crate::structure_index::hierarchy::is_type;
use crate::structure_index::{StructureFile, StructureIndex};
use crate::{AuditedFile, Language};

/// The external bases whose subclasses' `run` is a child process's main.
const PROCESS_BASES: [&str; 2] = ["multiprocessing.Process", "multiprocessing.context.Process"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntryKind {
    Main,
    CliCommand,
    HttpHandler,
    ProcessRun,
    SignalHandler,
}

impl EntryKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::CliCommand => "cli_command",
            Self::HttpHandler => "http_handler",
            Self::ProcessRun => "process_run",
            Self::SignalHandler => "signal_handler",
        }
    }
}

/// What a file's syntax marks as entered from outside.
#[derive(Clone, Debug)]
pub(crate) struct LocalEntry {
    pub(crate) kind: EntryKind,
    pub(crate) target: EntryTarget,
    pub(crate) line: usize,
    pub(crate) detail: String,
}

#[derive(Clone, Debug)]
pub(crate) enum EntryTarget {
    /// A function, class, lambda, or block, by its byte range.
    Span(Range<usize>),
    /// A function a registration names (`handlers.list`) at byte `site`.
    /// When `required`, a name resolving to no repository function drops
    /// the entry: the registration's shape alone does not prove a handler.
    Name {
        text: String,
        site: usize,
        required: bool,
    },
}

impl LocalEntry {
    pub(crate) fn spanning(node: AstNode<'_>, kind: EntryKind, detail: String) -> Self {
        Self {
            kind,
            target: EntryTarget::Span(node.start_byte()..node.end_byte()),
            line: node.start_position().0 + 1,
            detail,
        }
    }

    pub(crate) fn naming(
        site: AstNode<'_>,
        handler: AstNode<'_>,
        required: bool,
        kind: EntryKind,
        detail: String,
    ) -> Self {
        Self {
            kind,
            target: EntryTarget::Name {
                text: handler.text().split_whitespace().collect(),
                site: site.start_byte(),
                required,
            },
            line: site.start_position().0 + 1,
            detail,
        }
    }
}

/// An entry point resolved in the repository.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EntryPoint {
    pub(crate) file: usize,
    pub(crate) line: usize,
    pub(crate) kind: EntryKind,
    /// The function or class entered; `None` for a block (a `__main__`
    /// guard, top-level statements), a lambda, or an unresolved handler.
    pub(crate) symbol: Option<SymbolRef>,
    pub(crate) detail: String,
}

/// The entries a file's syntax marks, recorded while the structure index is
/// built.
pub(crate) fn local_entries(file: &AuditedFile, root: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = main_entries(file, root);
    entries.extend(
        crate::errorprone::exit_entry_points::process_entries(file, root)
            .into_iter()
            .map(|(span, kind, detail)| LocalEntry {
                kind,
                line: root.text()[..span.start].matches('\n').count() + 1,
                target: EntryTarget::Span(span),
                detail: detail.to_string(),
            }),
    );
    entries.extend(crate::entry_points_http::http_entries(file, root));
    entries
}

/// Every entry point of the repository, ordered by file and line.
pub(crate) fn entry_points(structure: &StructureIndex) -> Vec<EntryPoint> {
    let lookup = Lookup::new(structure);
    let mut entries = Vec::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        entries.extend(
            file.entries
                .iter()
                .filter_map(|entry| resolve_entry(&lookup, (file_index, file), entry)),
        );
    }
    entries.extend(process_runs(structure));
    entries.sort();
    entries.dedup_by(|left, right| {
        left.file == right.file
            && left.kind == right.kind
            && left.symbol.is_some()
            && left.symbol == right.symbol
    });
    entries
}

/// A file's local entry as a repository entry point, or `None` for one in
/// test code or naming a required handler that does not resolve.
fn resolve_entry(
    lookup: &Lookup,
    (file_index, file): (usize, &StructureFile),
    entry: &LocalEntry,
) -> Option<EntryPoint> {
    let site = match &entry.target {
        EntryTarget::Span(span) => span.start,
        EntryTarget::Name { site, .. } => *site,
    };
    // A route or `main` inside test code (a Rust `#[cfg(test)]` module of a
    // production file) is the test's, not the repository's.
    if enclosing_function(file, site).is_some_and(|owner| file.symbols[owner].in_test) {
        return None;
    }
    let symbol = match &entry.target {
        EntryTarget::Span(span) => spanned_symbol(file, span).map(|index| (file_index, index)),
        EntryTarget::Name { text, required, .. } => {
            let owner = enclosing_function(file, site);
            let resolved = NameRef::parse(text)
                .and_then(|reference| lookup.resolve(file_index, owner, reference));
            if resolved.is_none() && *required {
                return None;
            }
            resolved
        }
    };
    Some(EntryPoint {
        file: file_index,
        line: entry.line,
        kind: entry.kind,
        symbol,
        detail: entry.detail.clone(),
    })
}

/// The byte spans of process subclasses' `run` methods, by file path.
pub(crate) fn process_run_spans(structure: &StructureIndex) -> HashMap<&str, Vec<Range<usize>>> {
    let mut spans: HashMap<&str, Vec<Range<usize>>> = HashMap::new();
    for entry in process_runs(structure) {
        let file = &structure.files()[entry.file];
        if let Some((_, symbol)) = entry.symbol {
            let span = file.symbols[symbol].span;
            spans
                .entry(file.path.as_str())
                .or_default()
                .push(span.start_byte..span.end_byte);
        }
    }
    spans
}

/// `run` of each type whose bases lead to `multiprocessing.Process`.
fn process_runs(structure: &StructureIndex) -> Vec<EntryPoint> {
    let hierarchy = structure.hierarchy();
    let mut runs = Vec::new();
    for (file_index, file) in structure.files().iter().enumerate() {
        if file.language != Language::Python {
            continue;
        }
        for (class_index, class) in file.symbols.iter().enumerate() {
            if !is_type(class.kind) {
                continue;
            }
            let Some(root) = hierarchy
                .external_root((file_index, class_index))
                .filter(|root| PROCESS_BASES.contains(root))
            else {
                continue;
            };
            runs.extend(
                file.symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, method)| {
                        method.parent == Some(class_index as u32)
                            && method.name == "run"
                            && is_callable(method.kind)
                    })
                    .map(|(method_index, method)| EntryPoint {
                        file: file_index,
                        line: method.line,
                        kind: EntryKind::ProcessRun,
                        symbol: Some((file_index, method_index)),
                        detail: format!("run of {} subclass {}", root, class.name),
                    }),
            );
        }
    }
    runs
}

/// The innermost function or type whose span ends where `span` does and
/// contains it: the declaration a recognizer matched, decorators included.
fn spanned_symbol(file: &StructureFile, span: &Range<usize>) -> Option<usize> {
    file.symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            (is_callable(symbol.kind) || is_type(symbol.kind))
                && symbol.span.end_byte == span.end
                && symbol.span.start_byte <= span.start
        })
        .max_by_key(|(_, symbol)| symbol.span.start_byte)
        .map(|(index, _)| index)
}

fn enclosing_function(file: &StructureFile, at: usize) -> Option<usize> {
    file.symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            is_callable(symbol.kind) && symbol.span.start_byte <= at && at < symbol.span.end_byte
        })
        .max_by_key(|(_, symbol)| symbol.span.start_byte)
        .map(|(index, _)| index)
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

// ------------------------------------------------------------------ mains

fn main_entries(file: &AuditedFile, root: AstNode<'_>) -> Vec<LocalEntry> {
    let text = root.text();
    let main = |node: AstNode<'_>, detail: &str| {
        LocalEntry::spanning(node, EntryKind::Main, detail.to_string())
    };
    match file.language {
        Language::Go
            if text.contains("func main")
                && crate::go_api_rules::is_go_main_package(file, root) =>
        {
            named_functions(root, &["function_declaration"], &["main"])
                .map(|function| main(function, "func main"))
                .collect()
        }
        Language::Rust
            if text.contains("fn main") && crate::rust_api_rules::is_rust_program(file) =>
        {
            root.named_children()
                .filter(|item| {
                    item.kind() == "function_item" && declared_name(*item) == Some("main")
                })
                .map(|function| main(function, "fn main"))
                .collect()
        }
        Language::Java if text.contains("main") => nodes_of_kind(root, &["method_declaration"])
            .into_iter()
            .filter(|method| crate::java_syntax::is_java_main_method(*method))
            .map(|method| main(method, "static main"))
            .collect(),
        Language::Kotlin if text.contains("fun main") => root
            .named_children()
            .filter(|item| {
                item.kind() == "function_declaration" && declared_name(*item) == Some("main")
            })
            .map(|function| main(function, "fun main"))
            .collect(),
        Language::CSharp => csharp_mains(root),
        Language::C | Language::Cpp if text.contains("main") => {
            nodes_of_kind(root, &["function_definition"])
                .into_iter()
                .filter(|function| {
                    crate::c_api_rules::function_name(*function)
                        .is_some_and(|name| matches!(name, "main" | "wmain" | "WinMain"))
                })
                .map(|function| main(function, "main"))
                .collect()
        }
        Language::Python if text.contains("__main__") => python_main_guards(root),
        Language::JavaScript | Language::TypeScript if text.starts_with("#!") => {
            vec![main(root, "script with a shebang")]
        }
        _ => Vec::new(),
    }
}

fn csharp_mains(root: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    if let Some(first) = root
        .named_children()
        .find(|child| child.kind() == "global_statement")
    {
        entries.push(LocalEntry::spanning(
            first,
            EntryKind::Main,
            "top-level statements".to_string(),
        ));
    }
    if root.text().contains("Main") {
        entries.extend(
            named_functions(root, &["method_declaration"], &["Main"])
                .filter(|method| {
                    method
                        .named_children()
                        .any(|child| child.kind() == "modifier" && child.text() == "static")
                })
                .map(|method| {
                    LocalEntry::spanning(method, EntryKind::Main, "static Main".to_string())
                }),
        );
    }
    entries
}

/// A module-level `if __name__ == "__main__":` block, and the functions it
/// calls by name.
fn python_main_guards(root: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    for guard in root
        .named_children()
        .filter(|statement| statement.kind() == "if_statement")
    {
        let is_guard = guard
            .child_by_field_name("condition")
            .is_some_and(|condition| {
                let compact = condition
                    .text()
                    .replace('\'', "\"")
                    .split_whitespace()
                    .collect::<String>();
                matches!(
                    compact.as_str(),
                    "__name__==\"__main__\"" | "\"__main__\"==__name__"
                )
            });
        let Some(body) = guard
            .child_by_field_name("consequence")
            .filter(|_| is_guard)
        else {
            continue;
        };
        entries.push(LocalEntry::spanning(
            guard,
            EntryKind::Main,
            "__main__ guard".to_string(),
        ));
        for call in nodes_of_kind(body, &["call"]) {
            if let Some(function) = call
                .child_by_field_name("function")
                .filter(|function| function.kind() == "identifier")
            {
                entries.push(LocalEntry::naming(
                    call,
                    function,
                    true,
                    EntryKind::Main,
                    "called from the __main__ guard".to_string(),
                ));
            }
        }
    }
    entries
}

fn named_functions<'a>(
    root: AstNode<'a>,
    kinds: &[&str],
    names: &'static [&'static str],
) -> impl Iterator<Item = AstNode<'a>> {
    nodes_of_kind(root, kinds)
        .into_iter()
        .filter(move |function| declared_name(*function).is_some_and(|name| names.contains(&name)))
}

fn declared_name(node: AstNode<'_>) -> Option<&str> {
    node.child_by_field_name("name")
        .map(|name| name.text())
        .or_else(|| {
            node.named_children()
                .find(|child| child.kind() == "simple_identifier")
                .map(|name| name.text())
        })
}

#[cfg(test)]
#[path = "entry_points_tests.rs"]
mod tests;

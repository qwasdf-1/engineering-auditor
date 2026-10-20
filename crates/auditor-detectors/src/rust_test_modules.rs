//! Rust module graph facts for source roles: out-of-line modules declared
//! under `#[cfg(test)]` are test code in their own files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::source_discovery::{AuditedFile, Language, SourceRole};

/// A Rust module declared `#[cfg(test)] mod name;` (optionally with
/// `#[path = ".."]`) is test code in its own file, whatever that file is
/// named, and so is every module it declares in turn.
pub(crate) fn mark_rust_test_modules(files: &mut [AuditedFile]) {
    let index: BTreeMap<PathBuf, usize> = files
        .iter()
        .enumerate()
        .map(|(position, file)| (file.relative.clone(), position))
        .collect();
    let mut pending: Vec<usize> = Vec::new();
    for (position, file) in files.iter().enumerate() {
        if file.language != Language::Rust || file.role != SourceRole::Production {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file.absolute) else {
            continue;
        };
        pending.extend(
            rust_module_files(&file.relative, &text, true)
                .iter()
                .filter_map(|child| index.get(child).copied())
                .filter(|child| *child != position),
        );
    }
    while let Some(position) = pending.pop() {
        let file = &mut files[position];
        if file.role == SourceRole::Test {
            continue;
        }
        file.role = SourceRole::Test;
        let Ok(text) = fs::read_to_string(&file.absolute) else {
            continue;
        };
        pending.extend(
            rust_module_files(&file.relative, &text, false)
                .iter()
                .filter_map(|child| index.get(child).copied()),
        );
    }
}

/// Files of the out-of-line modules a Rust file declares (`mod name;`),
/// only those under `#[cfg(test)]` when `test_only` is set.
/// Strips leading `#[..]` attributes from `line`, noting `#[cfg(test)]` and
/// a `#[path = ".."]` value.
fn take_attributes<'a>(
    mut line: &'a str,
    cfg_test: &mut bool,
    path_attribute: &mut Option<String>,
) -> &'a str {
    while let Some(attribute) = line.strip_prefix("#[") {
        let Some(end) = attribute.find(']') else {
            break;
        };
        let body = attribute[..end].trim();
        *cfg_test |= body == "cfg(test)";
        if let Some(value) = body.strip_prefix("path") {
            *path_attribute = value
                .trim_start_matches([' ', '='])
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_string);
        }
        line = attribute[end + 1..].trim();
    }
    line
}

/// The name in an out-of-line module declaration (`pub mod name;`).
fn out_of_line_module(declaration: &str) -> Option<&str> {
    declaration
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        .trim_start_matches("pub ")
        .strip_prefix("mod ")
        .and_then(|tail| tail.strip_suffix(';'))
}

fn rust_module_files(parent: &Path, text: &str, test_only: bool) -> Vec<PathBuf> {
    let directory = parent.parent().unwrap_or(Path::new(""));
    let stem = parent
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let module_directory = if matches!(stem, "lib" | "main" | "mod") {
        directory.to_path_buf()
    } else {
        directory.join(stem)
    };
    let mut files = Vec::new();
    let (mut cfg_test, mut path_attribute) = (false, None::<String>);
    for line in text.lines() {
        let rest = take_attributes(line.trim(), &mut cfg_test, &mut path_attribute);
        if rest.is_empty() {
            continue;
        }
        if let Some(name) = out_of_line_module(rest) {
            if cfg_test || !test_only {
                match &path_attribute {
                    Some(path) => files.push(directory.join(path)),
                    None => {
                        files.push(module_directory.join(format!("{}.rs", name.trim())));
                        files.push(module_directory.join(name.trim()).join("mod.rs"));
                    }
                }
            }
        }
        (cfg_test, path_attribute) = (false, None);
    }
    files
}

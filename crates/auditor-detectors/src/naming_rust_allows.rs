//! Rust naming lints a module allows with an inner attribute
//! (`#![allow(non_camel_case_types)]`), which covers the module's own file
//! and every submodule file below it. hyper's `ffi` module allows C-style
//! names for the types its generated C header exports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{util, AuditedFile, Language};

const NAMING_LINTS: &[&str] = &[
    "non_camel_case_types",
    "non_snake_case",
    "non_upper_case_globals",
];

/// {relative path: naming lints allowed there} for every Rust file.
pub(super) fn allowed_naming_lints(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> BTreeMap<String, BTreeSet<&'static str>> {
    let own = files
        .iter()
        .filter(|file| file.language == Language::Rust)
        .map(|file| (file.relative.clone(), inner_allows(cache.text(file))))
        .collect::<BTreeMap<PathBuf, BTreeSet<&'static str>>>();
    own.keys()
        .map(|path| {
            let mut lints = BTreeSet::new();
            let mut current = Some(path.clone());
            // Module nesting is shallow; the bound only guards a cycle.
            for _ in 0..32 {
                let Some(module) = current else { break };
                lints.extend(own.get(&module).into_iter().flatten().copied());
                current = parent_module(&module, &own);
            }
            (util::path_slash(path), lints)
        })
        .collect()
}

/// The lint `#![allow(..)]` must name to permit a name breaking `expected`.
pub(super) fn lint_for(expected: &str) -> Option<&'static str> {
    match expected {
        "UpperCamelCase" => Some("non_camel_case_types"),
        "snake_case" => Some("non_snake_case"),
        "SCREAMING_SNAKE_CASE" => Some("non_upper_case_globals"),
        _ => None,
    }
}

fn inner_allows(text: &str) -> BTreeSet<&'static str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("#![allow(") || line.starts_with("#![cfg_attr("))
        .flat_map(|line| {
            NAMING_LINTS
                .iter()
                .copied()
                .filter(move |lint| line.contains(lint))
        })
        .collect()
}

/// The file declaring the module that `path` belongs to: `ffi/mod.rs` or
/// `ffi.rs` for `ffi/body.rs`, and the crate root (`lib.rs`, `main.rs`) or
/// the enclosing `mod.rs` for a module file itself.
fn parent_module(
    path: &Path,
    files: &BTreeMap<PathBuf, BTreeSet<&'static str>>,
) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let directory = path.parent()?;
    let directory = if matches!(stem, "mod" | "lib" | "main") {
        directory.parent()?
    } else {
        directory
    };
    let mut candidates = ["mod.rs", "lib.rs", "main.rs"]
        .iter()
        .map(|name| directory.join(name))
        .collect::<Vec<_>>();
    candidates.push(directory.with_extension("rs"));
    candidates
        .into_iter()
        .find(|candidate| candidate != path && files.contains_key(candidate))
}

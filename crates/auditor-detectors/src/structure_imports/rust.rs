//! The files Rust `use` paths name.
//!
//! A `use` tree's leaf (see [`crate::rust_api_identity::use_leaves`])
//! resolves through
//! the module tree the file layout implies: `crate` is the crate's `src`
//! directory, `self` the importing file's module, each `super` one module
//! up once it has left any inline `mod name { .. }` around the `use`, and a
//! first segment naming a workspace crate that crate's `src`. The longest
//! prefix of the path naming a module file wins, and an item path
//! (`crate::a::B`) resolves only when that file declares or imports `B`, or
//! re-exports a glob. A module placed by `#[path]` resolves by where its
//! file sits, and a path back into the importing file resolves to none.

use std::collections::{BTreeMap, BTreeSet};

use super::super::StructureFile;
use crate::Language;

/// What a file exposes to a `use` of one of its items.
#[derive(Default)]
struct Exposed<'a> {
    /// Names it declares, or imports under a name of its own.
    names: BTreeSet<&'a str>,
    /// Whether it re-exports a glob (`pub use self::x::*`), through which
    /// any name may come.
    glob: bool,
}

/// A crate's `src` directory and its root file.
struct Root {
    dir: String,
    file: String,
}

/// The repository's Rust files, crates, and the names each file exposes.
pub(super) struct RustModules<'a> {
    known: &'a BTreeSet<String>,
    /// Each workspace crate by name (its directory, `-` read as `_`), and
    /// its `src` directory; none when two crates share the name.
    crates: BTreeMap<String, Option<String>>,
    exposed: BTreeMap<&'a str, Exposed<'a>>,
}

impl<'a> RustModules<'a> {
    pub(super) fn new(files: &'a [StructureFile], known: &'a BTreeSet<String>) -> Self {
        let mut crates = BTreeMap::new();
        for path in known {
            if let Some(dir) = path.strip_suffix("/src/lib.rs") {
                let name = dir.rsplit('/').next().unwrap_or(dir).replace('-', "_");
                crates
                    .entry(name)
                    .and_modify(|src: &mut Option<String>| *src = None)
                    .or_insert_with(|| Some(format!("{dir}/src")));
            }
        }
        let mut exposed = BTreeMap::new();
        for file in files.iter().filter(|file| file.language == Language::Rust) {
            let entry: &mut Exposed<'a> = exposed.entry(file.path.as_str()).or_default();
            entry
                .names
                .extend(file.symbols.iter().map(|symbol| symbol.name.as_str()));
            for import in &file.imports {
                match import.alias.as_deref().or(import.imported.as_deref()) {
                    Some("*") => entry.glob |= import.reexport,
                    Some(name) => {
                        entry.names.insert(name);
                    }
                    None => {}
                }
            }
        }
        Self {
            known,
            crates,
            exposed,
        }
    }

    /// The repository file the import at `current` names, inside
    /// `inline_modules` inline modules; none for a path outside the
    /// repository or back into the importing file.
    pub(super) fn resolve(
        &self,
        current: &str,
        canonical: &str,
        inline_modules: usize,
    ) -> Option<String> {
        let mut segments = canonical
            .split("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.last() == Some(&"*") {
            segments.pop();
        }
        let root = self.root(current);
        let module = module_dir(current, &root);
        match *segments.first()? {
            "crate" => self.find(current, &root, &root.dir, &segments[1..], 0),
            "self" if inline_modules == 0 => self.find(current, &root, &module, &segments[1..], 0),
            "self" => None,
            "super" => {
                let supers = segments
                    .iter()
                    .take_while(|segment| **segment == "super")
                    .count();
                let dir = ancestor(module, &root, supers.checked_sub(inline_modules)?)?;
                self.find(current, &root, &dir, &segments[supers..], 0)
            }
            first => self.unqualified(current, &root, &module, first, &segments),
        }
    }

    /// A path whose first segment is no keyword: a workspace crate's, or
    /// one from the module in scope (2018) or the crate root (2015), which
    /// must name a module at least.
    fn unqualified(
        &self,
        current: &str,
        root: &Root,
        module: &str,
        first: &str,
        segments: &[&str],
    ) -> Option<String> {
        match self.crates.get(first) {
            Some(Some(src)) => {
                let root = Root {
                    dir: src.clone(),
                    file: format!("{src}/lib.rs"),
                };
                self.find(current, &root, &root.dir, &segments[1..], 0)
            }
            Some(None) => None,
            None => self
                .find(current, root, module, segments, 1)
                .or_else(|| self.find(current, root, &root.dir, segments, 1)),
        }
    }

    /// The module file of the longest prefix of `rest` under `start`, at
    /// least `min` segments long, when it exposes the item that follows.
    fn find(
        &self,
        current: &str,
        root: &Root,
        start: &str,
        rest: &[&str],
        min: usize,
    ) -> Option<String> {
        for taken in (min..=rest.len()).rev() {
            let dir = rest[..taken]
                .iter()
                .fold(start.to_string(), |dir, segment| child(&dir, segment));
            let Some(file) = self.module_file(&dir, root) else {
                continue;
            };
            if file == current {
                return None;
            }
            let exposes = rest.get(taken).is_none_or(|item| {
                self.exposed
                    .get(file.as_str())
                    .is_some_and(|exposed| exposed.glob || exposed.names.contains(item))
            });
            return exposes.then_some(file);
        }
        None
    }

    /// The file that holds the module whose children sit in `dir`.
    fn module_file(&self, dir: &str, root: &Root) -> Option<String> {
        if dir == root.dir {
            return Some(root.file.clone()).filter(|file| self.known.contains(file));
        }
        [format!("{dir}.rs"), child(dir, "mod.rs")]
            .into_iter()
            .find(|candidate| self.known.contains(candidate))
    }

    /// The crate `current` belongs to: under its last `src` directory, with
    /// `main.rs` its own root and `lib.rs` every other file's; a file under
    /// no `src` (a test, bench, or example) is a crate of its own.
    fn root(&self, current: &str) -> Root {
        let parts = current.split('/').collect::<Vec<_>>();
        let Some(src) = parts.iter().rposition(|part| *part == "src") else {
            return Root {
                dir: parent(current).to_string(),
                file: current.to_string(),
            };
        };
        let dir = parts[..=src].join("/");
        let lib = child(&dir, "lib.rs");
        let file = if current == child(&dir, "main.rs") || !self.known.contains(&lib) {
            child(&dir, "main.rs")
        } else {
            lib
        };
        Root { dir, file }
    }
}

/// The directory a file's child modules sit in: its own for a crate root
/// or `mod.rs`, otherwise one named after it.
fn module_dir(current: &str, root: &Root) -> String {
    let (dir, name) = current.rsplit_once('/').unwrap_or(("", current));
    if current == root.file || name == "mod.rs" {
        return dir.to_string();
    }
    child(dir, name.strip_suffix(".rs").unwrap_or(name))
}

/// The module directory `levels` above `dir`; none above the crate root.
fn ancestor(mut dir: String, root: &Root, levels: usize) -> Option<String> {
    for _ in 0..levels {
        if dir == root.dir {
            return None;
        }
        dir = parent(&dir).to_string();
    }
    Some(dir)
}

fn child(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

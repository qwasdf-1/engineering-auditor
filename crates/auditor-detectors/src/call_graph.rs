//! Resolution of calls to the repository functions they invoke, where the
//! target is certain without type information. Findings-free.
//!
//! A call resolves only to exactly one candidate:
//! - a bare call to a function in the same file, then to an imported
//!   function (`use`, `from .. import`, ES `import`), then, in Go, to a
//!   function in another file of the same package;
//! - a `self`/`this` call to a method of the caller's own class;
//! - a `Type.method` call to a method of a class or object named `Type` in
//!   the same file, the same package directory, or an imported file;
//! - a `module.function` call to a function in the file an imported module
//!   alias resolves to.
//!
//! A call on any other receiver needs the receiver's type and stays
//! unresolved, so an effect is never inherited through a guessed target.

use std::collections::HashMap;

use auditor_ast::SymbolKind;

use crate::structure_index::{StructureCall, StructureFile, StructureIndex};
use crate::Language;

/// A function or method: (file index, symbol index) in the structure index.
pub(crate) type SymbolRef = (usize, usize);

pub(crate) struct CallGraph {
    /// (file index, call index) to the call's resolved target.
    targets: HashMap<(usize, usize), SymbolRef>,
}

impl CallGraph {
    pub(crate) fn build(structure: &StructureIndex) -> Self {
        let files = structure.files();
        let lookup = Lookup::new(files);
        let mut targets = HashMap::new();
        for (file_index, file) in files.iter().enumerate() {
            for (call_index, call) in file.calls.iter().enumerate() {
                if let Some(target) = lookup.resolve(file_index, call) {
                    targets.insert((file_index, call_index), target);
                }
            }
        }
        Self { targets }
    }

    pub(crate) fn target(&self, file: usize, call: usize) -> Option<SymbolRef> {
        self.targets.get(&(file, call)).copied()
    }
}

struct Lookup<'a> {
    files: &'a [StructureFile],
    by_path: HashMap<&'a str, usize>,
    by_package: HashMap<(&'a str, &'a str), Vec<usize>>,
}

impl<'a> Lookup<'a> {
    fn new(files: &'a [StructureFile]) -> Self {
        let mut by_path = HashMap::new();
        let mut by_package: HashMap<_, Vec<usize>> = HashMap::new();
        for (index, file) in files.iter().enumerate() {
            by_path.insert(file.path.as_str(), index);
            by_package
                .entry((directory(&file.path), file.language.as_str()))
                .or_default()
                .push(index);
        }
        Self {
            files,
            by_path,
            by_package,
        }
    }

    fn resolve(&self, file_index: usize, call: &StructureCall) -> Option<SymbolRef> {
        let owner = call.owner? as usize;
        let name = call.callee.as_str();
        match call.receiver.as_deref() {
            None => self
                .free_function(file_index, name)
                .or_else(|| self.implicit_method(file_index, owner, name))
                .or_else(|| self.imported_function(file_index, name))
                .or_else(|| self.package_function(file_index, name)),
            Some("self" | "this" | "Self") => self.own_method(file_index, owner, name),
            Some(receiver) => self
                .type_method(file_index, receiver, name)
                .or_else(|| self.module_function(file_index, receiver, name)),
        }
    }

    /// A function (not a method) named `name` declared in the file.
    fn free_function(&self, file_index: usize, name: &str) -> Option<SymbolRef> {
        unique(
            self.files[file_index]
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, symbol)| symbol.name == name && symbol.kind == SymbolKind::Function)
                .map(|(index, _)| (file_index, index)),
        )
    }

    fn imported_function(&self, file_index: usize, name: &str) -> Option<SymbolRef> {
        unique(self.files[file_index].imports.iter().filter_map(|import| {
            let bound = import.alias.as_deref().or(import.imported.as_deref())?;
            if bound != name {
                return None;
            }
            let target = *self.by_path.get(import.resolved_path.as_deref()?)?;
            let original = import.imported.as_deref().unwrap_or(name);
            self.free_function(target, original)
        }))
    }

    /// Go: a function in another file of the same package directory.
    fn package_function(&self, file_index: usize, name: &str) -> Option<SymbolRef> {
        let file = &self.files[file_index];
        if file.language != Language::Go {
            return None;
        }
        unique(
            self.package_files(file)
                .filter(|other| *other != file_index)
                .filter_map(|other| self.free_function(other, name)),
        )
    }

    /// Java, Kotlin, C#, and C++: a bare call inside a method may call a
    /// method of the same class (an implicit `this`).
    fn implicit_method(&self, file_index: usize, owner: usize, name: &str) -> Option<SymbolRef> {
        let implicit_this = matches!(
            self.files[file_index].language,
            Language::Java | Language::Kotlin | Language::CSharp | Language::Cpp
        );
        implicit_this
            .then(|| self.own_method(file_index, owner, name))
            .flatten()
    }

    fn own_method(&self, file_index: usize, owner: usize, name: &str) -> Option<SymbolRef> {
        let symbols = &self.files[file_index].symbols;
        let class = symbols.get(owner)?.parent?;
        unique(
            symbols
                .iter()
                .enumerate()
                .filter(|(_, symbol)| {
                    symbol.parent == Some(class) && symbol.name == name && is_callable(symbol.kind)
                })
                .map(|(index, _)| (file_index, index)),
        )
    }

    /// A method of a class or object named `receiver`, declared in this
    /// file, this package directory, or a file this file imports.
    fn type_method(&self, file_index: usize, receiver: &str, name: &str) -> Option<SymbolRef> {
        let file = &self.files[file_index];
        let imported = file
            .imports
            .iter()
            .filter_map(|import| self.by_path.get(import.resolved_path.as_deref()?).copied());
        let mut candidates: Vec<usize> = self.package_files(file).chain(imported).collect();
        candidates.sort_unstable();
        candidates.dedup();
        unique(candidates.into_iter().flat_map(|candidate| {
            let symbols = &self.files[candidate].symbols;
            symbols
                .iter()
                .enumerate()
                .filter(move |(_, symbol)| {
                    symbol.name == name
                        && is_callable(symbol.kind)
                        && symbol.parent.is_some_and(|parent| {
                            symbols
                                .get(parent as usize)
                                .is_some_and(|class| class.name == receiver && is_type(class.kind))
                        })
                })
                .map(move |(index, _)| (candidate, index))
        }))
    }

    /// `module.function` where `module` is an import binding of a file.
    fn module_function(&self, file_index: usize, receiver: &str, name: &str) -> Option<SymbolRef> {
        unique(self.files[file_index].imports.iter().filter_map(|import| {
            let bound = import.alias.as_deref().or(import.imported.as_deref())?;
            if bound != receiver {
                return None;
            }
            let target = *self.by_path.get(import.resolved_path.as_deref()?)?;
            self.free_function(target, name)
        }))
    }

    fn package_files(&self, file: &'a StructureFile) -> impl Iterator<Item = usize> + '_ {
        self.by_package
            .get(&(directory(&file.path), file.language.as_str()))
            .into_iter()
            .flatten()
            .copied()
    }
}

/// A function as `name (path:line)`.
pub(crate) fn describe(
    structure: &StructureIndex,
    (file_index, symbol_index): SymbolRef,
) -> String {
    let file = &structure.files()[file_index];
    let symbol = &file.symbols[symbol_index];
    format!("{} ({}:{})", symbol.name, file.path, symbol.line)
}

fn unique(mut candidates: impl Iterator<Item = SymbolRef>) -> Option<SymbolRef> {
    let first = candidates.next()?;
    candidates.all(|other| other == first).then_some(first)
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Object | SymbolKind::Struct | SymbolKind::Enum
    )
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

#[cfg(test)]
#[path = "call_graph_tests.rs"]
mod tests;

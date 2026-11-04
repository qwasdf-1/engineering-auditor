//! Each repository type's first written base, resolved once for every
//! consumer: class metrics, type facts, the topology, the call graph, and
//! entry points. Findings-free.
//!
//! A base resolves, in order:
//! - to a type of that name declared in the same file;
//! - through the import that binds the name, or the first segment of its
//!   qualifier (`mp.Process`, `models.Base`): to the type the imported
//!   repository file declares, or, for a module outside the repository, to
//!   the external type by its canonical name (`multiprocessing.Process`);
//! - in Java, Kotlin, and C#, whose files see their package's or namespace's
//!   types without an import: to the only type of that name in the same
//!   directory, then in the repository.
//!
//! Python, JavaScript, and TypeScript see no type they do not import, so a
//! base bound nowhere is external under its written name (`Exception`,
//! `Error`). A base bound to a repository file that does not declare it (a
//! package re-export) stays unknown.

use std::collections::{BTreeSet, HashMap};

use auditor_ast::SymbolKind;

use super::{StructureFile, StructureImport, StructureSymbol};
use crate::Language;

/// A type: (file index, symbol index) in the structure index.
pub(crate) type TypeRef = (usize, usize);

/// Bases followed above one type; deeper hierarchies are cut off.
const MAX_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BaseTarget {
    Repository(TypeRef),
    /// A type outside the repository by its canonical or written name.
    External(String),
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedBase {
    pub(crate) target: BaseTarget,
    /// The repository file the binding import resolves to, when an import
    /// binds the base.
    pub(crate) import_file: Option<usize>,
}

#[derive(Default)]
pub(crate) struct TypeHierarchy {
    bases: HashMap<TypeRef, ResolvedBase>,
}

impl TypeHierarchy {
    pub(crate) fn build(files: &[StructureFile]) -> Self {
        let resolver = Resolver::new(files);
        let mut bases = HashMap::new();
        for (file_index, file) in files.iter().enumerate() {
            for (symbol_index, symbol) in file.symbols.iter().enumerate() {
                if !is_type(symbol.kind) || symbol.name.is_empty() {
                    continue;
                }
                if let Some(base) = resolver.resolve(file_index, symbol_index, symbol) {
                    bases.insert((file_index, symbol_index), base);
                }
            }
        }
        Self { bases }
    }

    /// The resolved first base of `class`, if it writes one.
    pub(crate) fn base(&self, class: TypeRef) -> Option<&ResolvedBase> {
        self.bases.get(&class)
    }

    /// The repository type `class` directly extends.
    pub(crate) fn repository_base(&self, class: TypeRef) -> Option<TypeRef> {
        match self.base(class)?.target {
            BaseTarget::Repository(base) => Some(base),
            _ => None,
        }
    }

    /// `class`, then its repository ancestors, nearest first; stops at a
    /// cycle or after [`MAX_DEPTH`] bases.
    pub(crate) fn lineage(&self, class: TypeRef) -> Vec<TypeRef> {
        let mut seen = BTreeSet::from([class]);
        let mut lineage = vec![class];
        let mut current = class;
        while let Some(base) = self.repository_base(current) {
            if lineage.len() > MAX_DEPTH || !seen.insert(base) {
                break;
            }
            lineage.push(base);
            current = base;
        }
        lineage
    }

    /// The external type at the top of `class`'s repository lineage.
    pub(crate) fn external_root(&self, class: TypeRef) -> Option<&str> {
        let top = *self.lineage(class).last()?;
        match &self.base(top)?.target {
            BaseTarget::External(name) => Some(name),
            _ => None,
        }
    }

    /// DIT: the bases above `class`, counting the first one the repository
    /// does not declare.
    pub(crate) fn depth(&self, class: TypeRef) -> usize {
        let lineage = self.lineage(class);
        let top = *lineage.last().expect("lineage starts with the class");
        let beyond = self
            .base(top)
            .is_some_and(|base| !matches!(base.target, BaseTarget::Repository(_)));
        lineage.len() - 1 + usize::from(beyond)
    }
}

struct Resolver<'a> {
    files: &'a [StructureFile],
    by_path: HashMap<&'a str, usize>,
    by_name: HashMap<&'a str, Vec<TypeRef>>,
}

impl<'a> Resolver<'a> {
    fn new(files: &'a [StructureFile]) -> Self {
        let mut by_path = HashMap::new();
        let mut by_name: HashMap<&str, Vec<TypeRef>> = HashMap::new();
        for (file_index, file) in files.iter().enumerate() {
            by_path.insert(file.path.as_str(), file_index);
            for (symbol_index, symbol) in file.symbols.iter().enumerate() {
                if is_type(symbol.kind) && !symbol.name.is_empty() {
                    by_name
                        .entry(symbol.name.as_str())
                        .or_default()
                        .push((file_index, symbol_index));
                }
            }
        }
        Self {
            files,
            by_path,
            by_name,
        }
    }

    fn resolve(
        &self,
        file_index: usize,
        symbol_index: usize,
        symbol: &StructureSymbol,
    ) -> Option<ResolvedBase> {
        let name = symbol.base.as_deref()?;
        let qualifier = symbol.base_qualifier.as_deref();
        let file = &self.files[file_index];
        if qualifier.is_none() {
            if let Some(local) = self.declared_in(file_index, name, Some(symbol_index)) {
                return Some(resolved(BaseTarget::Repository(local), None));
            }
        }
        let bound = qualifier.map_or(name, |qualifier| {
            qualifier.split('.').next().unwrap_or(qualifier)
        });
        if let Some(import) = file.imports.iter().find(|import| binds(import, bound)) {
            return Some(self.through_import(import, qualifier, name));
        }
        if let Some(star) = self.star_import(file, name) {
            return Some(star);
        }
        let written = qualifier.map_or_else(|| name.to_string(), |q| format!("{q}.{name}"));
        let target = if sees_package_types(&file.language) {
            self.package_type(file_index, name)
                .or_else(|| self.unique_type(name))
                .map_or(BaseTarget::External(written), BaseTarget::Repository)
        } else {
            BaseTarget::External(written)
        };
        Some(resolved(target, None))
    }

    /// The base an import binds: `name` itself (`from m import Base`), or
    /// the module its qualifier starts with (`import m as q` for `q.Base`).
    fn through_import(
        &self,
        import: &StructureImport,
        qualifier: Option<&str>,
        name: &str,
    ) -> ResolvedBase {
        let import_file = import
            .resolved_path
            .as_deref()
            .and_then(|path| self.by_path.get(path).copied());
        let rest = qualifier.and_then(|qualifier| qualifier.split_once('.').map(|(_, rest)| rest));
        let target = match import_file {
            // `pkg.sub.Base` through `import pkg` names a type of another
            // module than the one the import resolves to.
            Some(_) if rest.is_some() => BaseTarget::Unknown,
            Some(target) => {
                let declared = if qualifier.is_some() {
                    name
                } else {
                    import.imported.as_deref().unwrap_or(name)
                };
                self.declared_in(target, declared, None)
                    .map_or(BaseTarget::Unknown, BaseTarget::Repository)
            }
            // A relative module (`.models`, `./base`) is the repository's own,
            // even where its path does not resolve.
            None if is_relative(import) => BaseTarget::Unknown,
            None => {
                let mut canonical = imported_name(import);
                if qualifier.is_some() {
                    for segment in rest.into_iter().chain([name]) {
                        canonical.push('.');
                        canonical.push_str(segment);
                    }
                }
                BaseTarget::External(canonical)
            }
        };
        resolved(target, import_file)
    }

    /// A type `name` declared in a repository file the file star-imports
    /// (`from .base import *`).
    fn star_import(&self, file: &StructureFile, name: &str) -> Option<ResolvedBase> {
        file.imports
            .iter()
            .filter(|import| import.imported.as_deref() == Some("*"))
            .find_map(|import| {
                let target = *self.by_path.get(import.resolved_path.as_deref()?)?;
                let class = self.declared_in(target, name, None)?;
                Some(resolved(BaseTarget::Repository(class), Some(target)))
            })
    }

    /// The first type `name` declared in a file, other than `except`.
    fn declared_in(&self, file: usize, name: &str, except: Option<usize>) -> Option<TypeRef> {
        self.files[file]
            .symbols
            .iter()
            .enumerate()
            .find(|(index, symbol)| {
                Some(*index) != except && symbol.name == name && is_type(symbol.kind)
            })
            .map(|(index, _)| (file, index))
    }

    /// The only type `name` in another file of the same directory.
    fn package_type(&self, file_index: usize, name: &str) -> Option<TypeRef> {
        let directory = directory(&self.files[file_index].path);
        unique(
            self.by_name
                .get(name)?
                .iter()
                .filter(|(file, _)| {
                    *file != file_index && directory_of(self.files, *file) == directory
                })
                .copied(),
        )
    }

    fn unique_type(&self, name: &str) -> Option<TypeRef> {
        match self.by_name.get(name)?.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }
}

fn resolved(target: BaseTarget, import_file: Option<usize>) -> ResolvedBase {
    ResolvedBase {
        target,
        import_file,
    }
}

/// The name an import binds in its file: its alias, else what it imports.
fn binds(import: &StructureImport, name: &str) -> bool {
    match import.alias.as_deref() {
        Some(alias) => alias == name,
        None => import.imported.as_deref() == Some(name),
    }
}

/// An import's canonical dotted name (`multiprocessing.Process` for
/// `from multiprocessing import Process`, `react.Component` for
/// `import { Component } from 'react'`).
fn imported_name(import: &StructureImport) -> String {
    let canonical = import
        .canonical
        .as_deref()
        .or(import.module.as_deref())
        .unwrap_or_default()
        .replace("::", ".");
    match import.imported.as_deref() {
        Some(imported)
            if imported != "*"
                && canonical != imported
                && !canonical.ends_with(&format!(".{imported}")) =>
        {
            if canonical.is_empty() {
                imported.to_string()
            } else {
                format!("{canonical}.{imported}")
            }
        }
        _ => canonical,
    }
}

fn is_relative(import: &StructureImport) -> bool {
    import
        .module
        .as_deref()
        .or(import.canonical.as_deref())
        .is_some_and(|module| module.starts_with('.'))
}

fn sees_package_types(language: &Language) -> bool {
    matches!(
        language,
        Language::Java | Language::Kotlin | Language::CSharp
    )
}

pub(crate) fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Object
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Interface
            | SymbolKind::Trait
    )
}

fn unique(mut candidates: impl Iterator<Item = TypeRef>) -> Option<TypeRef> {
    let first = candidates.next()?;
    candidates.all(|other| other == first).then_some(first)
}

fn directory_of(files: &[StructureFile], file: usize) -> &str {
    directory(&files[file].path)
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

#[cfg(test)]
#[path = "type_hierarchy_tests.rs"]
mod tests;

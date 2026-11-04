//! Resolution of calls to the repository functions they invoke, where the
//! target is certain without type information. Findings-free.
//!
//! A call's receiver is the parsed one (Java), else the callee's prefix before
//! its last `.`, `::`, or `->` when that prefix is one name or `super()`
//! (`self.load`, `pkg.Func`, `Type::new`); a longer chain needs types. A call
//! resolves only to exactly one candidate:
//! - a bare call to a function in the same file, then to an imported
//!   function (`use`, `from .. import`, ES `import`), then, in Go, to a
//!   function in another file of the same package;
//! - a `self`/`this`/`cls` call (and, in Java, Kotlin, C#, and C++, a bare
//!   one) to a method of the caller's class or, through the type hierarchy,
//!   of its nearest repository ancestor declaring one;
//! - a `super`/`base` call to a method of the nearest ancestor declaring one;
//! - a `Type.method` call to a method a class named `Type` declares or
//!   inherits, the class declared in the same file, the same package
//!   directory, or an imported file;
//! - a `module.function` call to a function in the file (Go: the package)
//!   an imported module alias resolves to.
//!
//! A call on any other receiver needs the receiver's type and stays
//! unresolved, so an effect is never inherited through a guessed target.
//! Dispatch is static: a `self` call resolves to the method the caller's
//! class sees, not to an override in a subclass.

use std::collections::HashMap;

use auditor_ast::SymbolKind;

use crate::structure_index::hierarchy::{is_type, TypeHierarchy, TypeRef};
use crate::structure_index::{StructureCall, StructureFile, StructureImport, StructureIndex};
use crate::Language;

/// A function or method: (file index, symbol index) in the structure index.
pub(crate) type SymbolRef = (usize, usize);

/// Parents followed from a function to the type that encloses it.
const MAX_NESTING: usize = 16;

pub(crate) struct CallGraph {
    /// (file index, call index) to the call's resolved target.
    targets: HashMap<(usize, usize), SymbolRef>,
}

impl CallGraph {
    pub(crate) fn build(structure: &StructureIndex) -> Self {
        let lookup = Lookup::new(structure);
        let mut targets = HashMap::new();
        for (file_index, file) in structure.files().iter().enumerate() {
            for (call_index, call) in file.calls.iter().enumerate() {
                if let Some(target) = lookup.resolve_call(file_index, call) {
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

/// A name a function refers to: `name`, `receiver.name`, or `receiver::name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NameRef<'n> {
    pub(crate) receiver: Option<&'n str>,
    pub(crate) name: &'n str,
}

impl<'n> NameRef<'n> {
    /// `text` split at its last `.`, `::`, or `->`, if its receiver is one
    /// name or `super()`; `None` for a longer chain.
    pub(crate) fn parse(text: &'n str) -> Option<Self> {
        let split = [".", "::", "->"]
            .iter()
            .filter_map(|separator| {
                text.rfind(separator)
                    .map(|index| (index, index + separator.len()))
            })
            .max_by_key(|(index, _)| *index);
        let Some((end, start)) = split else {
            return Some(Self {
                receiver: None,
                name: text,
            });
        };
        let receiver = &text[..end];
        let name = &text[start..];
        (simple_receiver(receiver) && is_identifier(name)).then_some(Self {
            receiver: Some(receiver),
            name,
        })
    }

    fn of_call(call: &'n StructureCall) -> Option<Self> {
        match call.receiver.as_deref() {
            Some(receiver) => {
                let name = call
                    .callee
                    .rsplit(['.', ':'])
                    .find(|part| !part.is_empty())?;
                Some(Self {
                    receiver: Some(receiver),
                    name,
                })
            }
            None => Self::parse(&call.callee),
        }
    }
}

pub(crate) struct Lookup<'a> {
    files: &'a [StructureFile],
    hierarchy: &'a TypeHierarchy,
    by_path: HashMap<&'a str, usize>,
    by_package: HashMap<(&'a str, &'a str), Vec<usize>>,
}

impl<'a> Lookup<'a> {
    pub(crate) fn new(structure: &'a StructureIndex) -> Self {
        let files = structure.files();
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
            hierarchy: structure.hierarchy(),
            by_path,
            by_package,
        }
    }

    fn resolve_call(&self, file_index: usize, call: &StructureCall) -> Option<SymbolRef> {
        let owner = call.owner? as usize;
        self.resolve(file_index, Some(owner), NameRef::of_call(call)?)
    }

    /// The function `reference` names inside function `owner` of a file
    /// (`None`: at file level, where no class encloses it).
    pub(crate) fn resolve(
        &self,
        file_index: usize,
        owner: Option<usize>,
        reference: NameRef<'_>,
    ) -> Option<SymbolRef> {
        let name = reference.name;
        match reference.receiver {
            None => self
                .free_function(file_index, name)
                .or_else(|| self.implicit_method(file_index, owner?, name))
                .or_else(|| self.imported_function(file_index, name))
                .or_else(|| self.package_function(file_index, name)),
            Some("self" | "this" | "Self" | "cls") => {
                self.class_method(self.enclosing_type(file_index, owner?)?, name)
            }
            Some("super" | "super()" | "base") => {
                let class = self.enclosing_type(file_index, owner?)?;
                self.class_method(self.hierarchy.repository_base(class)?, name)
            }
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
            .then(|| self.class_method(self.enclosing_type(file_index, owner)?, name))
            .flatten()
    }

    /// The type whose method (or a function nested in one) `owner` is.
    fn enclosing_type(&self, file_index: usize, owner: usize) -> Option<TypeRef> {
        let symbols = &self.files[file_index].symbols;
        let mut current = symbols.get(owner)?.parent;
        for _ in 0..MAX_NESTING {
            let index = current? as usize;
            let symbol = symbols.get(index)?;
            if is_type(symbol.kind) {
                return Some((file_index, index));
            }
            current = symbol.parent;
        }
        None
    }

    /// The method `name` that `class` declares, else the one its nearest
    /// repository ancestor declares. A class declaring several (overloads)
    /// resolves to none.
    fn class_method(&self, class: TypeRef, name: &str) -> Option<SymbolRef> {
        self.hierarchy
            .lineage(class)
            .into_iter()
            .map(|(file, type_index)| self.declared_methods(file, type_index, name))
            .find(|declared| !declared.is_empty())
            .and_then(|declared| unique(declared.into_iter()))
    }

    fn declared_methods(&self, file: usize, class: usize, name: &str) -> Vec<SymbolRef> {
        self.files[file]
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                symbol.parent == Some(class as u32)
                    && symbol.name == name
                    && is_callable(symbol.kind)
            })
            .map(|(index, _)| (file, index))
            .collect()
    }

    /// A method that a class or object named `receiver`, declared in this
    /// file, this package directory, or a file this file imports, declares
    /// or inherits.
    fn type_method(&self, file_index: usize, receiver: &str, name: &str) -> Option<SymbolRef> {
        let file = &self.files[file_index];
        let imported = file
            .imports
            .iter()
            .filter_map(|import| self.by_path.get(import.resolved_path.as_deref()?).copied());
        let mut candidates: Vec<usize> = self.package_files(file).chain(imported).collect();
        candidates.sort_unstable();
        candidates.dedup();
        let classes = candidates.into_iter().flat_map(|candidate| {
            self.files[candidate]
                .symbols
                .iter()
                .enumerate()
                .filter(move |(_, symbol)| symbol.name == receiver && is_class(symbol.kind))
                .map(move |(index, _)| (candidate, index))
        });
        let mut methods = Vec::new();
        for class in classes {
            methods.push(self.class_method(class, name)?);
        }
        unique(methods.into_iter())
    }

    /// `module.function` where `module` is an import binding of a file; in
    /// Go, any file of the imported package.
    fn module_function(&self, file_index: usize, receiver: &str, name: &str) -> Option<SymbolRef> {
        let file = &self.files[file_index];
        unique(file.imports.iter().filter_map(|import| {
            if bound_name(import, &file.language)? != receiver {
                return None;
            }
            let target = *self.by_path.get(import.resolved_path.as_deref()?)?;
            let target_file = &self.files[target];
            if target_file.language == Language::Go {
                unique(
                    self.package_files(target_file)
                        .filter_map(|other| self.free_function(other, name)),
                )
            } else {
                self.free_function(target, name)
            }
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

/// The name an import binds: its alias, else what it imports; a Go import
/// binds its path's last segment (`example.com/app/store` binds `store`,
/// `github.com/x/y/v2` binds `y`).
fn bound_name<'i>(import: &'i StructureImport, language: &Language) -> Option<&'i str> {
    import
        .alias
        .as_deref()
        .or(import.imported.as_deref())
        .or_else(|| {
            let module = import
                .module
                .as_deref()
                .filter(|_| *language == Language::Go)?;
            let mut segments = module.rsplit('/');
            let last = segments.next()?;
            let version = last.len() > 1
                && last.starts_with('v')
                && last[1..]
                    .chars()
                    .all(|character| character.is_ascii_digit());
            if version {
                segments.next()
            } else {
                Some(last)
            }
        })
}

/// A type whose methods a `Type.method` call may name.
fn is_class(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Object | SymbolKind::Struct | SymbolKind::Enum
    )
}

fn simple_receiver(receiver: &str) -> bool {
    receiver == "super()" || is_identifier(receiver)
}

fn is_identifier(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || matches!(first, '_' | '$'))
        && text
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '$'))
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

#[cfg(test)]
#[path = "call_graph_tests.rs"]
mod tests;

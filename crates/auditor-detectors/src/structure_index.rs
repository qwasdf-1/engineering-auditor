use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::path::{Path, PathBuf};

use auditor_ast::{ControlRegionKind, ReferenceKind, SourceSpan, SymbolKind};

use crate::ast_scan::{self, AstScan};
use crate::{util, Language, SourceRole};

#[path = "structure_batches.rs"]
mod batches;
#[path = "structure_build.rs"]
mod build;
#[path = "structure_declared_metrics.rs"]
mod declared_metrics;
#[path = "structure_imports.rs"]
mod imports;
#[path = "structure_languages.rs"]
mod languages;
#[path = "structure_operands.rs"]
mod operands;
#[path = "structure_spans.rs"]
mod spans;
use build::build_structure_file;
#[cfg(test)]
use imports::normalize_path;
use imports::{first_quoted, import_bindings, parse_import, resolve_imports};
use spans::{
    import_has_documented_reason, module_name, python_guarded_import_spans, python_memoized_spans,
    python_overload_spans, python_type_checking_spans, rust_associated_spans,
    rust_implicitly_used_spans, rust_test_spans,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReferenceResolution {
    Referenced,
    Unreferenced,
    Unresolved,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureSymbol {
    pub(crate) kind: SymbolKind,
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) span: SourceSpan,
    pub(crate) name_span: Option<SourceSpan>,
    pub(crate) parent: Option<u32>,
    pub(crate) declaration: Option<String>,
    pub(crate) base: Option<String>,
    pub(crate) is_async: bool,
    pub(crate) is_public: bool,
    /// Rust `pub(crate)`, `pub(super)`, or `pub(in ..)`: public inside the
    /// crate only.
    pub(crate) visibility_restricted: bool,
    pub(crate) in_test: bool,
    /// Reached without a by-name reference: a Rust trait-impl associated item
    /// or an item bound by an attribute such as `#[global_allocator]`.
    pub(crate) implicitly_used: bool,
    /// A Python `@overload` stub: a typing declaration whose implementation
    /// is a separate, undecorated definition.
    pub(crate) overload_stub: bool,
    /// A Python function whose results are cached per argument
    /// (`@functools.cache`, `@lru_cache`).
    pub(crate) memoized: bool,
    /// A function defined in a Rust `impl` block: an associated item reached
    /// through its type, not its module.
    pub(crate) associated: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureReference {
    pub(crate) kind: ReferenceKind,
    pub(crate) name: String,
    pub(crate) span: SourceSpan,
    pub(crate) owner: Option<u32>,
    pub(crate) control_region: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureImport {
    pub(crate) canonical: Option<String>,
    pub(crate) imported: Option<String>,
    pub(crate) alias: Option<String>,
    pub(crate) resolved_path: Option<String>,
    pub(crate) owner: Option<u32>,
    pub(crate) line: usize,
    pub(crate) dynamic: bool,
    pub(crate) justified: bool,
    pub(crate) reexport: bool,
    pub(crate) module: Option<String>,
    pub(crate) form: ImportForm,
    /// The C/C++ preprocessor branches enclosing the import, outermost
    /// first: two includes in exclusive branches (`#ifdef A` .. `#else`)
    /// never both apply.
    pub(crate) branches: Vec<usize>,
}

/// Import shapes that cannot share one declaration: TypeScript keeps
/// `import type` apart from value imports, and a namespace binding
/// (`* as ns`) cannot be combined with named bindings. A CommonJS member
/// require (`require('./utils').methods`) binds one member per line by
/// design; no lint convention asks to merge those.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ImportForm {
    #[default]
    Value,
    Type,
    Namespace,
    TypeNamespace,
    Member,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureCall {
    pub(crate) callee: String,
    pub(crate) receiver: Option<String>,
    pub(crate) arguments: Option<String>,
    pub(crate) expression: String,
    pub(crate) span: SourceSpan,
    pub(crate) owner: Option<u32>,
    pub(crate) control_region: Option<u32>,
    pub(crate) line: usize,
    /// The call's value is consumed rather than evaluated for effect.
    pub(crate) result_consumed: bool,
    /// The callee's resolved API identity (`sort.Ints`, `os/exec.Command`),
    /// where the language's import bindings are resolved (Go, Java, JS/TS).
    pub(crate) api: Option<String>,
    /// The expression consuming a JS/TS call's value through member or index
    /// access (`xs.filter(p).length>0`), compacted.
    pub(crate) consumer: Option<String>,
}

/// An index or slice expression (`items[0]`, `items[:5]`).
#[derive(Clone, Debug)]
pub(crate) struct StructureSubscript {
    pub(crate) operand: String,
    /// `Some(index)` for an index expression, `None` for a slice.
    pub(crate) index: Option<String>,
    /// The slice bounds `(start, end)`; empty when omitted.
    pub(crate) bounds: (String, String),
    pub(crate) span: SourceSpan,
    pub(crate) owner: Option<u32>,
}

/// A Python membership test (`needle in collection`).
#[derive(Clone, Debug)]
pub(crate) struct StructureMembership {
    pub(crate) needle: String,
    pub(crate) collection: String,
    pub(crate) span: SourceSpan,
    pub(crate) owner: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureAssignment {
    pub(crate) target: String,
    pub(crate) value: Option<String>,
    pub(crate) expression: String,
    pub(crate) span: SourceSpan,
    pub(crate) owner: Option<u32>,
    pub(crate) control_region: Option<u32>,
    pub(crate) line: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureLiteral {
    pub(crate) text: String,
    pub(crate) owner: Option<u32>,
    pub(crate) control_region: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureControl {
    pub(crate) owner: Option<u32>,
    pub(crate) line: usize,
    pub(crate) span: SourceSpan,
    pub(crate) condition: Option<String>,
    pub(crate) parent: Option<u32>,
    pub(crate) depth: usize,
    pub(crate) kind: ControlRegionKind,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureLoop {
    pub(crate) owner: Option<u32>,
    pub(crate) binding: Vec<String>,
    pub(crate) iterable: Option<String>,
    pub(crate) iterable_span: Option<SourceSpan>,
    pub(crate) span: SourceSpan,
    pub(crate) control_region: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StructureFunctionMetrics {
    pub(crate) locals: usize,
    pub(crate) returns: usize,
    pub(crate) statements: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct StructureFile {
    pub(crate) path: String,
    pub(crate) language: Language,
    pub(crate) emittable: bool,
    /// The name callers qualify this file's items with: the Go package, or the
    /// Rust or Python module. `None` for crate roots and entry points.
    pub(crate) module: Option<String>,
    pub(crate) symbols: Vec<StructureSymbol>,
    pub(crate) references: Vec<StructureReference>,
    pub(crate) imports: Vec<StructureImport>,
    pub(crate) calls: Vec<StructureCall>,
    pub(crate) assignments: Vec<StructureAssignment>,
    pub(crate) literals: Vec<StructureLiteral>,
    pub(crate) controls: Vec<StructureControl>,
    pub(crate) loops: Vec<StructureLoop>,
    pub(crate) subscripts: Vec<StructureSubscript>,
    pub(crate) memberships: Vec<StructureMembership>,
    pub(crate) function_metrics: BTreeMap<u32, StructureFunctionMetrics>,
    /// The Go module a Go file belongs to, from its nearest `go.mod`.
    pub(crate) go_module: Option<GoModule>,
    /// Callbacks that run on a shared asynchronous executor (a Java lambda
    /// passed to `CompletableFuture.supplyAsync` without an executor).
    pub(crate) async_spans: Vec<SourceSpan>,
}

/// A Go module: its import path and its directory relative to the scan root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GoModule {
    pub(crate) path: String,
    pub(crate) root: String,
}

pub(crate) struct StructureIndex {
    files: Vec<StructureFile>,
    symbols_by_name: BTreeMap<String, Vec<(usize, usize)>>,
}

#[derive(Clone, Copy)]
struct ResolutionTarget<'a> {
    file_index: usize,
    file: &'a StructureFile,
    symbol: &'a StructureSymbol,
    candidate_count: usize,
}

impl StructureIndex {
    pub(crate) fn build(scan: AstScan<'_, '_>) -> Self {
        let mut go_modules = BTreeMap::new();
        let mut files = ast_scan::map_ast_files(scan, Language::is_source, |file, tree, _| {
            (file, build_structure_file(file, tree))
        })
        .into_iter()
        .map(|(file, mut structure)| {
            if file.language == Language::Go {
                structure.go_module = go_module_for(file, &mut go_modules);
            }
            structure
        })
        .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        resolve_imports(&mut files);
        let mut symbols_by_name: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
        for (file_index, file) in files.iter().enumerate() {
            for (symbol_index, symbol) in file.symbols.iter().enumerate() {
                if !symbol.name.is_empty() {
                    symbols_by_name
                        .entry(symbol.name.clone())
                        .or_default()
                        .push((file_index, symbol_index));
                }
            }
        }
        Self {
            files,
            symbols_by_name,
        }
    }

    pub(crate) fn files(&self) -> &[StructureFile] {
        &self.files
    }

    #[cfg(test)]
    pub(crate) fn dynamic_import_count(&self) -> usize {
        self.files
            .iter()
            .flat_map(|file| &file.imports)
            .filter(|import| import.dynamic)
            .count()
    }

    pub(crate) fn reference_resolution(
        &self,
        file_index: usize,
        symbol_index: usize,
    ) -> ReferenceResolution {
        let file = &self.files[file_index];
        let symbol = &file.symbols[symbol_index];
        let Some(candidates) = self.symbols_by_name.get(&symbol.name) else {
            return ReferenceResolution::Unreferenced;
        };
        let target = ResolutionTarget {
            file_index,
            file,
            symbol,
            candidate_count: candidates.len(),
        };
        let mut unresolved = false;
        for resolution in matching_reference_resolutions(&self.files, target) {
            match resolution {
                ReferenceResolution::Referenced => return ReferenceResolution::Referenced,
                ReferenceResolution::Unresolved => unresolved = true,
                ReferenceResolution::Unreferenced => {}
            }
        }
        if unresolved {
            ReferenceResolution::Unresolved
        } else {
            ReferenceResolution::Unreferenced
        }
    }
}

fn matching_reference_resolutions<'a>(
    files: &'a [StructureFile],
    target: ResolutionTarget<'a>,
) -> impl Iterator<Item = ReferenceResolution> + 'a {
    files
        .iter()
        .enumerate()
        .flat_map(move |(file_index, file)| {
            file.references
                .iter()
                .filter(move |reference| reference.name == target.symbol.name)
                .filter(move |reference| {
                    !file.symbols.iter().any(|candidate| {
                        candidate.name == target.symbol.name
                            && candidate.name_span == Some(reference.span)
                    })
                })
                .map(move |_| resolve_reference(target, file_index, file))
        })
}

fn resolve_reference(
    target: ResolutionTarget<'_>,
    reference_file_index: usize,
    reference_file: &StructureFile,
) -> ReferenceResolution {
    if reference_file_index == target.file_index {
        return ReferenceResolution::Referenced;
    }
    let bindings = import_bindings(reference_file, &target.symbol.name);
    if bindings
        .iter()
        .any(|binding| binding.resolved_path.as_deref() == Some(target.file.path.as_str()))
    {
        return ReferenceResolution::Referenced;
    }
    if !bindings.is_empty()
        && bindings
            .iter()
            .all(|binding| binding.resolved_path.is_some())
    {
        return ReferenceResolution::Unreferenced;
    }
    if bindings.is_empty() && target.candidate_count == 1 {
        return ReferenceResolution::Referenced;
    }
    ReferenceResolution::Unresolved
}

fn collect_function_metrics(
    symbols: &[StructureSymbol],
    facts: &auditor_ast::SemanticFileFacts,
) -> BTreeMap<u32, StructureFunctionMetrics> {
    let mut metrics = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|(index, _)| (index as u32, StructureFunctionMetrics::default()))
        .collect::<BTreeMap<_, _>>();
    for symbol in symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Variable)
    {
        if let Some(metric) =
            nearest_function_owner(symbols, symbol.parent).and_then(|owner| metrics.get_mut(&owner))
        {
            metric.locals += 1;
        }
    }
    for fact in facts.returns.iter() {
        if let Some(metric) =
            nearest_function_owner(symbols, fact.owner).and_then(|owner| metrics.get_mut(&owner))
        {
            metric.returns += 1;
        }
    }
    for fact in facts.statements.iter() {
        if let Some(metric) =
            nearest_function_owner(symbols, fact.owner).and_then(|owner| metrics.get_mut(&owner))
        {
            metric.statements += 1;
        }
    }
    metrics
}

/// The module of the nearest `go.mod` above a Go file, cached per directory.
fn go_module_for(
    file: &crate::AuditedFile,
    cache: &mut BTreeMap<std::path::PathBuf, Option<(String, usize)>>,
) -> Option<GoModule> {
    let directory = file.absolute.parent()?;
    let (path, levels) = match cache.get(directory) {
        Some(found) => found.clone()?,
        None => {
            let found = directory
                .ancestors()
                .enumerate()
                .find_map(|(levels, ancestor)| {
                    let manifest = std::fs::read_to_string(ancestor.join("go.mod")).ok()?;
                    let path = manifest.lines().find_map(|line| {
                        line.trim()
                            .strip_prefix("module ")
                            .map(|path| path.trim().trim_matches('"').to_string())
                    })?;
                    Some((path, levels))
                });
            cache.insert(directory.to_path_buf(), found.clone());
            found?
        }
    };
    let parents = file
        .relative
        .parent()
        .map(|parent| parent.components().collect::<Vec<_>>())
        .unwrap_or_default();
    let kept = parents.len().checked_sub(levels)?;
    let root = parents[..kept]
        .iter()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some(GoModule { path, root })
}

fn nearest_function_owner(symbols: &[StructureSymbol], mut owner: Option<u32>) -> Option<u32> {
    let mut visited = BTreeSet::new();
    while let Some(index) = owner {
        if !visited.insert(index) {
            return None;
        }
        let symbol = symbols.get(index as usize)?;
        if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
            return Some(index);
        }
        owner = symbol.parent;
    }
    None
}

/// Nesting of the control at `current` within its own function: a nested
/// function's depth does not include the loops around its definition.
fn control_depth(
    controls: &[auditor_ast::ControlRegionFact],
    symbols: &[StructureSymbol],
    mut current: Option<u32>,
) -> usize {
    let mut depth = 0;
    let mut visited = BTreeSet::new();
    let function = current
        .and_then(|index| controls.get(index as usize))
        .and_then(|control| nearest_function_owner(symbols, control.owner));
    while let Some(index) = current {
        if !visited.insert(index) {
            break;
        }
        let Some(control) = controls.get(index as usize) else {
            break;
        };
        if nearest_function_owner(symbols, control.owner) != function {
            break;
        }
        depth += usize::from(control.nesting_increment);
        current = control.parent;
    }
    depth
}

fn clean_name(text: &str) -> String {
    let compact = compact_text(text);
    if compact
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return compact;
    }
    util::identifiers(&compact)
        .into_iter()
        .next_back()
        .unwrap_or_default()
}

fn compact_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn bounded_declaration(source: &str) -> String {
    source.chars().take(512).collect()
}

fn bounded_expression(source: &str) -> String {
    compact_text(source).chars().take(256).collect()
}

fn declaration_is_public(language: &Language, source: &str) -> bool {
    let header = source
        .lines()
        .map(str::trim_start)
        .find(|line| !line.starts_with("#["))
        .unwrap_or_default();
    match language {
        Language::Rust => header.starts_with("pub ") || header.starts_with("pub("),
        Language::Java | Language::Kotlin | Language::CSharp => {
            header.split_whitespace().any(|word| word == "public")
        }
        Language::Go => header
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .find(|word| !word.is_empty() && !matches!(*word, "func" | "type" | "var" | "const"))
            .and_then(|word| word.chars().next())
            .is_some_and(char::is_uppercase),
        _ => false,
    }
}

fn class_base(language: &Language, source: &str) -> Option<String> {
    let header = class_header(source);
    let header = header.as_str();
    match language {
        Language::Python => header
            .split_once('(')
            .and_then(|(_, tail)| tail.split(')').next())
            .and_then(simple_type_name),
        // `extends` outside the type parameters: `class A<T extends X> extends B`.
        Language::JavaScript | Language::TypeScript | Language::Java => {
            let (index, _) =
                top_level(header).find(|(index, _)| header[*index..].starts_with(" extends "))?;
            simple_type_name(&header[index + " extends ".len()..])
        }
        // `class A(..) : B(), C` and `class A : B, IC`: the first supertype
        // after the colon that follows the primary constructor.
        Language::Kotlin | Language::CSharp => {
            let (index, _) = top_level(header).find(|(_, character)| *character == ':')?;
            simple_type_name(&header[index + 1..]).filter(|name| name != "where")
        }
        _ => None,
    }
}

/// A class declaration up to its body, joined onto one line, without the
/// attribute, annotation, and comment lines before it.
fn class_header(source: &str) -> String {
    let mut header = String::new();
    for line in source.lines().map(str::trim) {
        let declares = ["class ", "object ", "interface "]
            .iter()
            .any(|keyword| line.contains(keyword));
        let preamble = line.is_empty()
            || ["//", "/*", "*", "#["]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            || (line.starts_with('@') || line.starts_with('[')) && !declares;
        if header.is_empty() && preamble {
            continue;
        }
        let (head, body) = line
            .split_once('{')
            .map_or((line, false), |(head, _)| (head, true));
        header.push_str(head);
        header.push(' ');
        if body || line.ends_with(':') {
            break;
        }
    }
    header
}

/// `(index, character)` of `header` outside parentheses and angle brackets.
fn top_level(header: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut depth = 0usize;
    header.char_indices().filter(move |(_, character)| {
        match character {
            '(' | '<' => depth += 1,
            ')' | '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        depth == 0 && !matches!(character, ')' | '>')
    })
}

/// The simple name of the type that `text` starts with: the last segment of
/// a qualified name (`SqlMapper.ITypeHandler`, `models.Base`, `global::X`),
/// so a nested interface is not taken for its enclosing class.
fn simple_type_name(text: &str) -> Option<String> {
    let text = text.trim_start();
    let end = text
        .find(|character: char| {
            !(character.is_alphanumeric() || matches!(character, '_' | '.' | ':' | '$'))
        })
        .unwrap_or(text.len());
    text[..end]
        .rsplit(['.', ':'])
        .find(|segment| !segment.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[path = "structure_index_tests.rs"]
mod tests;

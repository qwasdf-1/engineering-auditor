use std::collections::{BTreeMap, BTreeSet, HashMap};

use auditor_ast::SymbolKind;
use auditor_core::{Confidence, Finding, Severity};

use crate::structure_index::{
    ImportForm, ReferenceResolution, StructureFile, StructureImport, StructureIndex,
    StructureSymbol,
};
use crate::Language;

const NESTING_DEPTH_THRESHOLD: usize = 5;
const CLASS_WMC_THRESHOLD: usize = 40;
const CLASS_CBO_THRESHOLD: usize = 8;
const CLASS_DIT_THRESHOLD: usize = 4;
const CLASS_NOC_THRESHOLD: usize = 10;

#[path = "structural_classes.rs"]
mod classes;
#[path = "structural_metrics.rs"]
mod metrics;
#[path = "structural_naming.rs"]
mod naming;
#[path = "structural_types.rs"]
mod types;

pub(crate) use classes::{class_structure_metrics_findings, orphan_symbol_findings};
pub(crate) use metrics::{
    excessive_nesting_depth_findings, god_module_findings, sibling_call_redundancy_findings,
    too_many_locals_findings, too_many_parameters_findings, too_many_returns_findings,
    too_many_statements_findings,
};
pub(crate) use naming::identifier_stutter_findings;
pub(crate) use types::{
    collection_parameter_mutation_findings, duplicate_import_findings,
    function_local_import_findings, out_parameter_findings, stringly_typed_parameter_findings,
};

/// The fixed text of a structural finding.
struct FindingText {
    detector: &'static str,
    title: &'static str,
    recommendation: &'static str,
}

/// A medium-severity, high-confidence structural finding at `line` of `file`.
fn structural_finding(
    text: &FindingText,
    message: String,
    file: &StructureFile,
    line: usize,
) -> Finding {
    auditor_core::finding!(
        text.detector,
        "structural",
        text.title,
        message,
        Severity::Medium,
        Confidence::High,
        &file.path,
        Some(line),
    )
    .with_recommendation(text.recommendation)
}

fn emittable_files(index: &StructureIndex) -> impl Iterator<Item = &StructureFile> {
    index.files().iter().filter(|file| file.emittable)
}

fn function_symbols(file: &StructureFile) -> impl Iterator<Item = (u32, &StructureSymbol)> {
    file.symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|(index, symbol)| (index as u32, symbol))
}

fn child_symbols(
    file: &StructureFile,
    owner: u32,
    kind: SymbolKind,
) -> impl Iterator<Item = &StructureSymbol> {
    file.symbols
        .iter()
        .filter(move |symbol| symbol.parent == Some(owner) && symbol.kind == kind)
}

fn declaration_has_string_type(declaration: &str) -> bool {
    let lower = declaration.to_ascii_lowercase();
    lower
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '&'
        })
        .any(|part| matches!(part, "str" | "&str" | "string"))
}

fn call_receiver_and_method(call: &crate::structure_index::StructureCall) -> Option<(&str, &str)> {
    if let Some(receiver) = call.receiver.as_deref() {
        let method = call
            .callee
            .rsplit(['.', ':'])
            .find(|part| !part.is_empty())?;
        return Some((receiver, method));
    }
    let (receiver, method) = call.callee.rsplit_once(['.', ':'])?;
    Some((receiver.trim_end_matches(':'), method))
}

/// A write the caller can observe: through a field or element of the
/// parameter (`p.field = x`, `p[i] = x`), or a C/C++ pointer (`*p`, `p->f`).
/// Rebinding the parameter itself (`p = x`) only changes a local: Java,
/// Python, JavaScript, Go, and Rust all pass the argument by value.
fn assignment_targets_parameter(language: &Language, target: &str, parameter: &str) -> bool {
    let compact = target.replace(' ', "");
    let pointer = matches!(language, Language::C | Language::Cpp);
    if pointer && compact == format!("*{parameter}") {
        return true;
    }
    compact.strip_prefix(parameter).is_some_and(|rest| {
        rest.starts_with('.') || rest.starts_with('[') || (pointer && rest.starts_with("->"))
    })
}

fn inheritance_depth(
    file_index: usize,
    class: &StructureSymbol,
    classes: &ClassIndex<'_>,
) -> usize {
    let mut depth = 0;
    let mut cursor = (file_index, class.base.as_deref());
    let mut seen = BTreeSet::new();
    while let (file, Some(base)) = cursor {
        if !seen.insert(base) {
            break;
        }
        depth += 1;
        let Some((base_file, _, base_class)) = classes.resolve(file, base) else {
            break;
        };
        cursor = (*base_file, base_class.base.as_deref());
    }
    depth
}

/// A repository's classes as (file, symbol, class), indexed by name.
pub(super) struct ClassIndex<'a> {
    pub(super) classes: Vec<(usize, usize, &'a StructureSymbol)>,
    by_name: HashMap<&'a str, Vec<usize>>,
}

impl<'a> ClassIndex<'a> {
    pub(super) fn of(index: &'a StructureIndex) -> Self {
        let classes = index
            .files()
            .iter()
            .enumerate()
            .flat_map(|(file_index, file)| {
                file.symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, symbol)| {
                        matches!(symbol.kind, SymbolKind::Class | SymbolKind::Object)
                    })
                    .map(move |(symbol_index, symbol)| (file_index, symbol_index, symbol))
            })
            .collect::<Vec<_>>();
        Self::new(classes)
    }

    /// NOC: each class's children, every base resolved once.
    pub(super) fn child_counts(&self) -> HashMap<(usize, usize), usize> {
        let mut children = HashMap::new();
        for (file, _, class) in &self.classes {
            let resolved = class
                .base
                .as_deref()
                .and_then(|base| self.resolve(*file, base));
            if let Some((base_file, base_symbol, _)) = resolved {
                *children.entry((*base_file, *base_symbol)).or_default() += 1;
            }
        }
        children
    }

    fn new(classes: Vec<(usize, usize, &'a StructureSymbol)>) -> Self {
        let mut by_name: HashMap<&'a str, Vec<usize>> = HashMap::new();
        for (position, (_, _, class)) in classes.iter().enumerate() {
            by_name
                .entry(class.name.as_str())
                .or_default()
                .push(position);
        }
        Self { classes, by_name }
    }

    pub(super) fn contains_name(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The class a base name in `file` refers to: one declared in the same
    /// file, else the only class of that name. A common name (`State`)
    /// declared in several files resolves to none of them.
    pub(super) fn resolve(
        &self,
        file: usize,
        name: &str,
    ) -> Option<&(usize, usize, &'a StructureSymbol)> {
        let named = self.by_name.get(name)?;
        let local = named
            .iter()
            .map(|position| &self.classes[*position])
            .find(|(candidate_file, _, _)| *candidate_file == file);
        local.or_else(|| match named.as_slice() {
            [only] => Some(&self.classes[*only]),
            _ => None,
        })
    }
}

fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Constant => "constant",
        SymbolKind::Static => "static",
        SymbolKind::TypeAlias => "type alias",
        SymbolKind::Union => "union",
        _ => "symbol",
    }
}

fn deduplicate(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    findings
}

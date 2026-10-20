use super::*;

/// Module-level kinds a caller names through the module.
const STUTTER_KINDS: &[SymbolKind] = &[
    SymbolKind::Function,
    SymbolKind::Class,
    SymbolKind::Object,
    SymbolKind::Struct,
    SymbolKind::Union,
    SymbolKind::Interface,
    SymbolKind::Trait,
    SymbolKind::Enum,
    SymbolKind::Constant,
    SymbolKind::Static,
    SymbolKind::TypeAlias,
];

/// Public items whose name repeats the module callers qualify them with:
/// `chubby.ChubbyFile` (Go Code Review Comments) or `config::ConfigError`
/// (clippy `module_name_repetitions`). A name equal to its module
/// (`error::Error`) is idiomatic, and locals are out of scope. Only modules
/// callers reach by name count: every Go package, and a Rust module declared
/// `pub`; a private Rust module's items are re-exported under another path.
pub(crate) fn identifier_stutter_findings(index: &StructureIndex) -> Vec<Finding> {
    let files = index
        .files()
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    let mut findings = Vec::new();
    for file in emittable_files(index).filter(|file| match file.language {
        Language::Go => true,
        Language::Rust => rust_module_is_public(&files, file),
        _ => false,
    }) {
        for symbol in &file.symbols {
            // A Python file's root `module` node is an unnamed module symbol.
            let module = match symbol
                .parent
                .and_then(|parent| file.symbols.get(parent as usize))
            {
                Some(parent) if parent.kind != SymbolKind::Module => None,
                Some(parent) if !parent.name.is_empty() => Some(parent.name.as_str()),
                _ => file.module.as_deref(),
            };
            let Some(module) = module.filter(|_| {
                STUTTER_KINDS.contains(&symbol.kind)
                    && !symbol.associated
                    && symbol.is_public
                    && !symbol.visibility_restricted
            }) else {
                continue;
            };
            let Some(repeated) = repeated_module_words(&symbol.name, module) else {
                continue;
            };
            let qualified = match file.language {
                Language::Rust => format!("{module}::{}", symbol.name),
                _ => format!("{module}.{}", symbol.name),
            };
            findings.push(
                stutter_finding(file, symbol, &qualified)
                    .with_identity("identifier", symbol.name.clone())
                    .with_evidence("identifier", symbol.name.clone())
                    .with_evidence("module", module.to_string())
                    .with_evidence("overlap_count", repeated),
            );
        }
    }
    findings
}

/// Whether callers outside the crate can name the file's module: it is
/// declared `pub mod` (not `pub(crate)`) in its parent module file
/// (`lib.rs`, `main.rs`, `mod.rs`, or the sibling `<parent>.rs`), and so is
/// every module above it.
fn rust_module_is_public(files: &BTreeMap<&str, &StructureFile>, file: &StructureFile) -> bool {
    let Some(module) = file.module.as_deref() else {
        // The crate root.
        return true;
    };
    let path = std::path::Path::new(&file.path);
    let directory = if path.file_stem().is_some_and(|stem| stem == "mod") {
        path.parent().and_then(std::path::Path::parent)
    } else {
        path.parent()
    };
    let Some(directory) = directory else {
        return false;
    };
    let mut parents = ["mod.rs", "lib.rs", "main.rs"]
        .map(|name| crate::util::path_slash(&directory.join(name)))
        .to_vec();
    parents.push(format!("{}.rs", crate::util::path_slash(directory)));
    parents
        .iter()
        .filter_map(|parent| files.get(parent.as_str()))
        .find(|parent| {
            parent.symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Module
                    && symbol.parent.is_none()
                    && symbol.name == module
                    && symbol.is_public
                    && !symbol.visibility_restricted
            })
        })
        .is_some_and(|parent| rust_module_is_public(files, parent))
}

/// How many module words the name starts or ends with, when it has more.
fn repeated_module_words(name: &str, module: &str) -> Option<usize> {
    let lower = |text: &str| {
        crate::util::split_identifier(text)
            .into_iter()
            .map(|word| word.to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    let (words, module_words) = (lower(name), lower(module));
    (!module_words.is_empty()
        && words.len() > module_words.len()
        && (words.starts_with(&module_words) || words.ends_with(&module_words)))
    .then_some(module_words.len())
}

fn stutter_finding(file: &StructureFile, symbol: &StructureSymbol, qualified: &str) -> Finding {
    auditor_core::finding!(
        "identifier_stutter",
        "lexical",
        "Public name repeats its module name",
        format!(
            "`{}` repeats its module name, so callers write `{qualified}`.",
            symbol.name
        ),
        Severity::Medium,
        Confidence::High,
        &file.path,
        Some(symbol.line),
    )
    .with_recommendation(
        "Drop the module words from the name; callers already qualify it with the module.",
    )
}

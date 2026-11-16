use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::Language;

use super::super::{GoModule, StructureFile, StructureImport};
use super::rust::RustModules;

pub(crate) fn resolve_imports(files: &mut [StructureFile]) {
    let known = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let modules = RustModules::new(files, &known);
    let rust = files
        .iter()
        .map(|file| {
            (file.language == Language::Rust).then(|| {
                file.imports
                    .iter()
                    .map(|import| {
                        let canonical = import.canonical.as_deref()?;
                        modules.resolve(&file.path, canonical, import.inline_modules)
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    for (file, rust) in files.iter_mut().zip(rust) {
        if let Some(resolved) = rust {
            for (import, resolved) in file.imports.iter_mut().zip(resolved) {
                import.resolved_path = resolved;
            }
            continue;
        }
        let go_module = file.go_module.clone();
        let is_java = file.language == Language::Java;
        for import in &mut file.imports {
            if is_java {
                import.resolved_path = import
                    .canonical
                    .as_deref()
                    .and_then(|canonical| resolve_java_import(canonical, &known));
                continue;
            }
            import.resolved_path = import
                .module
                .as_deref()
                .and_then(|module| match &go_module {
                    Some(go_module) => resolve_go_package(go_module, module, &known),
                    None => resolve_module(&file.path, &file.language, module, &known),
                });
        }
    }
    super::uses::resolve_used_paths(files);
}

/// A Go import under the file's own module resolves to its package
/// directory, represented by the first non-test Go file in it.
fn resolve_go_package(
    go_module: &GoModule,
    module: &str,
    known: &BTreeSet<String>,
) -> Option<String> {
    let suffix = module
        .strip_prefix(&go_module.path)
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))?
        .trim_start_matches('/');
    let directory = [go_module.root.as_str(), suffix]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    known
        .range(prefix.clone()..)
        .take_while(|path| path.starts_with(&prefix))
        .find(|path| {
            let name = &path[prefix.len()..];
            !name.contains('/') && name.ends_with(".go") && !name.ends_with("_test.go")
        })
        .cloned()
}

/// A Java import resolves to the source file its package path names under any
/// source root: `com.example.Model` to `../com/example/Model.java`, a static
/// import to its class, and `com.example.*` to the package's first file.
fn resolve_java_import(canonical: &str, known: &BTreeSet<String>) -> Option<String> {
    let ends_with =
        |path: &str, suffix: &str| path == suffix || path.ends_with(&format!("/{suffix}"));
    if let Some(package) = canonical.strip_suffix(".*") {
        let directory = format!("{}/", package.replace('.', "/"));
        return known
            .iter()
            .find(|path| {
                path.ends_with(".java")
                    && path
                        .rsplit_once('/')
                        .is_some_and(|(parent, _)| ends_with(&format!("{parent}/"), &directory))
            })
            .cloned();
    }
    let mut segments = canonical.split('.').collect::<Vec<_>>();
    while segments.len() > 1 {
        let suffix = format!("{}.java", segments.join("/"));
        if let Some(path) = known.iter().find(|path| ends_with(path, &suffix)) {
            return Some(path.clone());
        }
        segments.pop();
    }
    None
}

fn resolve_module(
    current: &str,
    language: &Language,
    module: &str,
    known: &BTreeSet<String>,
) -> Option<String> {
    let mut candidates = Vec::new();
    match language {
        Language::Python => python_module_candidates(current, module, &mut candidates),
        Language::JavaScript | Language::TypeScript => {
            relative_module_candidates(current, module, &mut candidates)
        }
        _ => {}
    }
    candidates
        .into_iter()
        .map(|candidate| normalize_path(&candidate))
        .find(|candidate| known.contains(candidate))
}

/// A Python module: absolute from the scan root, or relative with one
/// leading dot for the file's own package and each further dot one package
/// up (`..logger` in `pkg/utils/disk.py` is `pkg/logger`). A package resolves
/// to its `__init__.py`.
fn python_module_candidates(current: &str, module: &str, candidates: &mut Vec<PathBuf>) {
    let name = module.trim_start_matches('.');
    let mut base = PathBuf::new();
    let levels = module.len() - name.len();
    if levels > 0 {
        let mut package = Path::new(current).parent();
        for _ in 1..levels {
            package = package.and_then(Path::parent);
        }
        let Some(package) = package else {
            return;
        };
        base = package.to_path_buf();
    }
    for segment in name.split('.').filter(|segment| !segment.is_empty()) {
        base.push(segment);
    }
    candidates.push(base.join("__init__.py"));
    push_source_candidates(base, &["py"], candidates);
}

fn relative_module_candidates(current: &str, module: &str, candidates: &mut Vec<PathBuf>) {
    if !module.starts_with('.') {
        return;
    }
    let base = Path::new(current)
        .parent()
        .unwrap_or(Path::new(""))
        .join(module);
    push_source_candidates(base, &["js", "jsx", "ts", "tsx"], candidates);
}

fn push_source_candidates(base: PathBuf, extensions: &[&str], candidates: &mut Vec<PathBuf>) {
    for extension in extensions {
        candidates.push(base.with_extension(extension));
        candidates.push(base.join(format!("mod.{extension}")));
        candidates.push(base.join(format!("index.{extension}")));
    }
}

pub(crate) fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

pub(crate) fn import_bindings<'a>(file: &'a StructureFile, name: &str) -> Vec<&'a StructureImport> {
    file.imports
        .iter()
        .filter(|import| {
            import.imported.as_deref() == Some(name)
                || import.alias.as_deref() == Some(name)
                || import.canonical.as_deref().is_some_and(|canonical| {
                    canonical
                        .rsplit([':', '.', '/'])
                        .find(|part| !part.is_empty())
                        == Some(name)
                })
        })
        .collect()
}

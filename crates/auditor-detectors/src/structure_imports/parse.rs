use crate::Language;

use super::super::{compact_text, ImportForm, StructureImport};

pub(crate) fn parse_import(language: &Language, raw: &str) -> StructureImport {
    let normalized = compact_text(raw);
    let form = match language {
        Language::JavaScript | Language::TypeScript => javascript_import_form(&normalized),
        _ => ImportForm::Value,
    };
    let parts = match language {
        Language::Rust => rust_import_parts(&normalized),
        Language::Python => python_import_parts(&normalized),
        Language::JavaScript | Language::TypeScript => javascript_import_parts(&normalized),
        Language::Go | Language::C | Language::Cpp => quoted_import_parts(&normalized),
        Language::Java | Language::Kotlin | Language::CSharp => dotted_import_parts(&normalized),
        _ => ImportParts {
            canonical: Some(normalized),
            ..ImportParts::default()
        },
    };
    StructureImport {
        canonical: parts.canonical,
        imported: parts.imported,
        alias: parts.alias,
        resolved_path: None,
        owner: None,
        line: 0,
        dynamic: false,
        justified: false,
        reexport: false,
        module: parts.module,
        form,
        branches: Vec::new(),
    }
}

fn javascript_import_form(normalized: &str) -> ImportForm {
    let type_only =
        normalized.starts_with("import type ") || normalized.starts_with("export type ");
    let namespace = normalized.contains("* as ");
    match (type_only, namespace) {
        (true, true) => ImportForm::TypeNamespace,
        (true, false) => ImportForm::Type,
        (false, true) => ImportForm::Namespace,
        (false, false) => ImportForm::Value,
    }
}

#[derive(Default)]
struct ImportParts {
    canonical: Option<String>,
    imported: Option<String>,
    alias: Option<String>,
    module: Option<String>,
}

fn rust_import_parts(normalized: &str) -> ImportParts {
    let body = normalized
        .strip_prefix("pub use ")
        .or_else(|| normalized.strip_prefix("use "))
        .unwrap_or(normalized)
        .trim_end_matches(';');
    let (path, alias) = split_alias(body);
    let segments = path.split("::").collect::<Vec<_>>();
    let (module, imported) = if !path.contains('{') && !path.ends_with('*') {
        (
            (segments.len() > 1).then(|| segments[..segments.len() - 1].join("::")),
            segments.last().map(|segment| segment.trim().to_string()),
        )
    } else {
        (path.split("::{").next().map(str::to_string), None)
    };
    ImportParts {
        canonical: Some(path.to_string()),
        imported,
        alias,
        module,
    }
}

fn python_import_parts(normalized: &str) -> ImportParts {
    if let Some(body) = normalized.strip_prefix("from ") {
        let (module, names) = body.split_once(" import ").unwrap_or((body, ""));
        let (name, alias) = split_alias(names.split(',').next().unwrap_or_default().trim());
        return ImportParts {
            canonical: Some(format!("{module}::{name}")),
            imported: (!name.is_empty()).then(|| name.to_string()),
            alias,
            module: Some(module.to_string()),
        };
    }
    let body = normalized.strip_prefix("import ").unwrap_or(normalized);
    let (name, alias) = split_alias(body.split(',').next().unwrap_or_default().trim());
    ImportParts {
        canonical: (!name.is_empty()).then(|| name.to_string()),
        imported: name.rsplit('.').next().map(str::to_string),
        alias,
        module: (!name.is_empty()).then(|| name.to_string()),
    }
}

fn javascript_import_parts(normalized: &str) -> ImportParts {
    let module = first_quoted(normalized);
    let binding = normalized
        .split_once(" from ")
        .and_then(|(bindings, _)| brace_binding(bindings));
    ImportParts {
        canonical: module.clone(),
        imported: binding.as_ref().map(|(name, _)| name.clone()),
        alias: binding.and_then(|(_, alias)| alias),
        module,
    }
}

fn quoted_import_parts(normalized: &str) -> ImportParts {
    let module = first_quoted(normalized).or_else(|| first_angle_path(normalized));
    ImportParts {
        canonical: module.clone(),
        module,
        ..ImportParts::default()
    }
}

fn dotted_import_parts(normalized: &str) -> ImportParts {
    let body = normalized
        .trim_start_matches("import ")
        .trim_start_matches("using ")
        .trim_start_matches("static ")
        .trim_end_matches(';');
    let (path, alias) = split_alias(body);
    ImportParts {
        canonical: (!path.is_empty()).then(|| path.to_string()),
        imported: path
            .rsplit(['.', ':'])
            .find(|part| !part.is_empty())
            .map(str::to_string),
        alias,
        module: path.rsplit_once('.').map(|(module, _)| module.to_string()),
    }
}

fn split_alias(value: &str) -> (&str, Option<String>) {
    value
        .rsplit_once(" as ")
        .map_or((value, None), |(name, alias)| {
            (name, Some(alias.to_string()))
        })
}

fn brace_binding(text: &str) -> Option<(String, Option<String>)> {
    let body = text.split_once('{')?.1.split_once('}')?.0;
    let first = body.split(',').next()?.trim();
    let (name, alias) = first
        .split_once(" as ")
        .map_or((first, None), |(name, alias)| {
            (name, Some(alias.to_string()))
        });
    Some((name.to_string(), alias))
}

pub(crate) fn first_quoted(text: &str) -> Option<String> {
    for quote in ['\'', '"', '`'] {
        let Some(start) = text.find(quote) else {
            continue;
        };
        let tail = &text[start + quote.len_utf8()..];
        if let Some(end) = tail.find(quote) {
            return Some(tail[..end].to_string());
        }
    }
    None
}

fn first_angle_path(text: &str) -> Option<String> {
    let (_, tail) = text.split_once('<')?;
    let (path, _) = tail.split_once('>')?;
    Some(path.to_string())
}

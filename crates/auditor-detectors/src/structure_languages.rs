//! Per-language structure facts resolved from the syntax tree: call
//! identities (Java, C#, JavaScript/TypeScript), the expression that consumes a
//! JS/TS call, and JSDoc parameter types for JavaScript.

use std::collections::BTreeMap;

use auditor_ast::SymbolKind;

use super::StructureSymbol;
use crate::Language;

/// Resolved identities of the Java, C#, and JS/TS calls in a file, by span.
pub(super) fn call_apis(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> BTreeMap<(usize, usize), String> {
    let root = tree.root_node();
    let calls = match file.language {
        Language::Java => crate::java_api_identity::JavaScope::new(root).calls(root),
        Language::CSharp => crate::csharp_api_identity::CSharpScope::new(root).calls(root),
        Language::JavaScript | Language::TypeScript => {
            crate::js_api_identity::JsScope::new(root).calls(root)
        }
        _ => return BTreeMap::new(),
    };
    calls
        .into_iter()
        .map(|(api, call)| ((call.start_byte(), call.end_byte()), api))
        .collect()
}

/// For each JS/TS call, by span, the expression that consumes its value
/// through member or index access: `xs.filter(p).length > 0` for
/// `xs.filter(p)`, or `xs.filter(p)[0]`.
pub(super) fn call_consumers(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> BTreeMap<(usize, usize), String> {
    if !matches!(file.language, Language::JavaScript | Language::TypeScript) {
        return BTreeMap::new();
    }
    crate::ast_scan::nodes_of_kind(tree.root_node(), &["call_expression"])
        .into_iter()
        .filter_map(|call| {
            let mut consumer = call;
            while let Some(parent) = consumer.parent().filter(|parent| {
                matches!(parent.kind(), "member_expression" | "subscript_expression")
                    && parent
                        .child_by_field_name("object")
                        .is_some_and(|object| object.start_byte() == consumer.start_byte())
            }) {
                consumer = parent;
            }
            if consumer.start_byte() == call.start_byte() && consumer.end_byte() == call.end_byte()
            {
                return None;
            }
            let consumer = consumer
                .parent()
                .filter(|parent| matches!(parent.kind(), "binary_expression" | "unary_expression"))
                .unwrap_or(consumer);
            Some((
                (call.start_byte(), call.end_byte()),
                consumer
                    .text()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect(),
            ))
        })
        .collect()
}

/// JavaScript parameters typed by the JSDoc block before their function
/// (`@param {string[]} names`) get that type as their declaration
/// (`names: string[]`), the form TypeScript annotations take.
pub(super) fn apply_jsdoc_types(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    symbols: &mut [StructureSymbol],
) {
    if file.language != Language::JavaScript {
        return;
    }
    let source = tree.source();
    let mut types = BTreeMap::new();
    for (index, function) in symbols.iter().enumerate() {
        if !matches!(function.kind, SymbolKind::Function | SymbolKind::Method) {
            continue;
        }
        let Some(block) = preceding_jsdoc(&source[..function.span.start_byte]) else {
            continue;
        };
        for (name, kind) in jsdoc_parameters(block) {
            types.insert((index as u32, name), kind);
        }
    }
    for symbol in symbols.iter_mut() {
        if symbol.kind != SymbolKind::Parameter {
            continue;
        }
        if let Some(kind) = symbol
            .parent
            .and_then(|parent| types.get(&(parent, symbol.name.clone())))
        {
            symbol.declaration = Some(format!("{}: {kind}", symbol.name));
        }
    }
}

/// The `/** .. */` block that ends just before a declaration, allowing the
/// `export`, `default`, and `async` keywords in between.
fn preceding_jsdoc(before: &str) -> Option<&str> {
    let mut rest = before.trim_end();
    while let Some(stripped) = ["export", "default", "async"]
        .iter()
        .find_map(|keyword| rest.strip_suffix(keyword))
    {
        rest = stripped.trim_end();
    }
    let body = rest.strip_suffix("*/")?;
    let start = body.rfind("/**")?;
    Some(&body[start + 3..])
}

/// `(name, type)` of each `@param {type} name` (or `[name]`, `[name=..]`).
fn jsdoc_parameters(block: &str) -> Vec<(String, String)> {
    block
        .split("@param")
        .skip(1)
        .filter_map(|tag| {
            let tag = tag.trim_start();
            let kind = tag.strip_prefix('{')?;
            let mut depth = 1usize;
            let end = kind.char_indices().find_map(|(index, character)| {
                match character {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(index)
            })?;
            let name = kind[end + 1..]
                .split_whitespace()
                .next()?
                .trim_start_matches('[')
                .split(['=', ']'])
                .next()?;
            (!name.is_empty()).then(|| (name.to_string(), kind[..end].trim().to_string()))
        })
        .collect()
}

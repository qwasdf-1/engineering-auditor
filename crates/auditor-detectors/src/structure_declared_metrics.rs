//! Function metrics for grammars whose semantic facts do not normalize them:
//! Kotlin (locals are `val`/`var` properties and loop variables, a return is
//! a `return` jump, and statements are the entries of `statements` blocks)
//! and C/C++ (locals are the declarators of `declaration`s). Nested
//! functions and lambdas count toward their own owners, not the enclosing
//! function.

use std::collections::BTreeMap;

use auditor_ast::{AstNode, SymbolKind};

use super::{StructureFunctionMetrics, StructureSymbol};
use crate::Language;

const NESTED: &[&str] = &[
    "function_declaration",
    "function_definition",
    "lambda_literal",
    "anonymous_function",
    "lambda_expression",
];

pub(super) fn count_declared_metrics(
    language: &Language,
    root: AstNode<'_>,
    symbols: &[StructureSymbol],
    metrics: &mut BTreeMap<u32, StructureFunctionMetrics>,
) {
    if !matches!(language, Language::Kotlin | Language::C | Language::Cpp) {
        return;
    }
    let functions =
        crate::ast_scan::nodes_of_kind(root, &["function_declaration", "function_definition"])
            .into_iter()
            .map(|function| (function.start_byte(), function))
            .collect::<BTreeMap<_, _>>();
    for (index, symbol) in symbols.iter().enumerate() {
        if !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
            continue;
        }
        let (Some(function), Some(metric)) = (
            functions.get(&symbol.span.start_byte),
            metrics.get_mut(&(index as u32)),
        ) else {
            continue;
        };
        let mut counts = Counts::default();
        for child in function.named_children() {
            walk(language, child, &mut counts);
        }
        metric.locals = counts.locals;
        if *language == Language::Kotlin {
            metric.returns = counts.returns;
            metric.statements = counts.statements;
        }
    }
}

#[derive(Default)]
struct Counts {
    locals: usize,
    returns: usize,
    statements: usize,
}

fn walk(language: &Language, node: AstNode<'_>, counts: &mut Counts) {
    if NESTED.contains(&node.kind()) {
        return;
    }
    match (language, node.kind()) {
        (Language::Kotlin, "property_declaration") => counts.locals += 1,
        (Language::Kotlin, "for_statement") => {
            counts.locals += node
                .named_children()
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "variable_declaration" | "multi_variable_declaration"
                    )
                })
                .count();
        }
        (Language::Kotlin, "jump_expression") if node.text().starts_with("return") => {
            counts.returns += 1;
        }
        // Comments are named children of a block, not statements.
        (Language::Kotlin, "statements") => {
            counts.statements += node
                .named_children()
                .filter(|child| !matches!(child.kind(), "line_comment" | "multiline_comment"))
                .count();
        }
        (Language::C | Language::Cpp, "declaration") => {
            counts.locals += node
                .named_children()
                .filter(|child| {
                    child.kind().ends_with("declarator") || child.kind() == "identifier"
                })
                .count();
        }
        _ => {}
    }
    for child in node.named_children() {
        walk(language, child, counts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(
        language: Language,
        ast: auditor_ast::AstLang,
        source: &str,
    ) -> Vec<(usize, usize, usize)> {
        let tree = auditor_ast::AstParser::new()
            .unwrap()
            .parse(&ast, source)
            .unwrap();
        let root = tree.root_node();
        let functions =
            crate::ast_scan::nodes_of_kind(root, &["function_declaration", "function_definition"]);
        let symbols = functions
            .iter()
            .map(|function| StructureSymbol {
                kind: SymbolKind::Function,
                name: String::new(),
                line: 1,
                span: auditor_ast::SourceSpan::from_node(*function),
                name_span: None,
                parent: None,
                declaration: None,
                base: None,
                is_async: false,
                is_public: false,
                visibility_restricted: false,
                in_test: false,
                implicitly_used: false,
                overload_stub: false,
                associated: false,
            })
            .collect::<Vec<_>>();
        let mut metrics = (0..symbols.len() as u32)
            .map(|index| (index, StructureFunctionMetrics::default()))
            .collect::<BTreeMap<_, _>>();
        count_declared_metrics(&language, root, &symbols, &mut metrics);
        metrics
            .values()
            .map(|metric| (metric.locals, metric.returns, metric.statements))
            .collect()
    }

    #[test]
    fn kotlin_and_c_functions_count_their_own_declarations() {
        let kotlin = "fun f(xs: List<Int>): Int {\n    // a comment\n    val a = 1\n    /* another */\n    var b = 2\n    for (x in xs) { b += x }\n    val g = { y: Int -> val inner = y; inner }\n    if (a > b) return a\n    return b\n}\n";
        assert_eq!(
            metrics(Language::Kotlin, auditor_ast::AstLang::Kotlin, kotlin),
            [(4, 2, 7)]
        );
        let c = "int f(int n) {\n    int a = 1, b = 2;\n    char *p;\n    for (int i = 0; i < n; i++) { a += i; }\n    return a + b;\n}\n";
        assert_eq!(
            metrics(Language::C, auditor_ast::AstLang::C, c),
            [(4, 0, 0)]
        );
    }
}

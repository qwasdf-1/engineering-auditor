use super::*;

pub(super) fn build_structure_file(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> StructureFile {
    let facts = tree.semantic_facts();
    let mut symbols = collect_symbols(file, tree, &facts);
    super::languages::apply_jsdoc_types(file, tree, &mut symbols);
    let assignments = collect_assignments(tree, &facts, &symbols);
    let loops = collect_loops(tree, &facts, &symbols);
    let mut function_metrics = collect_function_metrics(&symbols, &facts);
    if file.language == Language::Python {
        super::operands::count_python_locals(&assignments, &loops, &mut function_metrics);
    }
    super::declared_metrics::count_declared_metrics(
        &file.language,
        tree.root_node(),
        &symbols,
        &mut function_metrics,
    );
    StructureFile {
        path: util::path_slash(&file.relative),
        language: file.language.clone(),
        emittable: matches!(file.role, SourceRole::Production | SourceRole::Tooling)
            && !util::is_test_source_path(&file.relative),
        module: module_name(file, tree),
        function_metrics,
        references: collect_references(tree, &facts, &symbols),
        imports: collect_imports(file, tree, &facts, &symbols),
        calls: collect_calls(file, tree, &facts, &symbols),
        assignments,
        literals: collect_literals(tree, &facts, &symbols),
        controls: collect_controls(file, tree, &facts, &symbols),
        loops,
        subscripts: super::operands::collect_subscripts(file, tree, &symbols),
        memberships: super::operands::collect_memberships(file, tree, &symbols),
        go_module: None,
        async_spans: java_async_spans(file, tree),
        symbols,
    }
}

fn collect_literals(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureLiteral> {
    facts
        .literals
        .iter()
        .map(|literal| StructureLiteral {
            text: tree.text_for_span(literal.span).trim().to_string(),
            owner: nearest_function_owner(symbols, literal.owner),
            control_region: literal.control_region,
        })
        .collect()
}

fn collect_symbols(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
) -> Vec<StructureSymbol> {
    let test_spans = rust_test_spans(file, tree);
    let implicitly_used_spans = rust_implicitly_used_spans(file, tree);
    let overload_spans = python_overload_spans(file, tree);
    let associated_spans = rust_associated_spans(file, tree);
    facts
        .symbols
        .iter()
        .map(|symbol| {
            let mut name = symbol
                .name
                .map(|span| clean_name(tree.text_for_span(span)))
                .unwrap_or_default();
            let source = tree.text_for_span(symbol.span);
            if name.is_empty() && symbol.kind == SymbolKind::Variable {
                name = declaration_binding(source).unwrap_or_default();
            }
            StructureSymbol {
                kind: symbol.kind,
                name,
                line: symbol.span.start_line,
                span: symbol.span,
                name_span: symbol.name,
                parent: symbol.parent,
                declaration: match symbol.kind {
                    SymbolKind::Parameter => Some(bounded_declaration(source)),
                    // A Java local's declared type (`List<String> names`).
                    SymbolKind::Variable if file.language == Language::Java => {
                        source.split('=').next().map(bounded_declaration)
                    }
                    // A Kotlin function's annotations and modifiers, through
                    // `fun` (visibility, `external`).
                    SymbolKind::Function | SymbolKind::Method
                        if file.language == Language::Kotlin =>
                    {
                        crate::util::identifiers(source)
                            .iter()
                            .position(|word| word == "fun")
                            .and_then(|_| source.find("fun"))
                            .map(|end| source[..end + "fun".len()].trim().to_string())
                    }
                    // A Java method's annotations, modifiers, and return type
                    // (`@Override public int read`).
                    SymbolKind::Function | SymbolKind::Method
                        if file.language == Language::Java =>
                    {
                        source.split('(').next().map(|head| head.trim().to_string())
                    }
                    // A JavaScript or TypeScript function's leading text, from
                    // the start of its line (`export const load = (`,
                    // `private #push(`), for its visibility.
                    SymbolKind::Function | SymbolKind::Method
                        if matches!(file.language, Language::JavaScript | Language::TypeScript) =>
                    {
                        let line_start = tree.source()[..symbol.span.start_byte]
                            .rfind('\n')
                            .map_or(0, |index| index + 1);
                        let head = source.lines().next().unwrap_or_default();
                        Some(format!(
                            "{}{head}",
                            &tree.source()[line_start..symbol.span.start_byte]
                        ))
                    }
                    // A class header (`abstract class Base<T> extends Node`).
                    SymbolKind::Class => source
                        .split(['{', '\n'])
                        .next()
                        .map(|header| header.trim().to_string()),
                    _ => None,
                },
                base: class_base(&file.language, source),
                is_async: declaration_is_async(&file.language, source),
                is_public: declaration_is_public(&file.language, source),
                visibility_restricted: file.language == crate::Language::Rust
                    && source
                        .lines()
                        .map(str::trim_start)
                        .find(|line| !line.starts_with("#["))
                        .is_some_and(|header| header.starts_with("pub(")),
                in_test: test_spans.iter().any(|span| {
                    span.start_byte <= symbol.span.start_byte
                        && symbol.span.end_byte <= span.end_byte
                }),
                implicitly_used: implicitly_used_spans.iter().any(|span| {
                    span.start_byte <= symbol.span.start_byte
                        && symbol.span.end_byte <= span.end_byte
                }),
                overload_stub: overload_spans
                    .iter()
                    .any(|span| span.start_byte == symbol.span.start_byte),
                associated: associated_spans
                    .iter()
                    .any(|span| span.start_byte == symbol.span.start_byte),
            }
        })
        .collect()
}

fn declaration_binding(source: &str) -> Option<String> {
    let identifiers = util::identifiers(source);
    let mut identifiers = identifiers.iter().map(String::as_str);
    match identifiers.next()? {
        "let" => identifiers
            .find(|identifier| *identifier != "mut")
            .map(str::to_string),
        "const" | "var" => identifiers.next().map(str::to_string),
        first => Some(first.to_string()),
    }
}

fn collect_references(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureReference> {
    facts
        .references
        .iter()
        .filter_map(|reference| {
            let name = clean_name(tree.text_for_span(reference.span));
            (!name.is_empty()).then_some(StructureReference {
                kind: reference.kind,
                name,
                span: reference.span,
                owner: nearest_function_owner(symbols, reference.owner),
                control_region: reference.control_region,
            })
        })
        .collect()
}

fn collect_imports(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureImport> {
    let guarded = python_guarded_import_spans(file, tree);
    let type_checking = python_type_checking_spans(file, tree);
    if file.language == Language::Go {
        return go_imports(tree);
    }
    let mut imports = facts
        .imports
        .iter()
        .map(|import| {
            let mut parsed = parse_import(&file.language, tree.text_for_span(import.span));
            parsed.owner = nearest_function_owner(symbols, import.owner);
            parsed.line = import.span.start_line;
            parsed.justified = guarded.iter().any(|span| {
                span.start_byte <= import.span.start_byte && import.span.end_byte <= span.end_byte
            }) || import_has_documented_reason(tree, parsed.line);
            parsed.reexport = import_is_reexport(&file.language, tree.text_for_span(import.span));
            if type_checking.iter().any(|span| {
                span.start_byte <= import.span.start_byte && import.span.end_byte <= span.end_byte
            }) {
                parsed.form = crate::structure_index::ImportForm::Type;
            }
            if matches!(file.language, Language::C | Language::Cpp) {
                parsed.branches = preprocessor_branches(tree, import.span);
            }
            parsed
        })
        .collect::<Vec<_>>();
    imports.extend(dynamic_imports(tree, facts, symbols));
    imports.sort_by_key(|import| {
        (
            import.line,
            import.owner,
            import.canonical.clone(),
            import.dynamic,
        )
    });
    imports.dedup_by(|left, right| {
        left.line == right.line
            && left.owner == right.owner
            && left.canonical == right.canonical
            && left.dynamic == right.dynamic
    });
    imports
}

/// The preprocessor branches enclosing `span`, outermost first: each
/// `#if`/`#ifdef`/`#elif` whose own lines hold the span, and each `#elif` or
/// `#else` alternative that does. A conditional whose alternative holds the
/// span is represented by that alternative alone.
fn preprocessor_branches(tree: &auditor_ast::AstTree, span: SourceSpan) -> Vec<usize> {
    const CONDITIONALS: &[&str] = &[
        "preproc_if",
        "preproc_ifdef",
        "preproc_elif",
        "preproc_elifdef",
        "preproc_else",
    ];
    let contains = |node: auditor_ast::AstNode<'_>| {
        node.start_byte() <= span.start_byte && span.end_byte <= node.end_byte()
    };
    let mut branches = crate::ast_scan::nodes_of_kind(tree.root_node(), CONDITIONALS)
        .into_iter()
        .filter(|node| contains(*node))
        .filter(|node| {
            node.child_by_field_name("alternative")
                .is_none_or(|alternative| !contains(alternative))
        })
        .map(|node| node.start_byte())
        .collect::<Vec<_>>();
    branches.sort_unstable();
    branches
}

/// One import per Go `import_spec`: a grouped `import ( .. )` declaration
/// binds each path separately, under its explicit name when it has one.
fn go_imports(tree: &auditor_ast::AstTree) -> Vec<StructureImport> {
    crate::ast_scan::nodes_of_kind(tree.root_node(), &["import_spec"])
        .into_iter()
        .filter_map(|spec| {
            let path = crate::api_identity::go_string(spec.child_by_field_name("path")?)?;
            let mut import = parse_import(&Language::Go, &format!("\"{path}\""));
            import.alias = spec
                .child_by_field_name("name")
                .map(|name| name.text().to_string());
            import.line = spec.start_position().0 + 1;
            Some(import)
        })
        .collect()
}

fn dynamic_imports(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureImport> {
    facts
        .calls
        .iter()
        .filter_map(|call| {
            let callee = compact_text(tree.text_for_span(call.callee));
            // Kotlin's `require(..)` is a precondition, not an import.
            let dynamic = match tree.root_node().language() {
                auditor_ast::AstLang::JavaScript | auditor_ast::AstLang::TypeScript => {
                    matches!(callee.as_str(), "import" | "require")
                }
                auditor_ast::AstLang::Python => callee == "__import__",
                _ => false,
            };
            dynamic.then(|| {
                let module = first_quoted(tree.text_for_span(call.span));
                let owner = nearest_function_owner(symbols, call.owner);
                StructureImport {
                    canonical: module.clone(),
                    imported: None,
                    alias: None,
                    resolved_path: None,
                    owner,
                    line: call.span.start_line,
                    dynamic: module.is_none(),
                    justified: import_has_documented_reason(tree, call.span.start_line),
                    reexport: false,
                    module,
                    form: if tree.source()[call.span.end_byte..].starts_with('.') {
                        crate::structure_index::ImportForm::Member
                    } else {
                        crate::structure_index::ImportForm::Value
                    },
                    branches: Vec::new(),
                }
            })
        })
        .collect()
}

fn declaration_is_async(language: &crate::Language, source: &str) -> bool {
    let header = source.split(['{', ':']).next().unwrap_or(source);
    match language {
        crate::Language::Rust => header.contains("async fn "),
        crate::Language::Python => header.trim_start().starts_with("async def "),
        crate::Language::JavaScript | crate::Language::TypeScript => {
            header.contains("async function ") || header.trim_start().starts_with("async ")
        }
        _ => false,
    }
}

fn import_is_reexport(language: &crate::Language, source: &str) -> bool {
    let source = source.trim_start();
    match language {
        crate::Language::Rust => source.starts_with("pub use "),
        crate::Language::JavaScript | crate::Language::TypeScript => {
            source.starts_with("export ") && source.contains(" from ")
        }
        _ => false,
    }
}

/// The call's value feeds an assignment, argument, element, or return,
/// judged from the nearest significant token before it.
fn result_consumed(source: &str, start: usize) -> bool {
    let before = source[..start].trim_end();
    let before = before.strip_suffix("await").unwrap_or(before).trim_end();
    before.ends_with(['=', '(', ',', '[', '{'])
        || ["return", "yield", "lambda"]
            .iter()
            .any(|keyword| before.ends_with(keyword))
}

fn collect_calls(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureCall> {
    let scope = (file.language == Language::Go)
        .then(|| crate::api_identity::ImportScope::go(tree.root_node()));
    let call_apis = super::languages::call_apis(file, tree);
    let consumers = super::languages::call_consumers(file, tree);
    facts
        .calls
        .iter()
        .map(|call| StructureCall {
            callee: compact_text(tree.text_for_span(call.callee)),
            receiver: call
                .receiver
                .map(|span| compact_text(tree.text_for_span(span))),
            arguments: call
                .arguments
                .map(|span| bounded_expression(tree.text_for_span(span))),
            expression: bounded_expression(tree.text_for_span(call.span)),
            span: call.span,
            owner: nearest_function_owner(symbols, call.owner),
            control_region: call.control_region,
            line: call.span.start_line,
            result_consumed: result_consumed(tree.source(), call.span.start_byte),
            api: scope
                .as_ref()
                .and_then(|scope| {
                    scope.resolve_text(&compact_text(tree.text_for_span(call.callee)))
                })
                .or_else(|| {
                    call_apis
                        .get(&(call.span.start_byte, call.span.end_byte))
                        .cloned()
                }),
            consumer: consumers
                .get(&(call.span.start_byte, call.span.end_byte))
                .cloned(),
        })
        .collect()
}

fn java_async_spans(file: &crate::AuditedFile, tree: &auditor_ast::AstTree) -> Vec<SourceSpan> {
    if file.language != Language::Java {
        return Vec::new();
    }
    crate::java_async::async_callbacks(tree.root_node())
        .into_iter()
        .map(SourceSpan::from_node)
        .collect()
}

fn collect_assignments(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureAssignment> {
    facts
        .assignments
        .iter()
        .filter_map(|assignment| {
            let target = assignment
                .target
                .map(|span| compact_text(tree.text_for_span(span)))
                .unwrap_or_default();
            (!target.is_empty()).then_some(StructureAssignment {
                target,
                value: assignment
                    .value
                    .map(|span| bounded_expression(tree.text_for_span(span))),
                expression: bounded_expression(tree.text_for_span(assignment.span)),
                span: assignment.span,
                owner: nearest_function_owner(symbols, assignment.owner),
                control_region: assignment.control_region,
                line: assignment.span.start_line,
            })
        })
        .collect()
}

fn collect_controls(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureControl> {
    let switch_subjects = go_case_switch_subjects(file, tree);
    facts
        .control_regions
        .iter()
        .enumerate()
        .map(|(index, control)| StructureControl {
            owner: nearest_function_owner(symbols, control.owner),
            line: control.span.start_line,
            span: control.span,
            condition: control
                .condition
                .map(|span| bounded_expression(tree.text_for_span(span)))
                .or_else(|| switch_subjects.get(&control.span.start_byte).cloned()),
            parent: control.parent,
            depth: control_depth(&facts.control_regions, symbols, Some(index as u32)),
            kind: control.kind,
        })
        .collect()
}

/// Go case arms keyed by start byte, with the subject of their switch
/// (`key` in `switch key { case "a": .. }`), which a case branch is guarded by.
fn go_case_switch_subjects(
    file: &crate::AuditedFile,
    tree: &auditor_ast::AstTree,
) -> BTreeMap<usize, String> {
    if file.language != Language::Go {
        return BTreeMap::new();
    }
    crate::ast_scan::nodes_of_kind(tree.root_node(), &["expression_case"])
        .into_iter()
        .filter_map(|case| {
            let switch = case
                .parent()
                .filter(|parent| parent.kind() == "expression_switch_statement")?;
            let subject = switch.child_by_field_name("value")?;
            Some((case.start_byte(), compact_text(subject.text())))
        })
        .collect()
}

fn collect_loops(
    tree: &auditor_ast::AstTree,
    facts: &auditor_ast::SemanticFileFacts,
    symbols: &[StructureSymbol],
) -> Vec<StructureLoop> {
    facts
        .loops
        .iter()
        .map(|loop_fact| StructureLoop {
            owner: nearest_function_owner(symbols, loop_fact.owner),
            binding: loop_fact
                .binding
                .map(|span| util::identifiers(tree.text_for_span(span)))
                .unwrap_or_default(),
            iterable: loop_fact
                .iterable
                .map(|span| bounded_expression(tree.text_for_span(span))),
            iterable_span: loop_fact.iterable,
            span: loop_fact.span,
            control_region: loop_fact.control_region,
        })
        .collect()
}

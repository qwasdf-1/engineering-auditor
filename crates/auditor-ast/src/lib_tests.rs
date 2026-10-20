use super::*;

fn fixture_path(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).expect("fixture not found")
}

fn parser() -> AstParser {
    AstParser::new().expect("failed to create parser")
}

#[test]
fn every_grammar_loads() {
    for language in languages::SUPPORTED_LANGUAGES {
        languages::get_language(language)
            .unwrap_or_else(|_| panic!("failed to load grammar for {language:?}"));
    }
}

#[test]
fn c_and_cpp_declarators_name_their_functions() {
    let c = "#ifdef __cplusplus\nstatic Bigint *\nlshift(Bigint *b, int k) { return b; }\n#endif\n";
    let tree = parser().parse(&AstLang::C, c).unwrap();
    let names = tree
        .function_spans()
        .iter()
        .map(|span| span.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, ["lshift"]);
    let cpp = "int &Table::at(int i) { return cells[i]; }\nvoid Table::operator()(int) {}\n";
    let tree = parser().parse(&AstLang::Cpp, cpp).unwrap();
    let names = tree
        .function_spans()
        .iter()
        .map(|span| span.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, ["at", "operator()"]);
}

#[test]
fn functions_are_named_by_their_name_not_their_return_type_or_parameter() {
    let names = |lang: AstLang, source: &str| {
        let tree = parser().parse(&lang, source).unwrap();
        tree.function_spans()
            .iter()
            .map(|span| span.name.clone())
            .collect::<Vec<_>>()
    };
    let csharp = "class P {\n  async Task LoopAsync() { }\n  static TextToken ParseTextToken(string s) { return null; }\n}\n";
    assert_eq!(
        names(AstLang::CSharp, csharp),
        ["LoopAsync", "ParseTextToken"]
    );
    let ts = "function apply(patches: string[]) {\n  patches.forEach(patch => { use(patch) })\n}\n";
    assert!(!names(AstLang::TypeScript, ts).contains(&"patch".to_string()));
    let cpp = "NLOHMANN_JSON_NAMESPACE_BEGIN\nnamespace detail {\nint f(int a) { return a; }\n}\nNLOHMANN_JSON_NAMESPACE_END\n";
    assert_eq!(names(AstLang::Cpp, cpp), ["f"]);
}

#[test]
fn fixtures_expose_expected_function_names() {
    let cases: &[(AstLang, &str, &[&str])] = &[
        (AstLang::Rust, "rust_sample.rs", &["hello", "greet"]),
        (AstLang::Python, "python_sample.py", &["greet", "add"]),
        (
            AstLang::JavaScript,
            "javascript_sample.js",
            &["add", "greet"],
        ),
        (AstLang::TypeScript, "typescript_sample.ts", &["greet"]),
        (AstLang::Go, "go_sample.go", &["add", "greet"]),
        (AstLang::Java, "java_sample.java", &["add"]),
        (AstLang::Kotlin, "kotlin_sample.kt", &["greet"]),
        (AstLang::Bash, "bash_sample.sh", &["hello"]),
        (AstLang::CSharp, "csharp_sample.cs", &["Add", "Greet"]),
        (AstLang::Julia, "julia_sample.jl", &["add", "greet"]),
        (AstLang::C, "c_sample.c", &["add"]),
        (AstLang::Cpp, "cpp_sample.cpp", &["add", "multiply"]),
        (AstLang::Ruby, "ruby_sample.rb", &["add", "greet"]),
    ];
    let mut parser = parser();
    for (language, fixture, expected) in cases {
        let source = read_fixture(fixture);
        let tree = parser.parse(language, &source).unwrap();
        let functions = extract_functions(&tree, language);
        assert!(!functions.is_empty(), "{language:?} produced no functions");
        for name in *expected {
            assert!(
                functions.iter().any(|function| function.name == *name),
                "{language:?} did not expose {name}; functions={functions:?}"
            );
        }
    }
}

#[test]
fn rust_comments_and_nodes_are_traversable() {
    let source = read_fixture("rust_sample.rs");
    let mut parser = parser();
    let tree = parser.parse(&AstLang::Rust, &source).unwrap();
    assert!(!extract_comments(&tree, &AstLang::Rust).is_empty());

    let tree = parser.parse(&AstLang::Rust, "fn foo() {}").unwrap();
    let root = tree.root_node();
    let children = root.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].kind(), "function_item");
}

#[test]
fn empty_source_has_no_functions() {
    let mut parser = parser();
    let tree = parser.parse(&AstLang::Rust, "// just a comment\n").unwrap();
    assert!(extract_functions(&tree, &AstLang::Rust).is_empty());
}

#[test]
fn function_spans_use_one_based_inclusive_lines() {
    let mut parser = parser();
    let tree = parser
        .parse(&AstLang::Rust, "fn a() {}\n\nfn b() {\n  let x = 1;\n}\n")
        .unwrap();
    let functions = extract_functions(&tree, &AstLang::Rust);
    assert_eq!(functions.len(), 2);
    assert_eq!(
        (
            &functions[0].name,
            functions[0].start_line,
            functions[0].end_line
        ),
        (&"a".to_string(), 1, 1)
    );
    assert_eq!(
        (
            &functions[1].name,
            functions[1].start_line,
            functions[1].end_line
        ),
        (&"b".to_string(), 3, 5)
    );
}

#[test]
fn shared_parse_reuses_source_spans_and_semantic_facts() {
    let source: Arc<str> = "fn first() {}\nfn second() {}\n".into();
    let mut parser = parser();
    let tree = parser
        .parse_shared(&AstLang::Rust, Arc::clone(&source))
        .unwrap();

    assert!(Arc::ptr_eq(&source, &tree.shared_source()));
    assert!(Arc::ptr_eq(&tree.function_spans(), &tree.function_spans()));
    let facts = tree.semantic_facts();
    assert!(Arc::ptr_eq(&facts, &tree.semantic_facts()));
    assert!(!facts.symbols.is_empty());
}

#[test]
fn kotlin_fixture_builds_error_free_call_facts() {
    let source = read_fixture("kotlin_sample.kt");
    let mut parser = parser();
    let tree = parser.parse(&AstLang::Kotlin, &source).unwrap();
    let facts = tree.semantic_facts();
    assert!(!facts.parse_has_errors);
    assert!(!facts.calls.is_empty());
}

#[test]
fn supported_language_list_is_parseable() {
    for language in languages::SUPPORTED_LANGUAGES {
        assert!(can_parse(language), "expected {language:?} to be parseable");
    }
}

#[test]
fn typescript_sources_with_jsx_parse_through_the_tsx_grammar() {
    let mut parser = parser();
    let jsx = "export function Card({ title }: { title: string }) {\n  return <div className=\"card\">{title}</div>;\n}\n";
    let tree = parser.parse(&AstLang::TypeScript, jsx).unwrap();
    assert!(!tree.root_node().has_error());
    assert_eq!(tree.root_node().language(), AstLang::TypeScript);
    // A generic arrow and an angle-bracket assertion, which TSX rejects,
    // keep the TypeScript grammar.
    let generic = "const id = <T>(value: T): T => value;\nconst n = <number>input;\n";
    assert!(!parser
        .parse(&AstLang::TypeScript, generic)
        .unwrap()
        .root_node()
        .has_error());
    // A source neither grammar reads keeps its errors.
    let broken = "function ( {\n";
    assert!(parser
        .parse(&AstLang::TypeScript, broken)
        .unwrap()
        .root_node()
        .has_error());
}

fn function_names(tree: &AstTree) -> Vec<String> {
    tree.function_spans()
        .iter()
        .map(|span| span.name.clone())
        .collect()
}

#[test]
fn declarations_with_syntax_errors_are_quarantined_in_a_recovered_tree() {
    let source = "int clean(int a) {\n  return a + 1;\n}\n\nint broken(int a) {\n  return a +* ;\n}\n\nint after(int b) {\n  return b;\n}\n";
    let tree = parser().parse(&AstLang::C, source).unwrap();
    // The tree as written keeps its errors for owners that tolerate them.
    assert!(tree.root_node().has_error());
    assert!(tree.quarantined().is_empty());
    let recovered = tree.error_free().expect("recoverable");
    assert!(!recovered.root_node().has_error());
    assert_eq!(function_names(recovered), ["clean", "after"]);
    let quarantined = recovered.quarantined();
    assert_eq!(quarantined.len(), 1);
    assert_eq!((quarantined[0].start_line, quarantined[0].end_line), (5, 7));
    assert!(quarantined[0].text.starts_with("int broken"));
    // Offsets and lines are kept: `after` is still on line 9.
    assert_eq!(recovered.function_spans()[1].start_line, 9);
    assert_eq!(recovered.source().len(), source.len());
}

#[test]
fn a_function_split_by_an_error_is_quarantined_whole() {
    let source = "int clean(int a) {\n  int total = a * 2 + 1;\n  if (total > 10) {\n    total -= 10;\n  }\n  return total;\n}\n\nint other(int a) {\n  int total = a * 3 + 1;\n  while (total > 10) {\n    total -= 10;\n  }\n  return total;\n}\n\nint split(int a) {\n  int b = 0;\n#ifdef WIDE\n  if (a) {\n#else\n  if (!a) {\n#endif\n    b = 1;\n  }\n  return b;\n}\n";
    let tree = parser().parse(&AstLang::C, source).unwrap();
    assert!(tree.root_node().has_error());
    let recovered = tree.error_free().expect("recoverable");
    assert_eq!(function_names(recovered), ["clean", "other"]);
    for quarantine in recovered.quarantined() {
        for span in recovered.function_spans().iter() {
            assert!(
                span.end_byte <= quarantine.start_byte || quarantine.end_byte <= span.start_byte
            );
        }
    }
}

#[test]
fn a_source_mostly_in_error_is_not_recovered() {
    let source = "int clean(void) { return 0; }\n@@@ ### $$$ %%% ^^^ &&& *** ((( ))) {{{ }}} [[[ ]]] !!! ~~~ \n";
    let tree = parser().parse(&AstLang::C, source).unwrap();
    assert!(tree.root_node().has_error());
    assert!(tree.error_free().is_none());
}

use crate::{AstLang, AstParser, ControlRegionKind};

#[test]
fn facts_are_spans_into_the_original_source() {
    let source =
        "use crate::store;\nfn load(items: &[Item]) { for item in items { store(item.id); } }\n";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Rust, source).unwrap();
    let facts = tree.semantic_facts();

    assert!(!facts.parse_has_errors);
    assert_eq!(facts.imports.len(), 1);
    assert_eq!(facts.loops.len(), 1);
    assert_eq!(facts.statements.len(), 2);
    assert!(facts
        .calls
        .iter()
        .any(|call| tree.text_for_span(call.callee).contains("store")));
    assert!(facts
        .symbols
        .iter()
        .filter_map(|symbol| symbol.name)
        .any(|name| tree.text_for_span(name).contains("load")));
}

#[test]
fn function_metrics_are_semantic_owner_facts() {
    let source = "fn choose(value: usize) -> usize { let candidate = value; if candidate > 0 { return candidate; } candidate }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Rust, source).unwrap();
    let facts = tree.semantic_facts();

    assert_eq!(facts.returns.len(), 1);
    assert!(facts.statements.len() >= 2);
    assert!(facts.returns[0].owner.is_some());
}

#[test]
fn else_if_chains_do_not_add_nesting_depth() {
    let source = "fn classify(value: usize) { if value == 1 {} else if value == 2 {} else if value == 3 {} }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Rust, source).unwrap();
    let controls = &tree.semantic_facts().control_regions;
    let conditionals = controls
        .iter()
        .filter(|control| control.kind == ControlRegionKind::Conditional)
        .collect::<Vec<_>>();

    assert_eq!(conditionals.len(), 3);
    assert!(conditionals[0].nesting_increment);
    assert!(conditionals[1..]
        .iter()
        .all(|control| !control.nesting_increment));
}

#[test]
fn handlers_and_question_marks_do_not_add_nesting_depth() {
    let mut parser = AstParser::new().unwrap();
    let java = parser
        .parse(
            &AstLang::Java,
            "class A { void f() { try { g(); } catch (Exception e) { h(); } } }",
        )
        .unwrap();
    let increments = |controls: &[crate::ControlRegionFact], kind| {
        controls
            .iter()
            .filter(|control| control.kind == kind)
            .map(|control| control.nesting_increment)
            .collect::<Vec<_>>()
    };
    let controls = &java.semantic_facts().control_regions;
    assert_eq!(increments(controls, ControlRegionKind::Try), [true]);
    assert_eq!(increments(controls, ControlRegionKind::Catch), [false]);

    let rust = parser
        .parse(&AstLang::Rust, "fn f() -> Result<(), E> { g()?; Ok(()) }")
        .unwrap();
    let controls = &rust.semantic_facts().control_regions;
    assert_eq!(increments(controls, ControlRegionKind::Try), [false]);
}

#[test]
fn branch_regions_preserve_mutually_exclusive_control_paths() {
    let source = "fn inspect(mode: Mode, ready: bool) { match mode { Mode::First => { if ready { first(); } else { fallback(); } }, Mode::Second => second(), } }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Rust, source).unwrap();
    let controls = &tree.semantic_facts().control_regions;
    let match_region = controls
        .iter()
        .position(|control| control.kind == ControlRegionKind::Match)
        .unwrap() as u32;
    let conditional_region = controls
        .iter()
        .position(|control| control.kind == ControlRegionKind::Conditional)
        .unwrap() as u32;

    assert_eq!(
        controls
            .iter()
            .filter(|control| {
                control.kind == ControlRegionKind::Branch && control.parent == Some(match_region)
            })
            .count(),
        2
    );
    assert_eq!(
        controls
            .iter()
            .filter(|control| {
                control.kind == ControlRegionKind::Branch
                    && control.parent == Some(conditional_region)
            })
            .count(),
        2
    );
}

#[test]
fn malformed_source_records_parser_errors_without_panicking() {
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Rust, "fn broken( {").unwrap();
    assert!(tree.semantic_facts().parse_has_errors);
}

#[test]
fn every_source_frontend_builds_scan_owned_semantic_facts() {
    let cases = [
        (
            AstLang::Rust,
            "fn work() { for item in [1] { consume(item); } }",
        ),
        (
            AstLang::Python,
            "def work():\n    for item in [1]:\n        consume(item)\n",
        ),
        (
            AstLang::JavaScript,
            "function work() { for (const item of [1]) { consume(item); } }",
        ),
        (
            AstLang::TypeScript,
            "function work(): void { for (const item of [1]) { consume(item); } }",
        ),
        (
            AstLang::Go,
            "package p\nfunc work() { for _, item := range []int{1} { consume(item) } }",
        ),
        (
            AstLang::Java,
            "class Sample { void work() { for (int item : new int[]{1}) { consume(item); } } }",
        ),
        (
            AstLang::Kotlin,
            "fun work() { for (item in listOf(1)) { consume(item) } }",
        ),
        (
            AstLang::CSharp,
            "class Sample { void Work() { foreach (var item in new[]{1}) { Consume(item); } } }",
        ),
        (
            AstLang::C,
            "void work(void) { for (int item = 0; item < 1; item++) { consume(item); } }",
        ),
        (
            AstLang::Cpp,
            "void work() { for (auto item : {1}) { consume(item); } }",
        ),
        (
            AstLang::Ruby,
            "def work\n  [1].each do |item|\n    consume(item)\n  end\nend\n",
        ),
        (
            AstLang::Julia,
            "function work()\n  for item in [1]\n    consume(item)\n  end\nend\n",
        ),
        (
            AstLang::Bash,
            "work() { for item in 1; do consume \"$item\"; done; }\n",
        ),
    ];
    let mut parser = AstParser::new().unwrap();
    for (language, source) in cases {
        let tree = parser.parse(&language, source).unwrap();
        let facts = tree.semantic_facts();
        assert!(
            !facts.parse_has_errors,
            "{language:?} frontend produced parse errors"
        );
        assert!(
            !facts.symbols.is_empty(),
            "{language:?} produced no symbols"
        );
        assert!(
            !facts.calls.is_empty() || !facts.loops.is_empty(),
            "{language:?} produced neither call nor loop facts"
        );
        assert!(std::sync::Arc::ptr_eq(&facts, &tree.semantic_facts()));
    }
}

#[test]
fn typescript_loop_facts_preserve_binding_iterable_and_call_context() {
    let source = "function select(ids: string[], wanted: string[]) { for (const id of ids) { if (wanted.includes(id)) consume(id) } }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::TypeScript, source).unwrap();
    let facts = tree.semantic_facts();
    assert_eq!(facts.loops.len(), 1);
    assert_eq!(
        facts.loops[0].binding.map(|span| tree.text_for_span(span)),
        Some("id")
    );
    assert_eq!(
        facts.loops[0].iterable.map(|span| tree.text_for_span(span)),
        Some("ids")
    );
    let membership = facts.calls.iter().find(|call| {
        tree.text_for_span(call.callee) == "wanted.includes"
            && call.arguments.map(|span| tree.text_for_span(span)) == Some("(id)")
    });
    let membership_region = membership.and_then(|call| call.control_region).unwrap();
    assert_eq!(
        facts.control_regions[membership_region as usize].parent,
        Some(facts.loops[0].control_region)
    );
}

#[test]
fn csharp_foreach_and_cpp_range_for_are_loops_with_a_binding_and_iterable() {
    let cases = [
        (
            AstLang::CSharp,
            "class C { void Run(List<int> ids) { foreach (var id in ids) { Use(id); } } }",
            "id",
        ),
        (
            AstLang::Cpp,
            "void run(const std::vector<int>& ids) { for (const auto& id : ids) { use(id); } }",
            "& id",
        ),
    ];
    let mut parser = AstParser::new().unwrap();
    for (language, source, binding) in cases {
        let tree = parser.parse(&language, source).unwrap();
        let facts = tree.semantic_facts();
        assert_eq!(facts.loops.len(), 1, "{language:?}");
        let fact = facts.loops[0];
        assert_eq!(
            fact.binding.map(|span| tree.text_for_span(span)),
            Some(binding),
            "{language:?}"
        );
        assert_eq!(
            fact.iterable.map(|span| tree.text_for_span(span)),
            Some("ids"),
            "{language:?}"
        );
        let call = facts
            .calls
            .iter()
            .find(|call| call.control_region.is_some());
        assert_eq!(
            call.and_then(|call| call.control_region),
            Some(fact.control_region),
            "{language:?}"
        );
    }
}

#[test]
fn go_parameters_are_one_symbol_per_signature_name() {
    // Results, function types, and closure parameters reuse the parameter
    // node kinds but are not the declaration's parameters.
    let source = "package p\n\
        func (s *Server) Handle(a, b int, name string, rest ...string) (int, error) {\n\
            run := func(x, y int) {}\n\
            _ = run\n\
            return 0, nil\n\
        }\n\
        func Unnamed(int, string) func(int) int { return nil }\n";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Go, source).unwrap();
    let facts = tree.semantic_facts();
    let parameters = |function: &str| {
        let owner = facts
            .symbols
            .iter()
            .position(|symbol| {
                symbol
                    .name
                    .is_some_and(|name| tree.text_for_span(name) == function)
            })
            .unwrap() as u32;
        facts
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == crate::SymbolKind::Parameter)
            .filter(|symbol| symbol.parent == Some(owner))
            .map(|symbol| symbol.name.map(|name| tree.text_for_span(name).to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        parameters("Handle"),
        ["a", "b", "name", "rest"].map(|name| Some(name.to_string()))
    );
    assert_eq!(parameters("Unnamed").len(), 2);
    let grouped = facts
        .symbols
        .iter()
        .find(|symbol| {
            symbol
                .name
                .is_some_and(|name| tree.text_for_span(name) == "b")
        })
        .unwrap();
    assert_eq!(tree.text_for_span(grouped.span), "a, b int");
}

#[test]
fn go_range_loop_facts_preserve_binding_and_iterable() {
    let source = "package parity\nfunc copyValues(items []int) []int {\n result := make([]int, 0)\n for _, item := range items { result = append(result, item) }\n return result\n}\n";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::Go, source).unwrap();
    let facts = tree.semantic_facts();
    assert_eq!(facts.loops.len(), 1);
    assert_eq!(
        facts.loops[0].binding.map(|span| tree.text_for_span(span)),
        Some("_, item")
    );
    assert_eq!(
        facts.loops[0].iterable.map(|span| tree.text_for_span(span)),
        Some("items")
    );
    let initializer = facts
        .assignments
        .iter()
        .find(|assignment| assignment.span.start_line == 3)
        .unwrap();
    assert_eq!(
        initializer.target.map(|span| tree.text_for_span(span)),
        Some("result")
    );
    assert_eq!(
        initializer.value.map(|span| tree.text_for_span(span)),
        Some("make([]int, 0)")
    );
}

#[test]
fn javascript_constructor_calls_preserve_constructor_and_arguments() {
    let source = "function inspect(items) { for (const item of items) { const pattern = new RegExp('^fixed$'); consume(pattern, item); } }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::JavaScript, source).unwrap();
    let facts = tree.semantic_facts();
    let constructor = facts
        .calls
        .iter()
        .find(|call| tree.text_for_span(call.callee) == "RegExp")
        .unwrap();

    assert_eq!(
        constructor.arguments.map(|span| tree.text_for_span(span)),
        Some("('^fixed$')")
    );
    assert_eq!(
        constructor.control_region,
        Some(facts.loops[0].control_region)
    );
}

#[test]
fn declaration_initializers_and_updates_are_assignment_facts() {
    let source = "function inspect(items: number[]) { let result: number[] = []; let limit = 1; for (const item of items) { limit++; result.push(item); } }";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::TypeScript, source).unwrap();
    let facts = tree.semantic_facts();
    let rendered = facts
        .assignments
        .iter()
        .map(|assignment| {
            (
                assignment.target.map(|span| tree.text_for_span(span)),
                assignment.value.map(|span| tree.text_for_span(span)),
            )
        })
        .collect::<Vec<_>>();

    assert!(rendered.contains(&(Some("result"), Some("[]"))));
    assert!(rendered.contains(&(Some("limit"), Some("1"))));
    assert!(rendered.iter().any(|(target, _)| *target == Some("limit")));
}

#[test]
fn java_enhanced_for_and_kotlin_fieldless_nodes_are_normalized() {
    let cases = [
        (
            AstLang::Java,
            "class Source { String render(java.util.List<String> items) { String output = \"\"; for (String item : items) output += item; return output; } }",
        ),
        (
            AstLang::Kotlin,
            "fun render(items: List<String>): String { var output = \"\"; for (item in items) output += item; return output }",
        ),
    ];
    let mut parser = AstParser::new().unwrap();
    for (language, source) in cases {
        let tree = parser.parse(&language, source).unwrap();
        let facts = tree.semantic_facts();
        assert_eq!(facts.loops.len(), 1, "{language:?}");
        assert_eq!(
            facts.loops[0].iterable.map(|span| tree.text_for_span(span)),
            Some("items"),
            "{language:?}"
        );
        assert!(
            facts.assignments.iter().any(|assignment| {
                assignment.target.map(|span| tree.text_for_span(span)) == Some("output")
                    && assignment.value.map(|span| tree.text_for_span(span)) == Some("item")
            }),
            "{language:?} lacks normalized compound assignment"
        );
    }
}

#[test]
fn javascript_parameters_exclude_function_types_and_function_expressions() {
    // A TS function type's parameters and a nested `function (..)` expression's
    // parameters belong to neither the enclosing function nor its signature.
    let source = "export function plugin(app: App, opts: Options, done: (err?: Error) => void) {\n\
        register({ visitor: function (value, key, path, helpers) { return value; } });\n\
        done();\n\
    }\n";
    let mut parser = AstParser::new().unwrap();
    let tree = parser.parse(&AstLang::TypeScript, source).unwrap();
    let facts = tree.semantic_facts();
    let owner = facts
        .symbols
        .iter()
        .position(|symbol| {
            symbol
                .name
                .is_some_and(|name| tree.text_for_span(name) == "plugin")
        })
        .unwrap() as u32;
    let parameters = facts
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == crate::SymbolKind::Parameter && symbol.parent == Some(owner)
        })
        .count();
    assert_eq!(parameters, 3);
}

#[test]
fn python_variadic_parameters_are_not_parameter_symbols_typed_or_not() {
    let source = "def add_task(self, description: str, start: bool = True, *args: int, **fields: Any): pass\n\
                  def plain(a, *args, **kwargs): pass\n";
    let tree = AstParser::new()
        .unwrap()
        .parse(&AstLang::Python, source)
        .unwrap();
    let parameters = tree
        .semantic_facts()
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == crate::SymbolKind::Parameter)
        .filter_map(|symbol| symbol.name.map(|name| tree.text_for_span(name).to_string()))
        .collect::<Vec<_>>();
    assert_eq!(parameters, ["self", "description", "start", "a"]);
}

#[test]
fn lambda_and_function_pointer_parameters_are_not_signature_parameters() {
    let parameters = |language: AstLang, source: &str| {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        tree.semantic_facts()
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == crate::SymbolKind::Parameter)
            .filter_map(|symbol| symbol.name.map(|name| tree.text_for_span(name).to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        parameters(
            AstLang::CSharp,
            "class P { void Run(Action<Context> action, Context context) => Engine.Run((ctx, token) => action(ctx), context); }\n"
        ),
        ["action", "context"]
    );
    assert_eq!(
        parameters(
            AstLang::C,
            "int set_title(const char* title, int (*cb)(int left, int right)) {\n  int (*fp)(long a, long b);\n  return 0;\n}\n"
        )
        .len(),
        2
    );
    assert_eq!(
        parameters(
            AstLang::Python,
            "def run(items):\n    return map(lambda x: x, items)\n"
        ),
        ["items"]
    );
    assert_eq!(
        parameters(
            AstLang::Rust,
            "fn find<M, F>(s: &S, m: M, mut bytes: &[u8], range: R, mut matched: F) -> u8\nwhere\n    F: FnMut(Match) -> bool,\n{ 0 }\n"
        ),
        ["s", "m", "bytes", "range", "matched"]
    );
    assert_eq!(
        parameters(
            AstLang::Kotlin,
            "internal fun String.canon(pos: Int = 0, limit: Int = length, charset: Charset? = null): String = this\n"
        ),
        ["pos", "limit", "charset"]
    );
}

#[test]
fn closure_and_lambda_returns_belong_to_the_closure() {
    let returns = |language: AstLang, source: &str| {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        tree.semantic_facts().returns.len()
    };
    assert_eq!(
        returns(
            AstLang::Rust,
            "fn spawn(w: W) -> u8 {\n    w.run(|| { if a { return 1; } return 2; });\n    return 3;\n}\n"
        ),
        1
    );
    assert_eq!(
        returns(
            AstLang::Java,
            "class A { int f() { list.forEach(x -> { return; }); return 0; } }\n"
        ),
        1
    );
}

#[test]
fn associated_functions_and_class_expressions_are_typed_symbols() {
    let kinds = |language: AstLang, source: &str| {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        let facts = tree.semantic_facts();
        facts
            .symbols
            .iter()
            .filter_map(|symbol| {
                let name = tree.text_for_span(symbol.name?).to_string();
                matches!(
                    symbol.kind,
                    crate::SymbolKind::Function
                        | crate::SymbolKind::Method
                        | crate::SymbolKind::Class
                )
                .then_some((name, symbol.kind))
            })
            .collect::<Vec<_>>()
    };
    use crate::SymbolKind::{Class, Function, Method};
    assert_eq!(
        kinds(
            AstLang::Rust,
            "fn free() {}\nimpl S { fn m(&self) {} }\ntrait T { fn t() {} }\n"
        ),
        [
            ("free".to_string(), Function),
            ("m".to_string(), Method),
            ("t".to_string(), Method)
        ]
    );
    assert_eq!(
        kinds(
            AstLang::Cpp,
            "int free() { return 0; }\nint *Block::Iter::Get() { return 0; }\n"
        ),
        [("free".to_string(), Function), ("Get".to_string(), Method)]
    );
    assert_eq!(
        kinds(
            AstLang::JavaScript,
            "module.exports = class Store { get(k) { return k; } };\n"
        ),
        [("Store".to_string(), Class), ("get".to_string(), Method)]
    );
}

#[test]
fn c_and_cpp_function_symbols_are_named_by_their_declarator() {
    let names = |language: AstLang, source: &str| {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        let facts = tree.semantic_facts();
        facts
            .symbols
            .iter()
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    crate::SymbolKind::Function | crate::SymbolKind::Method
                )
            })
            .filter_map(|symbol| symbol.name.map(|name| tree.text_for_span(name).to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(
            AstLang::C,
            "void uv__io_poll(uv_loop_t* loop, int timeout) {}\nchar *dup(const char *s) { return 0; }\n"
        ),
        ["uv__io_poll", "dup"]
    );
    assert_eq!(
        names(
            AstLang::Cpp,
            "int Block::Iter::Compare(const Slice& a, const Slice& b) const { return 0; }\n"
        ),
        ["Compare"]
    );
}

/// `(target, value)` of each assignment fact in `source`.
fn stores(language: AstLang, source: &str) -> Vec<(Option<String>, Option<String>)> {
    let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
    let facts = tree.semantic_facts();
    facts
        .assignments
        .iter()
        .map(|assignment| {
            let text = |span: Option<crate::SourceSpan>| {
                span.map(|span| tree.text_for_span(span).to_string())
            };
            (text(assignment.target), text(assignment.value))
        })
        .collect()
}

fn store(target: &str, value: Option<&str>) -> (Option<String>, Option<String>) {
    (Some(target.to_string()), value.map(str::to_string))
}

#[test]
fn assignment_facts_share_the_data_flow_layers_shapes() {
    assert_eq!(
        stores(
            AstLang::Kotlin,
            "fun f() {\n    @Ann val x: Int = 1\n    val lazyValue by lazy { 2 }\n}\n"
        ),
        [store("x", Some("1")), store("lazyValue", None)]
    );
    assert_eq!(
        stores(
            AstLang::Go,
            "package p\nfunc f() {\n\tvar x int = 5\n\ti := 0\n\ti++\n}\n"
        ),
        [
            store("x", Some("5")),
            store("i", Some("0")),
            store("i", None)
        ]
    );
    assert_eq!(
        stores(AstLang::C, "void f(int *q) { int *p = q; p++; }\n"),
        [store("p", Some("q")), store("p", None)]
    );
    assert_eq!(
        stores(
            AstLang::CSharp,
            "class C { public int A { get; set; } void F() { var x = G(); } }\n"
        ),
        [store("A", None), store("x", Some("G()"))]
    );
    assert_eq!(
        stores(
            AstLang::TypeScript,
            "let resolve!: (v: number) => void;\nlet n = 1;\n"
        ),
        [store("resolve", None), store("n", Some("1"))]
    );
    assert_eq!(
        stores(
            AstLang::Python,
            "def f():\n    with open(p) as fp:\n        pass\n    try:\n        pass\n    except (TypeError, KeyError) as e:\n        pass\n"
        ),
        [store("fp", Some("open(p)")), store("e", None)]
    );
    assert_eq!(
        stores(
            AstLang::Java,
            "class A { void f() { for (int i = 0; i < 3; i++) {} } }\n"
        ),
        [store("i", Some("0")), store("i", None)]
    );
}

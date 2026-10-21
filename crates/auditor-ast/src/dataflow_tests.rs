use super::*;
use crate::{AstLang, AstParser};

fn tree(language: AstLang, source: &str) -> crate::AstTree {
    AstParser::new().unwrap().parse(&language, source).unwrap()
}

/// The `nth` node (0-based) whose text is `text` and kind is `kind`.
fn nth<'a>(node: AstNode<'a>, kind: &str, text: &str, nth: usize) -> AstNode<'a> {
    fn walk<'a>(node: AstNode<'a>, kind: &str, text: &str, out: &mut Vec<AstNode<'a>>) {
        if node.kind() == kind && node.text() == text {
            out.push(node);
        }
        node.children()
            .for_each(|child| walk(child, kind, text, out));
    }
    let mut found = Vec::new();
    walk(node, kind, text, &mut found);
    found
        .get(nth)
        .copied()
        .unwrap_or_else(|| panic!("no {kind} {text:?} #{nth}"))
}

fn values<'a>(bindings: &[Binding<'a>]) -> Vec<&'a str> {
    bindings
        .iter()
        .map(|binding| binding.value.map_or("<none>", |value| value.text()))
        .collect()
}

fn opaque(_: AstNode<'_>) -> CallFlow {
    CallFlow::Opaque
}

fn through(_: AstNode<'_>) -> CallFlow {
    CallFlow::All
}

#[test]
fn a_later_unconditional_assignment_replaces_an_earlier_one() {
    let source = "function f(a, c) {\n  let x = 'lit';\n  x = a;\n  let y = 'one';\n  if (c) { y = a; }\n  return x + y;\n}\n";
    let tree = tree(AstLang::JavaScript, source);
    let read = nth(tree.root_node(), "identifier", "x", 2);
    let flow = FunctionFlow::enclosing(read);
    assert_eq!(flow.function_name(), Some("f"));
    assert_eq!(values(&flow.reaching(read)), ["a"]);
    let y = nth(tree.root_node(), "identifier", "y", 2);
    assert_eq!(values(&flow.reaching(y)), ["'one'", "a"]);
}

#[test]
fn a_loop_carries_later_bindings_to_the_next_iteration() {
    let source = "function join(parts) {\n  let s = '';\n  for (const p of parts) {\n    use(s);\n    s = s + p;\n  }\n  return s;\n}\n";
    let tree = tree(AstLang::JavaScript, source);
    let read = nth(tree.root_node(), "identifier", "s", 1);
    let flow = FunctionFlow::enclosing(read);
    assert_eq!(values(&flow.reaching(read)), ["''", "s + p"]);
    let p = nth(tree.root_node(), "identifier", "p", 1);
    assert_eq!(values(&flow.reaching(p)), ["parts"]);
}

#[test]
fn a_lambda_parameter_shadows_an_outer_name() {
    let source = "function f(xs) {\n  const x = 'outer';\n  return xs.map((x) => x + 1);\n}\n";
    let tree = tree(AstLang::JavaScript, source);
    let read = nth(tree.root_node(), "identifier", "x", 2);
    let reaching = FunctionFlow::enclosing(read).reaching(read);
    assert_eq!(reaching.len(), 1);
    assert_eq!(reaching[0].kind, BindingKind::Parameter);
}

#[test]
fn origins_follow_bindings_members_and_receivers() {
    let python = "def show(request):\n    t = request.args.get('t')\n    q = 'SELECT * FROM ' + t\n    cur.execute(q)\n";
    let tree = tree(AstLang::Python, python);
    let q = nth(tree.root_node(), "identifier", "q", 1);
    let flow = FunctionFlow::enclosing(q);
    let receiver = |call: AstNode<'_>| {
        if call.text().starts_with("request.") {
            CallFlow::Receiver
        } else {
            CallFlow::Opaque
        }
    };
    let origins = flow.origins(q, &receiver);
    let described = origins
        .iter()
        .map(|origin| match origin {
            Origin::Literal(node) => format!("literal {}", node.text()),
            Origin::Parameter(binding) => format!("parameter {}", binding.name),
            Origin::Call(node) => format!("call {}", node.text()),
            Origin::Member(node) => format!("member {}", node.text()),
            Origin::Unbound(node) => format!("unbound {}", node.text()),
            Origin::Other(node) => format!("other {}", node.text()),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        described,
        [
            "literal 'SELECT * FROM '",
            "call request.args.get('t')",
            "member request.args",
            "parameter request",
        ]
    );
}

#[test]
fn java_parameters_keep_their_annotations() {
    let java = "class C { List f(@RequestParam String table) {\n  String sql = \"SELECT * FROM \" + table;\n  return jdbc.query(sql);\n} }\n";
    let tree = tree(AstLang::Java, java);
    let sql = nth(tree.root_node(), "identifier", "sql", 1);
    let flow = FunctionFlow::enclosing(sql);
    let parameters = flow
        .origins(sql, &opaque)
        .into_iter()
        .filter_map(|origin| match origin {
            Origin::Parameter(binding) => Some(binding.node.text()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(parameters, ["@RequestParam String table"]);
}

#[test]
fn go_declarations_bind_positionally() {
    let go = "package p\nfunc h(c *gin.Context) {\n  t, n := c.Query(\"t\"), 10\n  q := fmt.Sprintf(\"SELECT * FROM %s LIMIT %d\", t, n)\n  db.Query(q)\n}\n";
    let tree = tree(AstLang::Go, go);
    let t = nth(tree.root_node(), "identifier", "t", 1);
    let flow = FunctionFlow::enclosing(t);
    assert_eq!(values(&flow.reaching(t)), ["c.Query(\"t\")"]);
    let n = nth(tree.root_node(), "identifier", "n", 1);
    assert_eq!(values(&flow.reaching(n)), ["10"]);
}

#[test]
fn destinations_follow_values_into_bindings_and_returns() {
    let js = "const cnonce = md5(`${Math.random()}`);\nfunction makePassword(n) {\n  let password = '';\n  for (let i = 0; i < n; i++) {\n    password += chars.charAt(Math.floor(Math.random() * chars.length));\n  }\n  return password;\n}\nconst id = (() => { return `${Date.now()}-${Math.random()}`; })();\n";
    let tree = tree(AstLang::JavaScript, js);
    let describe = |nth_random: usize| {
        let random = nth(
            tree.root_node(),
            "call_expression",
            "Math.random()",
            nth_random,
        );
        FunctionFlow::enclosing(random)
            .destinations(random, &through)
            .into_iter()
            .map(|destination| match destination {
                Destination::Bound(binding) => format!("bound {}", binding.name),
                Destination::Argument { call, index } => {
                    format!("argument {index} of {}", call.text())
                }
                Destination::Receiver(call) => format!("receiver of {}", call.text()),
                Destination::Property { key } => format!("property {}", key.text()),
                Destination::Returned { function } => format!("returned from {}", function.kind()),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(describe(0), ["bound cnonce"]);
    assert_eq!(
        describe(1),
        ["bound password", "returned from function_declaration"]
    );
    assert_eq!(describe(2), ["returned from arrow_function", "bound id"]);
}

#[test]
fn a_name_is_made_of_the_values_bound_and_appended_to_it() {
    let source = "def f(cur, name, base):\n    alias = \"SELECT 1\"\n    q = alias\n    if name:\n        q += \" AND name = \" + name\n    other = base\n    cur.execute(q, other)\n";
    let tree = tree(AstLang::Python, source);
    let read = nth(tree.root_node(), "identifier", "q", 2);
    let flow = FunctionFlow::enclosing(read);
    let parts = flow
        .assigned(read)
        .iter()
        .map(|part| match part {
            Assigned::Whole(node) => format!("whole {}", node.text()),
            Assigned::Appended(node) => format!("appended {}", node.text()),
            Assigned::Outside(node) => format!("outside {}", node.text()),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        parts,
        ["whole \"SELECT 1\"", "appended \" AND name = \" + name"]
    );
    let other = nth(tree.root_node(), "identifier", "other", 1);
    assert!(matches!(
        flow.assigned(other).as_slice(),
        [Assigned::Outside(node)] if node.text() == "base"
    ));
}

#[test]
fn a_c_declaration_without_a_value_binds_its_variables() {
    let source = "int f(void) {\n  size_t len;\n  char *p, buf[8];\n  int g(int);\n  len = 0;\n  return len + p[0] + buf[0];\n}\n";
    let tree = tree(AstLang::C, source);
    let len = nth(tree.root_node(), "identifier", "len", 2);
    let flow = FunctionFlow::enclosing(len);
    assert_eq!(values(&flow.reaching(len)), ["0"]);
    let names = flow
        .bindings()
        .iter()
        .map(|binding| binding.name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["len", "p", "buf", "len"]);
}

/// One program in each grammar: `x` declared, then replaced by the
/// parameter `a`, then returned; a loop variable `item` over `items`.
struct Conformance {
    language: AstLang,
    source: &'static str,
    /// Node kind of a read of `x` and `item`.
    name_kind: &'static str,
    /// Which occurrence of `x` is the returned read, and of `item` the read
    /// in the loop body.
    x_read: usize,
    item_read: usize,
}

const CONFORMANCE: &[Conformance] = &[
    Conformance {
        language: AstLang::TypeScript,
        source: "function f(a: string, items: number[]): string {\n  let x: string = 'lit';\n  x = a;\n  for (const item of items) { use(item); }\n  return x;\n}\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::Rust,
        source: "fn f(a: &str, items: Vec<i32>) -> String {\n    let mut x = \"lit\";\n    x = a;\n    for item in items { use_it(item); }\n    x.to_string()\n}\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::CSharp,
        source: "class C { string F(string a, List<int> items) {\n    var x = \"lit\";\n    x = a;\n    foreach (var item in items) { Use(item); }\n    return x;\n} }\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::Kotlin,
        source: "fun f(a: String, items: List<Int>): String {\n    var x = \"lit\"\n    x = a\n    for (item in items) { use(item) }\n    return x\n}\n",
        name_kind: "simple_identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::C,
        source: "const char *f(const char *a, int *items) {\n    const char *x = \"lit\";\n    x = a;\n    for (int *item = items; *item; item++) { use(*item); }\n    return x;\n}\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: usize::MAX,
    },
    Conformance {
        language: AstLang::Cpp,
        source: "std::string f(std::string a, std::vector<int> items) {\n    std::string x = \"lit\";\n    x = a;\n    for (int item : items) { use(item); }\n    return x;\n}\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::Ruby,
        source: "def f(a, items)\n  x = \"lit\"\n  x = a\n  for item in items do\n    use(item)\n  end\n  x\nend\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
    Conformance {
        language: AstLang::Julia,
        source: "function f(a, items)\n    x = \"lit\"\n    x = a\n    for item in items\n        use(item)\n    end\n    return x\nend\n",
        name_kind: "identifier",
        x_read: 2,
        item_read: 1,
    },
];

#[test]
fn every_grammar_reads_declarations_reassignments_and_loop_variables() {
    let mut failures = Vec::new();
    for case in CONFORMANCE {
        let tree = tree(case.language, case.source);
        let root = tree.root_node();
        let x = nth(root, case.name_kind, "x", case.x_read);
        let flow = FunctionFlow::enclosing(x);
        let name = flow.function_name();
        if !matches!(name, Some("f" | "F")) {
            failures.push(format!("{:?}: function name {name:?}", case.language));
        }
        let reaching = values(&flow.reaching(x));
        if reaching != ["a"] {
            failures.push(format!("{:?}: x reaches {reaching:?}", case.language));
        }
        let parameter = flow
            .origins(x, &opaque)
            .iter()
            .any(|origin| matches!(origin, Origin::Parameter(binding) if binding.name == "a"));
        if !parameter {
            failures.push(format!("{:?}: x not traced to parameter a", case.language));
        }
        let assignments = flow
            .bindings()
            .iter()
            .filter(|binding| binding.name == "x")
            .map(|binding| flow.reads_of(binding).len())
            .collect::<Vec<_>>();
        if assignments != [0, 1] {
            failures.push(format!(
                "{:?}: reads of each x binding {assignments:?}",
                case.language
            ));
        }
        if case.item_read == usize::MAX {
            continue;
        }
        let item = nth(root, case.name_kind, "item", case.item_read);
        let loop_bindings = flow.reaching(item);
        if !matches!(
            loop_bindings.as_slice(),
            [binding] if binding.kind == BindingKind::LoopVariable
                && binding.value.is_some_and(|value| value.text() == "items")
        ) {
            failures.push(format!(
                "{:?}: item reaches {:?}",
                case.language,
                values(&loop_bindings)
            ));
        } else if flow.reads_of(&loop_bindings[0]) != [item] {
            failures.push(format!(
                "{:?}: loop variable reads {:?}",
                case.language,
                flow.reads_of(&loop_bindings[0])
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bash_variables_are_bound_and_read_through_expansions() {
    let source = "f() {\n  local x=\"lit\"\n  x=\"$1\"\n  for item in \"$@\"; do\n    use \"$item\"\n  done\n  echo \"${x}\"\n}\n";
    let tree = tree(AstLang::Bash, source);
    let root = tree.root_node();
    let x = nth(root, "variable_name", "x", 2);
    let flow = FunctionFlow::enclosing(x);
    assert_eq!(flow.function_name(), Some("f"));
    assert_eq!(values(&flow.reaching(x)), ["\"$1\""]);
    let item = nth(root, "variable_name", "item", 1);
    let bindings = flow.reaching(item);
    assert!(matches!(
        bindings.as_slice(),
        [binding] if binding.kind == BindingKind::LoopVariable
    ));
    let echoed = nth(root, "string", "\"${x}\"", 0);
    assert!(flow
        .origins(echoed, &opaque)
        .iter()
        .any(|origin| matches!(origin, Origin::Unbound(node) if node.text() == "1")));
}

#[test]
fn anonymous_function_parameters_shadow_outer_names_in_every_grammar() {
    let cases: &[(AstLang, &str, &str)] = &[
        (
            AstLang::Ruby,
            "def f(x)\n  items.each { |x| use(x) }\nend\n",
            "identifier",
        ),
        (
            AstLang::Julia,
            "function f(x)\n    map(x -> x + 1, items)\nend\n",
            "identifier",
        ),
        (
            AstLang::Kotlin,
            "fun f(x: Int) {\n    items.map { x -> x + 1 }\n}\n",
            "simple_identifier",
        ),
        (
            AstLang::CSharp,
            "class C { void F(int x) {\n    items.Select(x => x + 1);\n} }\n",
            "identifier",
        ),
        (
            AstLang::Rust,
            "fn f(x: i32) {\n    items.iter().map(|x| x + 1);\n}\n",
            "identifier",
        ),
        (
            AstLang::Cpp,
            "void f(int x) {\n    std::transform(a, b, c, [](int x) { return x + 1; });\n}\n",
            "identifier",
        ),
    ];
    for (language, source, kind) in cases {
        let tree = tree(*language, source);
        let root = tree.root_node();
        // A C# lambda's untyped parameter is an `implicit_parameter`.
        let read = nth(
            root,
            kind,
            "x",
            if *language == AstLang::CSharp { 1 } else { 2 },
        );
        let flow = FunctionFlow::enclosing(read);
        let reaching = flow.reaching(read);
        assert!(
            matches!(
                reaching.as_slice(),
                [binding] if binding.kind == BindingKind::Parameter
                    && binding.target.start_byte() > read.start_byte() - 20
            ),
            "{language:?}: {:?}",
            reaching
        );
    }
}

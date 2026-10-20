use crate::{
    extract_functions, function_complexity, function_node_kinds, npath_complexity, AstLang,
    AstNodeKind, AstParser,
};

fn compute_npath(source: &str, fn_name: &str) -> u128 {
    let mut parser = AstParser::new().expect("failed to create parser");
    let tree = parser.parse(&AstLang::Rust, source).expect("parse failed");
    let functions = extract_functions(&tree, &AstLang::Rust);
    let func = functions
        .iter()
        .find(|f| f.name == fn_name)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let root = tree.root_node();
    for kind in function_node_kinds() {
        let ak = AstNodeKind::from(*kind);
        for node in root.descendants_of_kind(&ak) {
            let (sr, _) = node.start_position();
            let (er, _) = node.end_position();
            if sr + 1 == func.start_line && er + 1 == func.end_line {
                let has_name = node.named_children().any(|c| {
                    matches!(c.kind(), "name" | "identifier" | "word") && c.text() == fn_name
                });
                if has_name {
                    return npath_complexity(&node);
                }
            }
        }
    }
    panic!("function {fn_name} not found in AST");
}

fn compute_nesting_depth(source: &str, fn_name: &str) -> usize {
    let mut parser = AstParser::new().expect("failed to create parser");
    let tree = parser.parse(&AstLang::Rust, source).expect("parse failed");
    let functions = extract_functions(&tree, &AstLang::Rust);
    let function = functions
        .iter()
        .find(|function| function.name == fn_name)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let root = tree.root_node();
    for kind in function_node_kinds() {
        let ast_kind = AstNodeKind::from(*kind);
        for node in root.descendants_of_kind(&ast_kind) {
            let (start, _) = node.start_position();
            let (end, _) = node.end_position();
            if start + 1 == function.start_line && end + 1 == function.end_line {
                return function_complexity(&node, fn_name).nesting_depth;
            }
        }
    }
    panic!("function {fn_name} not found in AST");
}

fn compute_halstead_counts(source: &str, fn_name: &str) -> (usize, usize, usize, usize) {
    let mut parser = AstParser::new().expect("failed to create parser");
    let tree = parser.parse(&AstLang::Rust, source).expect("parse failed");
    let functions = extract_functions(&tree, &AstLang::Rust);
    let function = functions
        .iter()
        .find(|function| function.name == fn_name)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let root = tree.root_node();
    for kind in function_node_kinds() {
        let ast_kind = AstNodeKind::from(*kind);
        for node in root.descendants_of_kind(&ast_kind) {
            let (start, _) = node.start_position();
            let (end, _) = node.end_position();
            if start + 1 == function.start_line && end + 1 == function.end_line {
                let metrics = function_complexity(&node, fn_name).halstead;
                return (
                    metrics.unique_operators,
                    metrics.unique_operands,
                    metrics.total_operators,
                    metrics.total_operands,
                );
            }
        }
    }
    panic!("function {fn_name} not found in AST");
}

fn compute_cognitive(source: &str, fn_name: &str) -> usize {
    cognitive_in(&AstLang::Rust, source, fn_name)
}

fn cognitive_in(lang: &AstLang, source: &str, fn_name: &str) -> usize {
    metrics_in(lang, source, fn_name).cognitive
}

fn metrics_in(lang: &AstLang, source: &str, fn_name: &str) -> crate::FunctionComplexity {
    let mut parser = AstParser::new().expect("failed to create parser");
    let tree = parser.parse(lang, source).expect("parse failed");
    let function = extract_functions(&tree, lang)
        .into_iter()
        .find(|function| function.name == fn_name)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let root = tree.root_node();
    for kind in function_node_kinds() {
        for node in root.descendants_of_kind(&AstNodeKind::from(*kind)) {
            let (start, _) = node.start_position();
            let (end, _) = node.end_position();
            if start + 1 == function.start_line && end + 1 == function.end_line {
                return function_complexity(&node, fn_name);
            }
        }
    }
    panic!("function {fn_name} not found in AST");
}

#[test]
fn nesting_depth_counts_control_flow_not_raw_ast_depth() {
    let sequential = "fn f() { if a { x(); } if b { y(); } }";
    let nested = "fn f() { if a { while b { if c { x(); } } } }";
    assert_eq!(compute_nesting_depth(sequential, "f"), 1);
    assert_eq!(compute_nesting_depth(nested, "f"), 3);
}

#[test]
fn halstead_ignores_nested_function_tokens() {
    let base = "fn outer(input: i32) -> i32 { let value = input + 10; value }";
    let with_nested = "fn outer(input: i32) -> i32 { let value = input + 10; fn inner() { let ignored = alpha + beta + gamma + delta; } value }";
    assert_eq!(
        compute_halstead_counts(base, "outer"),
        compute_halstead_counts(with_nested, "outer")
    );
}

#[test]
fn halstead_ignores_nested_closure_tokens() {
    let empty = "fn outer(input: i32) -> i32 { let callback = || {}; consume(callback); input }";
    let dense = "fn outer(input: i32) -> i32 { let callback = || { let ignored = alpha + beta + gamma + delta; }; consume(callback); input }";
    assert_eq!(
        compute_halstead_counts(empty, "outer"),
        compute_halstead_counts(dense, "outer")
    );
}

#[test]
fn halstead_measures_function_body_not_signature() {
    let short = "fn short() { consume(1); }";
    let wide = "fn wide(first_parameter: VeryLongTypeName, second_parameter: AnotherLongTypeName, third_parameter: ThirdLongTypeName) -> Result<(), LongErrorType> { consume(1); }";
    assert_eq!(
        compute_halstead_counts(short, "short"),
        compute_halstead_counts(wide, "wide")
    );
}

#[test]
fn cognitive_complexity_counts_a_match_once_not_per_arm() {
    // Outer match +1; the inner match nests inside it, +1+1.
    let source = "fn classify(kind: Kind, value: Value) { match kind { Kind::First => match value { Value::One => first(), Value::Two => second() }, Kind::Second => fallback(), } }";
    assert_eq!(compute_cognitive(source, "classify"), 3);
    let java = "class A { int f(int x) { switch (x) { case 1: return 1; case 2: case 3: return 2; default: return 0; } } }";
    assert_eq!(cognitive_in(&AstLang::Java, java, "f"), 1);
}

#[test]
fn cognitive_complexity_counts_recursion_once_per_function() {
    // if (+1), else if (+1), else (+1), recursion (+1 once for three calls).
    let source = "function f(t) {\n  if (t.a) { return f(t.a); } else if (t.b) { return f(t.b); } else { return f(t.c); }\n}\n";
    assert_eq!(cognitive_in(&AstLang::TypeScript, source, "f"), 4);
    let java =
        "class A { int f(int n) { if (n > 0) { return f(n - 1) + this.f(n - 2); } return 0; } }";
    assert_eq!(cognitive_in(&AstLang::Java, java, "f"), 2);
    let python = "class A:\n    def f(self, n):\n        if n:\n            return self.f(n - 1)\n";
    assert_eq!(cognitive_in(&AstLang::Python, python, "f"), 2);
}

#[test]
fn cognitive_complexity_gives_else_if_and_else_no_nesting_credit() {
    let cases = [
        (
            AstLang::Rust,
            "fn f() { if a { x(); } else if b { y(); } else { z(); } }",
        ),
        (
            AstLang::Java,
            "class A { void f() { if (a) { x(); } else if (b) { y(); } else { z(); } } }",
        ),
        (
            AstLang::Go,
            "package p\nfunc f() { if a { x() } else if b { y() } else { z() } }",
        ),
        (
            AstLang::JavaScript,
            "function f() { if (a) { x(); } else if (b) { y(); } else { z(); } }",
        ),
        (
            AstLang::Python,
            "def f():\n    if a:\n        x()\n    elif b:\n        y()\n    else:\n        z()\n",
        ),
    ];
    for (lang, source) in cases {
        assert_eq!(cognitive_in(&lang, source, "f"), 3, "{lang:?}");
    }
    // The else-if body stays at the first if's nesting level: 1 + 1 + (1+1).
    assert_eq!(
        compute_cognitive("fn f() { if a {} else if b { if c {} } }", "f"),
        4
    );
    let chain = "fn f() { if a {} else if b {} else if c {} else if d {} else {} }";
    assert_eq!(compute_nesting_depth(chain, "f"), 1);
}

#[test]
fn cognitive_complexity_counts_runs_of_like_boolean_operators() {
    assert_eq!(
        compute_cognitive("fn f() { if a && b && c || d {} }", "f"),
        3
    );
    assert_eq!(compute_cognitive("fn f() { if a && (b && c) {} }", "f"), 2);
    let python = "def f():\n    if a and b and c or d:\n        x()\n";
    assert_eq!(cognitive_in(&AstLang::Python, python, "f"), 3);
}

#[test]
fn npath_matches_pmd_on_exits_conditions_and_branches() {
    // Expected values are PMD 7.16 NPathComplexity results for the same methods.
    let java = "class N {
  boolean seqReturn(int a, int b, int c) {
if (a > 0) { return true; }
if (b > 0) { return true; }
if (c > 0) { return true; }
return false;
  }
  void seqCall(int a, int b, int c) {
if (a > 0) { x(); }
if (b > 0) { x(); }
if (c > 0) { x(); }
  }
  void boolOps(int a, int b) {
if (a > 0 && b > 0) { x(); }
  }
  void loop(int[] xs) {
for (int v : xs) { if (v > 0) { x(); } }
  }
  int tern(boolean a) {
int v = a ? 1 : 2;
return v;
  }
  void sw(int k) {
switch (k) { case 1: x(); break; case 2: x(); break; default: x(); }
  }
  void chain(int k) {
if (k == 1) { x(); } else if (k == 2) { x(); } else { x(); }
  }
  void cont(int[] xs) {
for (int v : xs) {
  if (v == 1) { x(); continue; }
  if (v == 2) { x(); continue; }
  if (v == 3) { x(); continue; }
  x();
}
  }
  void brk(int a) {
while (true) {
  if (a == 1) { break; }
  if (a == 2) { break; }
  x();
}
  }
  void swret(int k) {
switch (k) { case 1: return; case 2: x(); break; default: x(); }
if (k > 3) { x(); }
  }
  int tryRet(int a) {
try { if (a == 1) { x(); } return a; } catch (RuntimeException e) { if (a == 2) throw e; }
return 0;
  }
  void seqAfterLoop(int[] xs, int a) {
for (int v : xs) { if (v == 1) { return; } }
if (a == 1) { x(); }
  }
  void fallthrough(int c) {
switch (c) { case 1: case 2: case 3: x(); case 4: case 5: break; default: }
  }
  void labeled(int c) {
outer:
while (true) {
  for (; c < 3; c++) { if (c == 1) break outer; }
  if (c > 2) { break; }
}
  }
  void labeledSwitch(int i, int lim, char[] b) {
Object builder = null;
outer:
while (true) {
  for (; i < lim; i++) {
    switch (b[i]) { case '/': case ';': x(); case '{': case '}': break outer; default: }
  }
  if (i < b.length) { if (f()) { continue; } else { break; } }
  if (builder == null) { builder = 1; }
  i = 0;
  if (!f()) { break; }
}
Object r = (builder == null) ? 1 : 2;
  }
  void fallReturn(int c) { switch (c) { case 1: case 2: x(); case 3: case 4: return; default: } }
  void fallLabeled(int c) { o: while (true) { switch (c) { case 1: case 2: x(); case 3: case 4: break o; default: } } }
  void fallInto(int c) { switch (c) { case 1: x(); case 2: x(); break; default: x(); } }
  void noDefault(int c) { switch (c) { case 1: case 2: x(); break; } }
  void multiLabel(int c) { switch (c) { case 1, 2: x(); break; case 3: x(); } }
  void arrows(int c) { switch (c) { case 1, 2 -> x(); case 3 -> { if (c > 0) x(); } default -> x(); } }
  void boolValue(boolean a, boolean b, boolean c) { boolean v = a || (b && c); }
  int ternaryAnd(boolean a, boolean b) { return a && b ? 1 : 0; }
  boolean returnAnd(boolean a, boolean b) { return a && b; }
  void orNested(boolean a, boolean b, boolean c) { if (a || b) { if (c) x(); } }
  void andElse(boolean a, boolean b, boolean c) { if (a && b) { if (c) x(); } else { if (c) x(); } }
  int orReturn(boolean a, boolean b) { if (a || b) return 1; return 0; }
  void orLoop(boolean a, boolean b, boolean c) { while (a || b) { if (c) x(); } }
  void orBreak(boolean a, boolean b, boolean c) { while (true) { if (a || b) break; if (c) x(); } }
  void notAnd(boolean a, boolean b) { if (!(a && b)) x(); }
  void mixedBreak(boolean f, boolean a, boolean b, boolean c) {
while (true) {
  if (a) {
    if (f || !b || (c) && !a) break;
    if (c) { f = true; if ((c) && (!b)) break; }
    continue;
  }
  if (b) break;
  x();
}
  }
}";
    for (name, expected) in [
        ("seqReturn", 4),
        ("seqCall", 8),
        ("boolOps", 3),
        ("loop", 3),
        ("tern", 2),
        ("sw", 3),
        ("chain", 3),
        ("cont", 5),
        ("brk", 3),
        ("swret", 5),
        ("seqAfterLoop", 5),
        ("tryRet", 2),
        ("fallthrough", 6),
        ("labeled", 5),
        ("labeledSwitch", 32),
        ("fallReturn", 5),
        ("fallLabeled", 5),
        ("fallInto", 3),
        ("noDefault", 3),
        ("multiLabel", 4),
        ("arrows", 5),
        ("boolValue", 3),
        ("ternaryAnd", 3),
        ("returnAnd", 2),
        ("orNested", 5),
        ("andElse", 6),
        ("orReturn", 3),
        ("orLoop", 5),
        ("orBreak", 4),
        ("notAnd", 3),
        ("mixedBreak", 13),
    ] {
        assert_eq!(
            metrics_in(&AstLang::Java, java, name).npath,
            expected,
            "{name}"
        );
    }
    let python = "def f(a, b):\n    if a:\n        x()\n    elif b:\n        y()\n";
    assert_eq!(metrics_in(&AstLang::Python, python, "f").npath, 3);
}

#[test]
fn cognitive_complexity_counts_python_except_and_java_enhanced_for() {
    let python = "def f():\n    try:\n        x()\n    except ValueError:\n        y()\n";
    assert_eq!(cognitive_in(&AstLang::Python, python, "f"), 1);
    let java = "class A { void f(int[] xs) { for (int x : xs) { use(x); } } }";
    assert_eq!(cognitive_in(&AstLang::Java, java, "f"), 1);
}

#[test]
fn cognitive_complexity_excludes_nested_closures() {
    let source = "fn outer() { let closure = || { if a { if b { if c {} } } }; }";
    assert_eq!(compute_cognitive(source, "outer"), 0);
}

#[test]
fn cognitive_complexity_excludes_kotlin_and_go_closures() {
    let kotlin = "fun read() { lock.withLock { if (a) { if (b) { log(a) } } } }";
    assert_eq!(cognitive_in(&AstLang::Kotlin, kotlin, "read"), 0);
    let go = "package p\nfunc run() { go func() { if a { if b { log() } } }() }";
    assert_eq!(cognitive_in(&AstLang::Go, go, "run"), 0);
}

#[test]
fn empty_function_has_one_path() {
    let source = "fn f() {}";
    assert_eq!(compute_npath(source, "f"), 1);
}

#[test]
fn function_with_only_returns_has_one_path() {
    let source = "fn f() { let x = 1; return x; }";
    assert_eq!(compute_npath(source, "f"), 1);
}

#[test]
fn single_if_no_else() {
    let source = "fn f() { if a { x; } }";
    assert_eq!(compute_npath(source, "f"), 2);
}

#[test]
fn single_if_else() {
    let source = "fn f() { if a { x; } else { y; } }";
    assert_eq!(compute_npath(source, "f"), 2);
}

#[test]
fn ten_sequential_ifs() {
    let source = "\
fn f() {
if a { x; }
if b { x; }
if c { x; }
if d { x; }
if e { x; }
if f { x; }
if g { x; }
if h { x; }
if i { x; }
if j { x; }
}";
    // 2^10 = 1024
    assert_eq!(compute_npath(source, "f"), 1024);
}

#[test]
fn nested_if_else_combines_multiplicatively() {
    let source = "\
fn f() {
if a {
    if b { x; } else { y; }
} else {
    if c { z; } else { w; }
}
}";
    // inner each = 2 (if-else), outer then(2) + else(2) = 4
    assert_eq!(compute_npath(source, "f"), 4);
}

#[test]
fn if_else_if_else_chain() {
    let source = "\
fn f() {
if a { x; }
else if b { y; }
else { z; }
}";
    // then(x)=1, else_if(b)=1, else(z)=1 => total = 1+1+1 = 3
    assert_eq!(compute_npath(source, "f"), 3);
}

#[test]
fn while_loop_adds_one_path() {
    let source = "fn f() { while a { x; } }";
    // body(1) + 1(exit) = 2
    assert_eq!(compute_npath(source, "f"), 2);
}

#[test]
fn for_loop_adds_one_path() {
    let source = "fn f() { for i in 0..n { x; } }";
    // body(1) + 1(exit) = 2
    assert_eq!(compute_npath(source, "f"), 2);
}

const MATCH_ARMS_EXPECTED_NPATH: u128 = 3;

#[test]
fn match_expression_sums_arms() {
    let source = "\
fn f(x: i32) -> i32 {
match x {
    1 => { 10 },
    2 => { 10 },
    _ => { 10 },
}
}";
    // 3 arms, each body npath=1 => total = 3
    assert_eq!(compute_npath(source, "f"), MATCH_ARMS_EXPECTED_NPATH);
}

#[test]
fn sequential_if_and_loop_multiply() {
    let source = "\
fn f() {
if a { x; }
while b { y; }
}";
    // if(2) * while(2) = 4
    assert_eq!(compute_npath(source, "f"), 4);
}

#[test]
fn deeply_nested_ifs_produce_large_npath() {
    let source = "\
fn f() {
if a { if b { if c { x; } } }
}";
    // if(c): 1+1=2, if(b): then(2)+1=3, if(a): then(3)+1=4
    assert_eq!(compute_npath(source, "f"), 4);
}

#[test]
fn if_else_with_nested_function_stops_at_inner_fn() {
    let source = "\
fn outer() {
if a {
    fn inner() { if b { x; } }
    y;
}
}";
    // outer: inner function is treated as atomic (npath=1)
    // then branch: fn inner(1) * stmt y(1) = 1
    // outer if: then(1) + 1(fallthrough) = 2
    assert_eq!(compute_npath(source, "outer"), 2);
}

#[test]
fn cognitive_complexity_ignores_returned_names_and_counts_labeled_jumps() {
    let java = "class A { int f(int a) { return a; } }";
    assert_eq!(cognitive_in(&AstLang::Java, java, "f"), 0);
    let python = "def f(a):\n    return a\n";
    assert_eq!(cognitive_in(&AstLang::Python, python, "f"), 0);
    // while (+1), for (+2 nested), break label (+1).
    let js = "function f(a) { outer: while (a) { for (const x of a) { break outer; } } }";
    assert_eq!(cognitive_in(&AstLang::JavaScript, js, "f"), 4);
    // `break value` returns a value from `loop`; only `break 'label` is labeled.
    let rust = "fn f(a: i32) -> i32 { loop { break a; } }";
    assert_eq!(cognitive_in(&AstLang::Rust, rust, "f"), 1);
}

#[test]
fn kotlin_npath_reads_when_jumps_and_bodies() {
    let source = "fun route(a: Int, xs: List<Int>): Int {
    if (a > 0) { log(a) } else { log(-a) }
    when (a) {
        1 -> log(1)
        2 -> log(2)
        else -> log(3)
    }
    for (x in xs) { log(x) }
    if (a > 9 && a < 99) return 1
    return 0
}
";
    let tree = AstParser::new()
        .unwrap()
        .parse(&AstLang::Kotlin, source)
        .unwrap();
    let function = tree
        .root_node()
        .descendants_of_kind(&AstNodeKind::from("function_declaration"))
        .remove(0);
    // 2 (if/else) x 3 (when) x 2 (loop entered or skipped) x 3 (the `&&`
    // condition: one way true, two ways false), the exits included.
    assert_eq!(npath_complexity(&function), 36);
}

#[test]
fn kotlin_halstead_counts_soft_keyword_names_in_the_function_body() {
    let source = "fun inspect(value: Int) {\n    consume(value * value + value / 3)\n}\n";
    let tree = AstParser::new()
        .unwrap()
        .parse(&AstLang::Kotlin, source)
        .unwrap();
    let function = tree
        .root_node()
        .descendants_of_kind(&AstNodeKind::from("function_declaration"))
        .remove(0);
    let standalone = crate::halstead(&function);
    // `consume`, three `value`s, and `3`; the signature is not the body.
    assert_eq!(standalone.total_operands, 5);
    let combined = function_complexity(&function, "inspect").halstead;
    assert_eq!(combined.total_operands, standalone.total_operands);
    assert_eq!(combined.total_operators, standalone.total_operators);
}

#[test]
fn cognitive_breakdown_sums_to_the_score() {
    let source = "fn f(a: bool, b: bool, xs: &[u8]) -> u8 {\n    for x in xs {\n        if a && b {\n            while *x > 0 {\n                if a { return 1; } else { return 2; }\n            }\n        }\n    }\n    0\n}\n";
    let metrics = metrics_in(&AstLang::Rust, source, "f");
    let parts = metrics.cognitive_breakdown;
    assert_eq!(
        parts.structural + parts.hybrid + parts.fundamental + parts.nesting_increments,
        metrics.cognitive
    );
    assert_eq!(parts.nesting_increments, 1 + 2 + 3);
}

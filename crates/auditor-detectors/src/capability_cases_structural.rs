use std::fs;

use crate::{AnalysisMethod, ScanProfile};

use super::{CapabilityCase, TempRepo};

fn file(path: &str, source: impl Into<String>) -> (String, String) {
    (path.to_string(), source.into())
}

fn lines(count: usize) -> String {
    (0..count)
        .map(|index| format!("// source line {index}\n"))
        .collect()
}

fn branching_source(branches: usize) -> String {
    let branches = (0..branches)
        .map(|index| format!("    if value == {index} {{ return {index}; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn choose(value: usize) -> usize {{\n{branches}\n    value\n}}\n")
}

/// A flat `match` with many arms: one decision for cognitive complexity.
fn flat_match_source(arms: usize) -> String {
    let arms = (0..arms)
        .map(|index| format!("        {index} => \"{index}\","))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn label(value: usize) -> &'static str {{\n    match value {{\n{arms}\n        _ => \"other\",\n    }}\n}}\n")
}

/// Guard clauses that each return: paths add instead of multiplying.
fn guard_clause_source(guards: usize) -> String {
    let guards = (0..guards)
        .map(|index| format!("    if value == {index} {{ return {index}; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn choose(value: usize) -> usize {{\n{guards}\n    value\n}}\n")
}

fn npath_source(branches: usize) -> String {
    let branches = (0..branches)
        .map(|index| format!("    if value == {index} {{ result += {index}; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "fn choose(value: usize) -> usize {{\n    let mut result = value;\n{branches}\n    result\n}}\n"
    )
}

fn nested_source(depth: usize) -> String {
    let mut source = "fn inspect() {\n".to_string();
    for index in 0..depth {
        source.push_str(&format!(
            "{}if condition_{index} {{\n",
            "    ".repeat(index + 1)
        ));
    }
    source.push_str(&format!("{}work();\n", "    ".repeat(depth + 1)));
    for index in (0..depth).rev() {
        source.push_str(&format!("{}}}\n", "    ".repeat(index + 1)));
    }
    source.push_str("}\n");
    source
}

fn flat_else_if_source() -> &'static str {
    "fn classify(value: usize) -> usize {\n    if value == 1 { 1 } else if value == 2 { 2 } else if value == 3 { 3 } else if value == 4 { 4 } else if value == 5 { 5 } else if value == 6 { 6 } else { 0 }\n}\n"
}

fn local_source(count: usize) -> String {
    let locals = (0..count)
        .map(|index| format!("    let value_{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn calculate() -> usize {{\n{locals}\n    0\n}}\n")
}

fn return_source(count: usize) -> String {
    let returns = (0..count)
        .map(|index| format!("    if value == {index} {{\n        return {index};\n    }}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn choose(value: usize) -> usize {{\n{returns}\n    value\n}}\n")
}

fn statement_source(count: usize) -> String {
    let statements = (0..count)
        .map(|index| format!("    consume({index});"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn process() {{\n{statements}\n}}\n")
}

fn method_source(count: usize) -> String {
    let methods = (0..count)
        .map(|index| format!("    def method_{index}(self):\n        return {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("class CohesiveService:\n{methods}\n")
}

fn function_source(count: usize) -> String {
    (0..count)
        .map(|index| format!("fn operation_{index}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn halstead_source(count: usize) -> String {
    let statements = (1..=count)
        .map(|index| {
            let previous = index - 1;
            format!(
                "    let value_{index} = (input * {index} + value_{previous}) % ((input - {index}) | 1) ^ value_{previous};"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "fn calculate(input: usize) -> usize {{\n    let value_0 = input;\n{statements}\n    input\n}}\n"
    )
}

/// A flat lookup table: high Halstead volume, near-zero difficulty.
fn data_table_source(count: usize) -> String {
    let rows = (0..count)
        .map(|index| format!("        \"entry_{index}\","))
        .collect::<Vec<_>>()
        .join("\n");
    format!("fn names() -> Vec<&'static str> {{\n    vec![\n{rows}\n    ]\n}}\n")
}

fn duplicate_files(similar: bool) -> Vec<(String, String)> {
    (0..5)
        .map(|index| {
            let body = if similar {
                format!(
                    "pub fn process_{index}(input: Vec<i32>) -> i32 {{\n    let mut total_{index} = 0;\n    let mut count_{index} = 0;\n    for item in input {{\n        let scaled = item * 2;\n        total_{index} = total_{index}.wrapping_add(scaled);\n        count_{index} = count_{index}.wrapping_add(item);\n    }}\n    let result = total_{index}.wrapping_add(count_{index});\n    if result > 100 {{\n        return result;\n    }}\n    result\n}}\n"
                )
            } else {
                [
                    "fn enabled_alpha() -> bool { true }\n",
                    "fn enabled_beta() -> bool { true }\n",
                    "fn enabled_gamma() -> bool { true }\n",
                    "fn label() -> &'static str { \"unique\" }\n",
                    "fn ratio() -> f64 { 3.14159 }\n",
                ][index]
                    .to_string()
            };
            file(&format!("src/module_{index}.rs"), body)
        })
        .collect()
}

fn churn_source() -> String {
    let mut source = "pub fn hotspot(data: &[u8]) {\n".to_string();
    for index in 0..12 {
        source.push_str(&format!(
            "    if data.get({index}) == Some(&{index}) {{ return; }}\n"
        ));
    }
    source.push_str(
        "    for a in data { if *a > 0 { for b in 0..*a { if b > 1 { work(b); } } } }\n}\n",
    );
    source
}

fn add_churn(repository: &TempRepo) {
    for index in 0..8 {
        repository.write(
            &format!("src/filler_{index}.rs"),
            format!("pub fn filler_{index}() {{}}\n"),
        );
    }
    repository.git(&["init"]);
    repository.git(&["config", "user.name", "test"]);
    repository.git(&["config", "user.email", "test@example.com"]);
    repository.git(&["add", "src"]);
    repository.git(&["commit", "-m", "initial"]);
    let capability_path = fs::read_dir(repository.path().join("src"))
        .expect("read churn source directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| !stem.starts_with("filler_"))
        })
        .expect("locate churn capability source");
    let capability_relative = capability_path
        .strip_prefix(repository.path())
        .expect("churn source belongs to repository")
        .to_string_lossy()
        .into_owned();
    for index in 0..12 {
        let source = fs::read_to_string(&capability_path).expect("read churn source");
        repository.write(
            &capability_relative,
            format!("// revision {index}\n{source}"),
        );
        if index == 0 {
            for filler in 0..8 {
                repository.write(
                    &format!("src/filler_{filler}.rs"),
                    format!("pub fn filler_{filler}() {{ let value = {filler}; }}\n"),
                );
            }
        }
        repository.git(&["add", "src"]);
        repository.git(&["commit", "-m", &format!("revision {index}")]);
    }
}

/// The Python test-quality boundary; the other Tier-1 frameworks retarget
/// through the same fixtures.
fn test_quality_files(id: &str, positive: bool) -> Vec<(String, String)> {
    super::language_tests::test_quality_capability_files(id, "python", positive)
        .expect("Python test-quality boundary")
}

/// The Python boundary of a Layer 3 rule added for v1.0; the other
/// languages retarget through the same fixtures.
fn l3_rule_files(id: &str, positive: bool) -> Vec<(String, String)> {
    super::language_tests::l3_rule_capability_files(id, "python", positive)
        .expect("Python Layer 3 rule boundary")
}

/// The Python boundary of a dependency or error hygiene rule; the other
/// languages retarget through the same fixtures.
fn hygiene_files(id: &str, positive: bool) -> Vec<(String, String)> {
    super::language_tests::hygiene_capability_files(id, "python", positive)
        .expect("Python hygiene boundary")
}

/// The Python network-timeout boundary; the other Tier-1 client tables
/// retarget through the same fixtures.
fn network_timeout_files(positive: bool) -> Vec<(String, String)> {
    super::language_tests::network_timeout_capability_files("python", positive)
        .expect("Python network-timeout boundary")
}

pub(super) fn default_cases() -> Vec<CapabilityCase> {
    vec![
        CapabilityCase::new("unused_declared_dependency", ScanProfile::Full, "", "", "", "")
            .positive_files(hygiene_files("unused_declared_dependency", true))
            .negative_files(hygiene_files("unused_declared_dependency", false)),
        CapabilityCase::new("discarded_fallible_result", ScanProfile::Full, "", "", "", "")
            .positive_files(hygiene_files("discarded_fallible_result", true))
            .negative_files(hygiene_files("discarded_fallible_result", false)),
        CapabilityCase::new("quadratic_front_removal", ScanProfile::Full, "", "", "", "")
            .positive_files(l3_rule_files("quadratic_front_removal", true))
            .negative_files(l3_rule_files("quadratic_front_removal", false)),
        CapabilityCase::new("catastrophic_backtracking_regex", ScanProfile::Full, "", "", "", "")
            .positive_files(l3_rule_files("catastrophic_backtracking_regex", true))
            .negative_files(l3_rule_files("catastrophic_backtracking_regex", false)),
        CapabilityCase::new("network_call_without_timeout", ScanProfile::Full, "", "", "", "")
            .positive_files(network_timeout_files(true))
            .negative_files(network_timeout_files(false)),
        CapabilityCase::new("test_without_assertion", ScanProfile::Full, "", "", "", "")
            .positive_files(test_quality_files("test_without_assertion", true))
            .negative_files(test_quality_files("test_without_assertion", false)),
        CapabilityCase::new("tautological_assertion", ScanProfile::Full, "", "", "", "")
            .positive_files(test_quality_files("tautological_assertion", true))
            .negative_files(test_quality_files("tautological_assertion", false)),
        CapabilityCase::new(
            "generated_vs_source",
            ScanProfile::Gating,
            "docs/diagnostics/scan.md",
            "temporary diagnostic\n",
            "docs/scan.md",
            "durable documentation\n",
        ),
        CapabilityCase::new(
            "large_file",
            ScanProfile::Gating,
            "src/large.rs",
            lines(1000),
            "src/large.rs",
            lines(999),
        ),
        CapabilityCase::new(
            "stale_path_reference",
            ScanProfile::Gating,
            "README.md",
            "See `docs/missing.md`.\n",
            "README.md",
            "See `docs/guide.md`.\n",
        )
        .negative_files(vec![
            file(
                "README.md",
                "See `docs/guide.md`. Run `tooling/runtime-cargo.sh fmt --check --all`.\n",
            ),
            file("docs/guide.md", "# Guide\n"),
            file("tooling/runtime-cargo.sh", "#!/bin/sh\nexec cargo \"$@\"\n"),
        ]),
        CapabilityCase::new(
            "stale_link_reference",
            ScanProfile::Gating,
            "docs/README.md",
            "See [guide](missing.md).\n",
            "docs/README.md",
            "See [guide](guide.md).\n",
        )
        .negative_files(vec![
            file(
                "docs/README.md",
                "See [guide](guide.md), [design](guide.md \"Design notes\"), [site page](site), and [`Router`](crate::routing::Router).\nSee the [FAQ]({{ faq_url }}).\n`[shown](not-a-link.md)`\n\n```md\n[example](example.md)\n```\n",
            ),
            file("docs/guide.md", "# Guide\n"),
            file("docs/site.mdx", "# Site\n"),
            file(
                "docs/react.md",
                "---\nid: introduction\nslug: /react\n---\nRead the [quickstart](quickstart) and [queries](react-queries).\n",
            ),
        ]),
        CapabilityCase::new(
            "stale_section_anchor",
            ScanProfile::Gating,
            "README.md",
            "See [guide](docs/guide.md#missing).\n",
            "README.md",
            "See [guide](docs/guide.md#getting-started).\n",
        )
        .positive_files(vec![
            file("README.md", "See [guide](docs/guide.md#missing).\n"),
            file("docs/guide.md", "# Getting Started\n"),
        ])
        .negative_files(vec![
            file(
                "README.md",
                "See [guide](docs/guide.md#getting-started), [line](docs/guide.md#L1-L2), [runbook](docs/guide.md#37-incident-response), [setup](docs/guide.md#proguard--r8), [options](docs/guide.md#options), and [faq](docs/guide.md#colors).\n",
            ),
            file(
                "docs/guide.md",
                "# Getting Started\n## 3.7 Incident response\n## ProGuard / R8\n## `httpBatchLink` Options {#options}\n<h3 name=\"colors\">Colors</h3>\n",
            ),
        ]),
        CapabilityCase::new(
            "function_complexity",
            ScanProfile::Gating,
            "src/lib.rs",
            branching_source(10),
            "src/lib.rs",
            branching_source(2),
        )
        .method(AnalysisMethod::Token),
        CapabilityCase::new(
            "too_many_parameters",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn execute(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) {}\n",
            "src/lib.rs",
            "fn execute(a: i32, b: i32, c: i32, d: i32, e: i32) {}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn execute(a: i32, b: i32, c: i32, d: i32, e: i32) {}\n"),
            file(
            "src/progress.py",
            "from typing import overload\n\n@overload\ndef track(a, b, c, d, e, f): ...\n\ndef track(*args): ...\n",
        ),
        ]),
        CapabilityCase::new(
            "section_divider_comment",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn execute() {\n// ====\nwork();\n// ====\nwork();\n// ====\n}\n",
            "src/lib.rs",
            "fn execute() {\n// ====\nwork();\n// ====\n}\n",
        )
        .negative_files(vec![file(
            "src/lib.rs",
            "fn execute() {\n    // ====\n    work();\n    // Pass -e/--regexp or -f/--file; snake_case_names_are_prose.\n    #[allow(clippy::declare_interior_mutable_const)]\n    const A: u8 = 1;\n    #[allow(clippy::declare_interior_mutable_const)]\n    const B: u8 = 2;\n    // --context-separator takes a value, --no-context-separator does not.\n    work();\n}\n",
        )]),
        CapabilityCase::new(
            "magic_literal_cluster",
            ScanProfile::Full,
            "src/lib.rs",
            "fn calculate() -> i32 { 42 + 7 + 100 + 365 }\n",
            "src/lib.rs",
            "fn calculate() -> i32 { 42 + 7 + 1 }\n",
        )
        .negative_files(vec![
            file(
                "src/lib.rs",
                "fn calculate() -> i32 { 42 + 7 + 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn positions() {\n        assert_eq!(super::calculate(), 50 + 0 * 17 + 0 * 23 + 0 * 99);\n    }\n}\n",
            ),
            file(
                "src/render.py",
                "def install(width=100, extra_lines=3, max_frames=120, locals_max=24):\n    return width\n",
            ),
        ]),
        CapabilityCase::new(
            "identifier_verbosity",
            ScanProfile::Gating,
            "src/service.py",
            "def process_customer_account_status_record():\n    customer_account_status_record = 1\n    customer_account_status_projection = customer_account_status_record\n    customer_account_status_validation = customer_account_status_projection\n    return customer_account_status_validation\n",
            "src/service.py",
            "def process(data):\n    value = data\n    result = value\n    return result\n",
        ),
        CapabilityCase::new(
            "identifier_tersity",
            ScanProfile::Full,
            "src/service.py",
            "def terse():\n    aa = bb = cc = dd = ee = ff = 3\n    return aa + bb + cc + dd + ee + ff\n",
            "src/service.py",
            "def clear():\n    alpha = beta = gamma = delta = epsilon = 3\n    return alpha + beta + gamma + delta + epsilon\n",
        )
        .negative_files(vec![
            file(
                "src/service.py",
                "def clear():\n    alpha = beta = gamma = delta = epsilon = 3\n    return alpha + beta + gamma + delta + epsilon\n",
            ),
            file(
                "src/files.py",
                "def names(files):\n    return [f.name for f in files if f.id and f.name]\n",
            ),
        ]),
        CapabilityCase::new(
            "stringly_typed_parameter",
            ScanProfile::Gating,
            "src/service.py",
            "def execute(mode: str):\n    if mode == 'fast':\n        return 1\n    if mode == 'safe':\n        return 2\n    return 0\n",
            "src/service.py",
            "def execute(mode: Mode):\n    return mode\n",
        )
        .negative_files(vec![
            file("src/model.ts", "function treeLines(states: Map<string, ModelState>): string[] { return [] }\nfunction close(statusMessage?: string): void {}\n"),
            file("src/service.py", "def execute(mode: Mode):\n    return mode\n"),
            file(
                "src/conversion.py",
                "def rust_variant_to_system_id(variant: str) -> str:\n    chars = []\n    for index, ch in enumerate(variant):\n        if ch.isupper() and index > 0:\n            chars.append('_')\n        chars.append(ch.lower())\n    return ''.join(chars)\n",
            ),
        ]),
        CapabilityCase::new(
            "function_local_import",
            ScanProfile::Gating,
            "src/service.py",
            "def execute():\n    import json\n    return json.dumps({})\n",
            "src/service.py",
            "import json\ndef execute():\n    return json.dumps({})\n",
        )
        .negative_files(vec![
            file("src/service.py", "import json\ndef execute():\n    return json.dumps({})\n"),
            file(
                "src/auth.py",
                "def credentials():\n    # Lazily import 'netrc' to improve import time.\n    import netrc\n    return netrc.netrc()\n",
            ),
            file(
            "src/guarded.py",
            "from typing import TYPE_CHECKING\n\ndef connect():\n    try:\n        import h2\n    except ImportError:\n        h2 = None\n    return h2\n\ndef annotate():\n    if TYPE_CHECKING:\n        from .models import Model\n    return None\n",
        ),
        ]),
        CapabilityCase::new(
            "sibling_call_redundancy",
            ScanProfile::Full,
            "src/service.py",
            "def first():\n    load(); validate(); persist()\ndef second():\n    load(); validate(); persist()\ndef load(): pass\ndef validate(): pass\ndef persist(): pass\n",
            "src/service.py",
            "def first():\n    load(); validate(); persist()\ndef second():\n    render(); notify(); archive()\n",
        ),
        CapabilityCase::new(
            "cognitive_complexity",
            ScanProfile::Gating,
            "src/lib.rs",
            nested_source(6),
            "src/lib.rs",
            nested_source(1),
        )
        .negative_files(vec![
            file("src/lib.rs", nested_source(1)),
            file("src/label.rs", flat_match_source(30)),
        ]),
        CapabilityCase::new(
            "npath_complexity",
            ScanProfile::Gating,
            "src/lib.rs",
            npath_source(8),
            "src/lib.rs",
            npath_source(2),
        )
        .negative_files(vec![
            file("src/lib.rs", npath_source(2)),
            file("src/guards.rs", guard_clause_source(12)),
        ]),
        CapabilityCase::new(
            "halstead_information_density",
            ScanProfile::Gating,
            "src/lib.rs",
            halstead_source(120),
            "src/lib.rs",
            "fn identity(value: usize) -> usize { value }\n",
        )
        .negative_files(vec![file("src/lib.rs", data_table_source(400))]),
        CapabilityCase::new(
            "identifier_stutter",
            ScanProfile::Full,
            "src/config.rs",
            "pub struct ConfigError;\n",
            "src/config.rs",
            "pub struct Error;\n",
        )
        .positive_files(vec![
            file("src/lib.rs", "pub mod config;\n"),
            file("src/config.rs", "pub struct ConfigError;\n"),
        ])
        .negative_files(vec![
            file("src/lib.rs", "pub mod config;\nmod sink;\npub use sink::SinkMatch;\n"),
            file("src/config.rs", "pub struct Error;\nstruct ConfigCache;\nimpl Error {\n    pub fn config_path() {}\n}\n"),
            file("src/sink.rs", "pub struct SinkMatch;\n"),
            file("src/collect.rs", "fn process_pdf_document() {\n    let pdf_document_bytes = 1;\n}\n"),
            file("chubby/file.go", "package chubby\n\ntype File struct{}\n"),
            file("src/emoji.py", "class NoEmoji(Exception):\n    pass\n"),
        ]),
        CapabilityCase::new(
            "escape_hatch_type_density",
            ScanProfile::Full,
            "src/service.py",
            "def convert(first: Any, second: Any, third: Any, fourth: int, fifth: int) -> Any:\n    return first\n",
            "src/service.py",
            "def convert(first: str, second: str, third: int, fourth: int, fifth: int) -> str:\n    return first\n",
        ),
        CapabilityCase::new(
            "collection_parameter_mutation",
            ScanProfile::Full,
            "src/service.py",
            "def add(items, value):\n    items.append(value)\n    return items\n",
            "src/service.py",
            "def add(items, value):\n    return items + [value]\n\nclass Visitor:\n    def visit(self, value):\n        self.stack.append(value)\n\ndef render(console, options):\n    narrowed = options.update(width=10)\n    return console.render(options.update(height=None), narrowed)\n",
        ),
        CapabilityCase::new(
            "out_parameter",
            ScanProfile::Gating,
            "src/Service.java",
            "class Service { void update(Box box) { box.value = 2; } }\n",
            "src/Service.java",
            "class Service { int update(int value) { return value + 2; } }\n",
        )
        .negative_files(vec![
            file("src/Service.java", "class Service { int update(int value) { return value + 2; } }\n"),
            file(
            "src/Rebind.java",
            "class Rebind { int update(int value) { value = value + 2; return value; } }\n",
        ),
            file(
                "src/Tree.java",
                "class Tree { static final class Node { Node left; } void rotate(Node root) { root.left = null; } }\n",
            ),
            file(
                "src/State.java",
                "enum State { DATA { void read(Machine m) { m.pending = true; } }; abstract void read(Machine m); }\n",
            ),
            file(
                "src/Utf32Reader.java",
                "class Utf32Reader extends java.io.Reader {\n    @Override\n    public int read(char[] cbuf, int start, int len) { cbuf[start] = 'a'; return 1; }\n}\n",
            ),
        ]),
        CapabilityCase::new(
            "excessive_nesting_depth",
            ScanProfile::Gating,
            "src/lib.rs",
            nested_source(6),
            "src/lib.rs",
            format!("{}\n{}", nested_source(5), flat_else_if_source()),
        ),
        CapabilityCase::new(
            "duplicate_import",
            ScanProfile::Gating,
            "src/service.js",
            "import { first } from './model.js';\nimport { second } from './model.js';\nexport function load() { return first + second; }\n",
            "src/service.js",
            "import { first } from './model.js';\nimport { second } from './other.js';\nexport function load() { return first + second; }\n",
        )
        .negative_files(vec![file(
            "src/service.ts",
            "import type { Shape } from './model.js';\nimport { build } from './model.js';\nimport * as util from './util.js';\nimport { helper } from './util.js';\nexport function load(shape: Shape) { return build(shape, util.id, helper); }\n",
        ), file(
            "src/legacy.js",
            "var methods = require('./utils').methods;\nvar compileETag = require('./utils').compileETag;\nmodule.exports = { methods, compileETag };\n",
        )]),
        CapabilityCase::new(
            "class_structure_metrics",
            ScanProfile::Gating,
            "src/service.py",
            method_source(41),
            "src/service.py",
            method_source(2),
        )
        .negative_files(vec![file(
            "src/errors.py",
            "class Base(Exception):\n    pass\n\nclass Transport(Base):\n    pass\n\nclass Timeout(Transport):\n    pass\n\nclass Network(Timeout):\n    pass\n\nclass Pool(Network):\n    pass\n\nclass ConnectTimeout(Pool):\n    pass\n",
        )]),
        CapabilityCase::new(
            "god_module",
            ScanProfile::Full,
            "src/lib.rs",
            function_source(21),
            "src/lib.rs",
            function_source(20),
        ),
        CapabilityCase::new(
            "source_duplicate_detection",
            ScanProfile::Gating,
            "src/module_0.rs",
            "",
            "src/module_0.rs",
            "",
        )
        .positive_files(duplicate_files(true))
        .negative_files(duplicate_files(false)),
        CapabilityCase::new(
            "architectural_layering",
            ScanProfile::Gating,
            "src/domain/user.rs",
            "use crate::app::UserService;\nfn check() {}\n",
            "src/domain/user.rs",
            "use crate::domain::User;\nfn check() {}\n",
        )
        .positive_files(vec![
            file(
                "src/domain/user.rs",
                "use crate::app::UserService;\nfn check() {}\n",
            ),
            file("src/app/mod.rs", "pub struct UserService;\n"),
            file("auditor.toml", layering_contract()),
        ])
        .negative_files(vec![
            file(
                "src/domain/user.rs",
                "use crate::domain::User;\nfn check() {}\n",
            ),
            file(
                "src/domain/mod.rs",
                "mod user;\npub use self::{user::check};\npub struct User;\n",
            ),
            file(
                "src/domain/unresolved.rs",
                "use crate::app::Missing;\nfn unresolved() {}\n",
            ),
            file("src/app/mod.rs", "pub struct UserService;\n"),
            file("auditor.toml", layering_contract()),
        ]),
        CapabilityCase::new(
            "package_coupling_metrics",
            ScanProfile::Gating,
            "crates/core/src/lib.rs",
            "pub struct Config;\n",
            "src/lib.rs",
            "pub fn hello() {}\n",
        )
        .positive_files(vec![
            file("crates/core/src/lib.rs", "pub struct Config;\n"),
            file(
                "crates/app/src/lib.rs",
                "use core::Config;\npub trait Processor { fn process(&self, cfg: &Config); }\n",
            ),
            file(
                "crates/web/src/lib.rs",
                "use core::Config;\npub trait Handler { fn handle(&self, cfg: &Config); }\n",
            ),
        ])
        .negative_files(vec![
            file("crates/core/src/lib.rs", "pub struct Config;\n"),
            file(
                "crates/app/src/lib.rs",
                "use core::Missing;\npub struct App;\n",
            ),
        ]),
        CapabilityCase::new(
            "loop_artifact_rewrite",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn rewrite(paths: Vec<PathBuf>) {\n    for path in paths {\n        let raw = std::fs::read_to_string(path).unwrap();\n        std::fs::write(\"report.json\", raw).unwrap();\n    }\n}\n",
            "src/lib.rs",
            "fn rewrite(paths: Vec<PathBuf>) {\n    let raw = std::fs::read_to_string(\"input\").unwrap();\n    for path in paths { consume(path, &raw); }\n}\n",
        )
        .negative_files(vec![file(
            "copy.py",
            "def copy_tree(src, dst):\n    for item in src.iterdir():\n        dest = dst / item.name\n        raw = item.read_bytes()\n        dest.write_bytes(raw)\n",
        )]),
        CapabilityCase::new(
            "latest_only_unbounded_cache",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn update_cache(cache: &mut Vec<Entry>, entry: Entry) {\n    cache.push(entry);\n    let latest = cache.last().unwrap();\n    consume(latest);\n}\n",
            "src/lib.rs",
            "fn update_cache(cache: &mut Vec<Entry>, entry: Entry) {\n    cache.push(entry);\n    cache.truncate(10);\n    let latest = cache.last().unwrap();\n    consume(latest);\n}\n",
        ),
        CapabilityCase::new(
            "repeated_parse_scan_serialize",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn process(items: Vec<String>) {\n    for item in items {\n        let parsed = serde_json::from_str(&item).unwrap();\n        scan(parsed);\n        let encoded = serde_json::to_string(&parsed).unwrap();\n        write(encoded);\n    }\n}\n",
            "src/lib.rs",
            "fn process(item: String) {\n    let parsed = serde_json::from_str(&item).unwrap();\n    scan(parsed);\n}\n",
        )
        .negative_files(vec![
            file(
                "src/parse_serialize.rs",
                "fn process(items: Vec<String>) {\n    for item in items {\n        let parsed = serde_json::from_str(&item).unwrap();\n        let encoded = serde_json::to_string(&parsed).unwrap();\n        write(encoded);\n    }\n}\n",
            ),
            file(
                "src/scan_only.rs",
                "fn process(item: String) {\n    let parsed = serde_json::from_str(&item).unwrap();\n    scan(parsed);\n}\n",
            ),
            file(
                "src/catalog.rs",
                "fn catalog(args: &[String]) -> String {\n    for arg in args {\n        let profile = ScanProfile::parse(arg).unwrap();\n        let message = profile.to_string();\n        consume(message);\n    }\n    serde_json::to_string(args).unwrap()\n}\n",
            ),
        ]),
        CapabilityCase::new(
            "blocking_io_in_async_context",
            ScanProfile::Full,
            "src/lib.rs",
            "async fn load() {\n    let raw = std::fs::read_to_string(\"input.txt\").unwrap();\n    consume(raw);\n}\n",
            "src/lib.rs",
            "fn load() {\n    let raw = std::fs::read_to_string(\"input.txt\").unwrap();\n    consume(raw);\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn load() {\n    let raw = std::fs::read_to_string(\"input.txt\").unwrap();\n    consume(raw);\n}\n"),
            file(
            "scripts/release.rs",
            "async fn load() {\n    let raw = std::fs::read_to_string(\"input.txt\").unwrap();\n    consume(raw);\n}\n",
        ),
        ]),
        CapabilityCase::new(
            "per_item_external_io",
            ScanProfile::Full,
            "src/lib.rs",
            "fn fetch_all(ids: Vec<u64>, client: Client) {\n    for id in ids { consume(client.get(id)); }\n}\n",
            "src/lib.rs",
            "fn fetch_all(ids: Vec<u64>, client: Client) {\n    let response = client.get(ids);\n    for item in response { consume(item); }\n}\nfn inspect_locals(items: Vec<u64>, retrieval_requests: Requests, out: Vec<u8>) {\n    for item in items {\n        reserve_http_worker(item);\n        consume(retrieval_requests.values());\n        consume(out.windows(4).any(|window| !window.is_empty()));\n        consume(item.in_flight_requests.len());\n    }\n}\n",
        ),
        CapabilityCase::new(
            "repeated_expensive_initialization",
            ScanProfile::Full,
            "src/lib.rs",
            "fn validate(items: Vec<String>) {\n    for item in items {\n        let parser = Regex::new(\"[a-z]+\").unwrap();\n        consume(parser, item);\n    }\n}\n",
            "src/lib.rs",
            "fn validate(items: Vec<String>) {\n    let parser = Regex::new(\"[a-z]+\").unwrap();\n    for item in items { consume(&parser, item); }\n}\nfn load_once(layout: Layout) {\n    for item in ProviderRuntimeEventStore::new(layout).list_events() { consume(item); }\n}\nfn per_pattern(patterns: Vec<String>) {\n    for pattern in patterns { consume(Regex::new(&pattern).unwrap()); }\n}\n",
        ),
        CapabilityCase::new(
            "request_path_repository_scan",
            ScanProfile::Full,
            "src/lib.rs",
            "fn handle_request() {\n    let entries = std::fs::read_dir(\".\").unwrap();\n    consume(entries);\n}\n",
            "src/lib.rs",
            "fn refresh_index() { consume(std::fs::read_dir(\".\").unwrap()); }\nfn latest_report() { consume(std::fs::read_dir(\".\").unwrap()); }\nfn latest_route_chain_id() { consume(std::fs::read_dir(\".\").unwrap()); }\nfn server_metadata_entries() { consume(std::fs::read_dir(\".\").unwrap()); }\n",
        ),
        CapabilityCase::new(
            "churn_complexity_hotspot",
            ScanProfile::Gating,
            "src/lib.rs",
            churn_source(),
            "src/lib.rs",
            churn_source(),
        )
        .positive_setup(add_churn),
        CapabilityCase::new(
            "too_many_locals",
            ScanProfile::Full,
            "src/lib.rs",
            local_source(16),
            "src/lib.rs",
            local_source(15),
        ),
        CapabilityCase::new(
            "too_many_returns",
            ScanProfile::Gating,
            "src/lib.rs",
            return_source(6),
            "src/lib.rs",
            return_source(5),
        ),
        CapabilityCase::new(
            "too_many_statements",
            ScanProfile::Gating,
            "src/lib.rs",
            statement_source(51),
            "src/lib.rs",
            statement_source(50),
        ),
        CapabilityCase::new(
            "naming_convention",
            ScanProfile::Gating,
            "src/User.java",
            "public class user {}\n",
            "src/User.java",
            "public class User {}\n",
        )
        .negative_files(vec![file(
            "src/User.java",
            "public class User implements java.io.Serializable {\n    private static final long serialVersionUID = 1L;\n    private static final ConcurrentMap<String, Method> methodCache =\n        new MapMaker().makeMap();\n    private static final String[] defaultNames =\n        {\"a\"};\n}\n",
        )]),
    ]
}

fn layering_contract() -> String {
    "[layering.layer.domain]\nglob = src/domain/**/*.rs\n\n[layering.layer.app]\nglob = src/app/**/*.rs\n\n[layering.rule.domain]\ndeny = app\n"
        .to_string()
}

#[cfg(feature = "graph-analysis")]
pub(super) fn feature_cases() -> Vec<CapabilityCase> {
    vec![
        CapabilityCase::new(
            "tooling_overlap",
            ScanProfile::Gating,
            "tooling/run-a-first.sh",
            "#!/bin/sh\necho first\n",
            "tooling/run-a.sh",
            "#!/bin/sh\necho first\n",
        )
        .positive_files(vec![
            file(
                "tooling/run-a-first.sh",
                "#!/bin/sh\necho first\n",
            ),
            file(
                "tooling/run-a-second.sh",
                "#!/bin/sh\necho second\n",
            ),
        ])
        .negative_files(vec![
            file("tooling/run-a.sh", "#!/bin/sh\necho first\n"),
            file("tooling/run-b.sh", "#!/bin/sh\necho second\n"),
        ]),
        CapabilityCase::new("repo_graph_isolated_source_file", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .repo_graphs(r#"{"nodes":[{"path":"src/solo.rs","node_kind":"file","line_count":25}],"edges":[]}"#, r#"{"nodes":[{"path":"src/a.rs","node_kind":"file","line_count":25},{"path":"src/b.rs","node_kind":"file","line_count":5}],"edges":[{"from":"src/a.rs","to":"src/b.rs","edge_kind":"imports"}]}"#),
        CapabilityCase::new("repo_graph_high_fanout_source_file", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .repo_graphs(r#"{"nodes":[{"path":"hub.rs","node_kind":"file","imports":["d0.rs","d1.rs","d2.rs","d3.rs","d4.rs","d5.rs","d6.rs","d7.rs","d8.rs","d9.rs","d10.rs","d11.rs"]},{"path":"d0.rs","node_kind":"file"},{"path":"d1.rs","node_kind":"file"},{"path":"d2.rs","node_kind":"file"},{"path":"d3.rs","node_kind":"file"},{"path":"d4.rs","node_kind":"file"},{"path":"d5.rs","node_kind":"file"},{"path":"d6.rs","node_kind":"file"},{"path":"d7.rs","node_kind":"file"},{"path":"d8.rs","node_kind":"file"},{"path":"d9.rs","node_kind":"file"},{"path":"d10.rs","node_kind":"file"},{"path":"d11.rs","node_kind":"file"}],"edges":[]}"#, r#"{"nodes":[{"path":"hub.rs","node_kind":"file","imports":[]}],"edges":[]}"#),
        CapabilityCase::new("repo_graph_import_cycle", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .repo_graphs(r#"{"nodes":[{"path":"a.rs","node_kind":"file"},{"path":"b.rs","node_kind":"file"}],"edges":[{"from":"a.rs","to":"b.rs","edge_kind":"imports"},{"from":"b.rs","to":"a.rs","edge_kind":"imports"}]}"#, r#"{"nodes":[{"path":"a.rs","node_kind":"file"},{"path":"b.rs","node_kind":"file"}],"edges":[{"from":"a.rs","to":"b.rs","edge_kind":"imports"}]}"#),
        CapabilityCase::new("large_file_split_candidate", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .repo_graphs(r#"{"nodes":[{"path":"large.rs","node_kind":"file","line_count":1000},{"path":"a1","node_kind":"symbol","file":"large.rs"},{"path":"a2","node_kind":"symbol","file":"large.rs"},{"path":"a3","node_kind":"symbol","file":"large.rs"},{"path":"b1","node_kind":"symbol","file":"large.rs"},{"path":"b2","node_kind":"symbol","file":"large.rs"},{"path":"b3","node_kind":"symbol","file":"large.rs"}],"edges":[{"from":"a1","to":"a2","edge_kind":"references"},{"from":"a2","to":"a3","edge_kind":"references"},{"from":"b1","to":"b2","edge_kind":"references"},{"from":"b2","to":"b3","edge_kind":"references"}]}"#, r#"{"nodes":[{"path":"large.rs","node_kind":"file","line_count":999}],"edges":[]}"#),
        CapabilityCase::new("small_file_consolidation_candidate", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .repo_graphs(r#"{"nodes":[{"path":"owner.rs","node_kind":"file","line_count":200},{"path":"helper.rs","node_kind":"file","line_count":50}],"edges":[{"from":"owner.rs","to":"helper.rs","edge_kind":"imports"}]}"#, r#"{"nodes":[{"path":"lib.rs","node_kind":"file","line_count":50},{"path":"owner.rs","node_kind":"file","line_count":200}],"edges":[{"from":"owner.rs","to":"lib.rs","edge_kind":"imports"}]}"#),
        CapabilityCase::new("shotgun_surgery_cluster", ScanProfile::Gating, "src/lib.rs", "fn live() {}\n", "src/lib.rs", "fn live() {}\n")
            .co_edit_journals(r#"{"records":[{"change_set_id":"1","files":["a.rs","b.rs","c.rs"]},{"change_set_id":"2","files":["a.rs","b.rs","c.rs"]},{"change_set_id":"3","files":["a.rs","b.rs","c.rs"]}]}"#, r#"{"records":[{"change_set_id":"1","files":["a.rs","b.rs","c.rs"]},{"change_set_id":"2","files":["a.rs","b.rs","c.rs"]}]}"#),
    ]
}

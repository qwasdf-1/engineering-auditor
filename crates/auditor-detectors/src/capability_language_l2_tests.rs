//! Native L2 capability boundaries for the structural and lexical detectors
//! qualified across the Tier-1 and Tier-2 languages. Each negative is the nearest
//! conforming form of its positive.

pub(super) fn l2_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let source = match detector {
        "section_divider_comment" => divided_function(language, if positive { 3 } else { 2 }),
        "magic_literal_cluster" => literal_function(
            language,
            if positive {
                "42 + 7 + 100 + 365"
            } else {
                "42 + 7 + 1"
            },
        ),
        "identifier_verbosity" => verbose_function(language, positive)?,
        "identifier_tersity" => terse_function(language, positive)?,
        "sibling_call_redundancy" => sibling_functions(language, positive)?,
        "class_structure_metrics" => class_with_methods(language, if positive { 41 } else { 2 })?,
        "source_duplicate_detection" => return duplicate_sources(language, positive),
        other => return shaped_source(other, language, positive),
    };
    Some(vec![(
        format!("src/capability.{}", extension(language)),
        source,
    )])
}

fn extension(language: &str) -> &'static str {
    match language {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "go" => "go",
        "java" => "java",
        "kotlin" => "kt",
        "csharp" => "cs",
        "c" => "c",
        "cpp" => "cpp",
        other => panic!("not a Tier-1 or Tier-2 language: {other}"),
    }
}

/// A function named `name` whose body is `body` (statements, one per line).
fn function(language: &str, name: &str, body: &[String]) -> String {
    let indent = |prefix: &str| {
        body.iter()
            .map(|line| format!("{prefix}{line}\n"))
            .collect::<String>()
    };
    match language {
        "rust" => format!("pub fn {name}() {{\n{}}}\n", indent("    ")),
        "python" => format!("def {name}():\n{}", indent("    ")),
        "javascript" => format!("export function {name}() {{\n{}}}\n", indent("  ")),
        "typescript" => format!("export function {name}(): void {{\n{}}}\n", indent("  ")),
        "go" => format!(
            "package capability\n\nfunc {name}() {{\n{}}}\n",
            indent("\t")
        ),
        "kotlin" => format!("fun {name}() {{\n{}}}\n", indent("    ")),
        "csharp" => format!(
            "class Capability {{\n    public void {}() {{\n{}    }}\n}}\n",
            name[..1].to_uppercase() + &name[1..],
            indent("        ")
        ),
        "c" => format!("void {name}(void) {{\n{}}}\n", indent("    ")),
        "cpp" => format!("void {name}() {{\n{}}}\n", indent("    ")),
        _ => format!(
            "class Capability {{\n    void {name}() {{\n{}    }}\n}}\n",
            indent("        ")
        ),
    }
}

fn statement(language: &str, text: &str) -> String {
    match language {
        "python" | "go" | "kotlin" => text.to_string(),
        _ => format!("{text};"),
    }
}

fn divided_function(language: &str, dividers: usize) -> String {
    let marker = if language == "python" {
        "# ===="
    } else {
        "// ===="
    };
    let mut body = vec![marker.to_string()];
    for _ in 1..dividers {
        body.push(statement(language, "work()"));
        body.push(marker.to_string());
    }
    function(language, "execute", &body)
}

fn literal_function(language: &str, expression: &str) -> String {
    let consume = format!("consume({expression})");
    function(language, "calculate", &[statement(language, &consume)])
}

/// Locals whose names average more than three words.
fn verbose_function(language: &str, positive: bool) -> Option<String> {
    if !matches!(
        language,
        "rust" | "python" | "javascript" | "typescript" | "go" | "java"
    ) {
        return None;
    }
    let names: [&str; 3] = if positive {
        [
            "customer_account_status_record_entry",
            "customer_account_status_projection_entry",
            "customer_account_status_validation_entry",
        ]
    } else {
        ["record", "projection", "validation"]
    };
    let camel = |name: &str| {
        let mut parts = name.split('_');
        let first = parts.next().unwrap_or_default().to_string();
        first
            + &parts
                .map(|part| part[..1].to_uppercase() + &part[1..])
                .collect::<String>()
    };
    let names = names.map(|name| {
        if matches!(language, "rust" | "python") {
            name.to_string()
        } else {
            camel(name)
        }
    });
    let declare = |target: &str, value: &str| match language {
        "rust" => format!("let {target} = {value};"),
        "python" => format!("{target} = {value}"),
        "javascript" | "typescript" => format!("const {target} = {value};"),
        "go" => format!("{target} := {value}"),
        _ => format!("int {target} = {value};"),
    };
    let body = vec![
        declare(&names[0], "1"),
        declare(&names[1], &names[0]),
        declare(&names[2], &names[1]),
        statement(language, &format!("consume({})", names[2])),
    ];
    Some(function(language, "process", &body))
}

/// Six two-letter locals, or five readable ones.
fn terse_function(language: &str, positive: bool) -> Option<String> {
    let names: &[&str] = if positive {
        &["aa", "bb", "cc", "dd", "ee", "ff"]
    } else {
        &["alpha", "beta", "gamma", "delta", "epsilon"]
    };
    let mut body = names
        .iter()
        .map(|name| match language {
            "rust" => format!("let {name} = 3;"),
            "python" => format!("{name} = 3"),
            "javascript" | "typescript" => format!("const {name} = 3;"),
            "go" => format!("{name} := 3"),
            _ => format!("int {name} = 3;"),
        })
        .collect::<Vec<_>>();
    body.push(statement(
        language,
        &format!("consume({})", names.join(" + ")),
    ));
    Some(function(language, "compute", &body))
}

/// Two functions running the same three-call sequence, or different ones.
fn sibling_functions(language: &str, positive: bool) -> Option<String> {
    let calls = |names: [&str; 3]| {
        names
            .iter()
            .map(|name| statement(language, &format!("{name}()")))
            .collect::<Vec<_>>()
    };
    let second = if positive {
        ["load", "validate", "persist"]
    } else {
        ["render", "notify", "archive"]
    };
    let first = function(language, "first", &calls(["load", "validate", "persist"]));
    let second = function(language, "second", &calls(second));
    let stubs = ["load", "validate", "persist", "render", "notify", "archive"]
        .iter()
        .map(|name| {
            let body = if language == "python" {
                vec!["pass".to_string()]
            } else {
                Vec::new()
            };
            function(language, name, &body)
        })
        .collect::<Vec<_>>();
    let strip = |source: String| {
        source
            .trim_start_matches("package capability\n\n")
            .to_string()
    };
    let second = std::iter::once(second)
        .chain(stubs)
        .map(|source| {
            if language == "go" {
                strip(source)
            } else {
                source
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(match language {
        "go" => format!(
            "{first}\n{}",
            second.trim_start_matches("package capability\n\n")
        ),
        _ => format!("{first}\n{second}"),
    })
}

fn class_with_methods(language: &str, count: usize) -> Option<String> {
    let methods = (0..count)
        .map(|index| match language {
            "javascript" => format!("  operation{index}() {{ return {index}; }}\n"),
            "typescript" => format!("  operation{index}(): number {{ return {index}; }}\n"),
            "java" => format!("    int operation{index}() {{ return {index}; }}\n"),
            "kotlin" => format!("    fun operation{index}(): Int = {index}\n"),
            "csharp" => format!("    public int Operation{index}() {{ return {index}; }}\n"),
            "cpp" => format!("    int operation{index}() {{ return {index}; }}\n"),
            _ => String::new(),
        })
        .collect::<String>();
    Some(match language {
        "javascript" | "typescript" => format!("export class Service {{\n{methods}}}\n"),
        "java" | "kotlin" | "csharp" => format!("class Service {{\n{methods}}}\n"),
        "cpp" => format!("class Service {{\npublic:\n{methods}}};\n"),
        _ => return None,
    })
}

/// Five files sharing one accumulate-and-branch function, or five distinct
/// one-liners (the Rust fixture's shape in each language).
fn duplicate_sources(language: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let files = (0..5)
        .map(|index| -> Option<(String, String)> {
            let source = if positive {
                accumulating_function(language, index)?
            } else {
                function(
                    language,
                    &format!("label{index}"),
                    &[statement(language, &format!("consume({index})"))],
                )
            };
            Some((
                format!("src/module_{index}.{}", extension(language)),
                source,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(files)
}

fn accumulating_function(language: &str, index: usize) -> Option<String> {
    Some(match language {
        "javascript" | "typescript" => format!(
            "export function process{index}(input) {{\n  let total{index} = 0;\n  let count{index} = 0;\n  for (const item of input) {{\n    const scaled = item * 2;\n    total{index} = accumulate(total{index}, scaled);\n    count{index} = accumulate(count{index}, item);\n  }}\n  const result = total{index} + count{index};\n  if (result > 100) {{\n    return result;\n  }}\n  return result;\n}}\n"
        ),
        "go" => format!(
            "package capability\n\nfunc process{index}(input []int) int {{\n\ttotal{index} := 0\n\tcount{index} := 0\n\tfor _, item := range input {{\n\t\tscaled := item * 2\n\t\ttotal{index} = accumulate(total{index}, scaled)\n\t\tcount{index} = accumulate(count{index}, item)\n\t}}\n\tresult := total{index} + count{index}\n\tif result > 100 {{\n\t\treturn result\n\t}}\n\treturn result\n}}\n"
        ),
        "java" => format!(
            "class Module{index} {{\n    int process{index}(int[] input) {{\n        int total{index} = 0;\n        int count{index} = 0;\n        for (int item : input) {{\n            int scaled = item * 2;\n            total{index} = accumulate(total{index}, scaled);\n            count{index} = accumulate(count{index}, item);\n        }}\n        int result = total{index} + count{index};\n        if (result > 100) {{\n            return result;\n        }}\n        return result;\n    }}\n}}\n"
        ),
        "kotlin" => format!(
            "fun process{index}(input: List<Int>): Int {{\n    var total{index} = 0\n    var count{index} = 0\n    for (item in input) {{\n        val scaled = item * 2\n        total{index} = accumulate(total{index}, scaled)\n        count{index} = accumulate(count{index}, item)\n    }}\n    val result = total{index} + count{index}\n    if (result > 100) {{\n        return result\n    }}\n    return result\n}}\n"
        ),
        "csharp" => format!(
            "class Module{index} {{\n    public int Process{index}(int[] input) {{\n        int total{index} = 0;\n        int count{index} = 0;\n        foreach (int item in input) {{\n            int scaled = item * 2;\n            total{index} = Accumulate(total{index}, scaled);\n            count{index} = Accumulate(count{index}, item);\n        }}\n        int result = total{index} + count{index};\n        if (result > 100) {{\n            return result;\n        }}\n        return result;\n    }}\n}}\n"
        ),
        "c" | "cpp" => format!(
            "int process{index}(const int *input, int length) {{\n    int total{index} = 0;\n    int count{index} = 0;\n    for (int index = 0; index < length; index++) {{\n        int scaled = input[index] * 2;\n        total{index} = accumulate(total{index}, scaled);\n        count{index} = accumulate(count{index}, input[index]);\n    }}\n    int result = total{index} + count{index};\n    if (result > 100) {{\n        return result;\n    }}\n    return result;\n}}\n"
        ),
        _ => return None,
    })
}

/// Hand-written boundaries for detectors whose shape is language-specific.
fn shaped_source(detector: &str, language: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let (positive_source, negative_source): (&str, &str) = match (detector, language) {
        ("function_local_import", "javascript" | "typescript") => (
            "export function load(path) {\n  const fs = require('fs');\n  return fs.readFileSync(path);\n}\n",
            "const fs = require('fs');\n\nexport function load(path) {\n  return fs.readFileSync(path);\n}\n",
        ),
        ("escape_hatch_type_density", "typescript") => (
            "export function convert(first: any, second: any, third: any, fourth: number, fifth: number): any {\n  return first;\n}\n",
            "export function convert(first: string, second: string, third: number, fourth: number, fifth: number): string {\n  return first;\n}\n",
        ),
        // A typed collection parameter; a recursive walk threading the same
        // array is the adversarial negative.
        ("collection_parameter_mutation", "typescript") => (
            "export function add(items: number[], value: number) {\n  items.push(value);\n  return items;\n}\n",
            "export function add(items: number[], value: number): number[] {\n  if (value > 0) {\n    items.push(value);\n    add(items, value - 1);\n  }\n  return items;\n}\n",
        ),
        // A public method mutating a `List` parameter; appending to a
        // `StringBuilder` sink is the adversarial negative.
        ("collection_parameter_mutation", "java") => (
            "class Service {\n    public void add(java.util.List<Integer> items, int value) {\n        items.add(value);\n    }\n}\n",
            "class Service {\n    public void add(StringBuilder items, int value) {\n        items.append(value);\n    }\n}\n",
        ),
        ("duplicate_import", "python") => (
            "import json\nimport json\n\ndef load(text):\n    return json.loads(text)\n",
            "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from json import JSONDecoder\n\ndef load(text):\n    return text\n\nif __name__ == \"__main__\":\n    from json import JSONDecoder\n    print(JSONDecoder().decode(\"1\"))\n",
        ),
        // A Kotlin-mutable or C# generic collection parameter; appending to a
        // `StringBuilder` sink, or a C# extension method acting on its `this`
        // receiver, is the adversarial negative.
        ("collection_parameter_mutation", "kotlin") => (
            "fun add(items: MutableList<Int>, value: Int) {\n    items.add(value)\n}\n",
            "fun add(items: StringBuilder, value: Int) {\n    items.append(value)\n}\n",
        ),
        ("collection_parameter_mutation", "csharp") => (
            "class Service {\n    public void Add(List<int> items, int value) {\n        items.Add(value);\n    }\n}\n",
            "static class Service {\n    public static void Add(StringBuilder items, int value) {\n        items.Append(value);\n    }\n\n    public static void Push(this List<int> items, int value) {\n        items.Add(value);\n    }\n}\n",
        ),
        // The same header in mutually exclusive preprocessor branches is the
        // adversarial negative.
        ("duplicate_import", "c" | "cpp") => (
            "#include <stdio.h>\n#include <stdio.h>\n\nvoid greet(void) {\n    puts(\"hi\");\n}\n",
            "#ifdef _WIN32\n#include \"config_win.h\"\n#include <stdio.h>\n#else\n#include <stdio.h>\n#endif\n\nvoid greet(void) {\n    puts(\"hi\");\n}\n",
        ),
        ("duplicate_import", "kotlin") => (
            "import java.util.UUID\nimport java.util.UUID\n\nfun id(): UUID = UUID.randomUUID()\n",
            "import java.util.UUID\nimport kotlin.uuid.Uuid\n\nfun id(): UUID = UUID.randomUUID()\n",
        ),
        ("duplicate_import", "csharp") => (
            "using System;\nusing System;\n\nclass Service {\n    public Guid Id() { return Guid.NewGuid(); }\n}\n",
            "using System;\nusing System.Text;\n\nclass Service {\n    public Guid Id() { return Guid.NewGuid(); }\n}\n",
        ),
        // A snake_case function; an `external` binding keeping its native
        // name is the adversarial negative.
        ("naming_convention", "kotlin") => (
            "fun compute_total(values: List<Int>): Int = values.size\n",
            "fun computeTotal(values: List<Int>): Int = values.size\n\nexternal fun native_total(): Int\n",
        ),
        // An interface without the `I` prefix and a camelCase method; an
        // `IPascalCase` interface and PascalCase method are the negative.
        ("naming_convention", "csharp") => (
            "public interface Handler {}\n\npublic class Service {\n    public int computeTotal() { return 0; }\n}\n",
            "public interface IHandler {}\n\npublic class Service {\n    public int ComputeTotal() { return 0; }\n}\n",
        ),
        ("duplicate_import", "java") => (
            "import java.util.List;\nimport java.util.List;\n\nclass Service {\n    List<String> names;\n}\n",
            "import java.util.List;\nimport java.util.Map;\n\nclass Service {\n    List<String> names;\n    Map<String, String> index;\n}\n",
        ),
        ("naming_convention", "go") => (
            "package capability\n\nfunc compute_total(values []int) int {\n\treturn len(values)\n}\n",
            "package capability\n\nfunc computeTotal(values []int) int {\n\treturn len(values)\n}\n",
        ),
        ("naming_convention", "javascript" | "typescript") => (
            "export function compute_total(values) {\n  return values.length;\n}\n\nexport class request_handler {}\n",
            "export function computeTotal(values) {\n  return values.length;\n}\n\nexport function Button() {\n  return null;\n}\n\nexport class RequestHandler {\n  _privateHelper() {}\n}\n",
        ),
        ("stringly_typed_parameter", "javascript") => (
            "/** @param {string} mode */\nexport function execute(mode) {\n  if (mode === 'fast') return 1;\n  if (mode === 'safe') return 2;\n  return 0;\n}\n",
            "/** @param {Mode} mode */\nexport function execute(mode) {\n  return mode.speed;\n}\n",
        ),
        ("stringly_typed_parameter", "typescript") => (
            "export function execute(mode: string): number {\n  if (mode === 'fast') return 1;\n  if (mode === 'safe') return 2;\n  return 0;\n}\n",
            "export function execute(mode: 'fast' | 'safe'): number {\n  return mode === 'fast' ? 1 : 2;\n}\n",
        ),
        ("stringly_typed_parameter", "java") => (
            "class Capability {\n    int execute(String mode) {\n        if (mode.equals(\"fast\")) return 1;\n        if (mode.equals(\"safe\")) return 2;\n        return 0;\n    }\n}\n",
            "class Capability {\n    enum Mode { FAST, SAFE }\n\n    int execute(Mode mode) {\n        return mode == Mode.FAST ? 1 : 2;\n    }\n}\n",
        ),
        ("range_for_each_iteration", "java") => (
            "class Capability {\n    void emit(int limit) {\n        java.util.stream.IntStream.range(0, limit).forEach(value -> consume(value));\n    }\n}\n",
            "class Capability {\n    void emit(int limit, java.util.List<Integer> items) {\n        for (int value = 0; value < limit; value++) {\n            consume(value);\n        }\n        items.forEach(value -> consume(value));\n    }\n}\n",
        ),
        _ => return None,
    };
    let source = if positive {
        positive_source
    } else {
        negative_source
    };
    Some(vec![(
        format!("src/capability.{}", extension(language)),
        source.to_string(),
    )])
}

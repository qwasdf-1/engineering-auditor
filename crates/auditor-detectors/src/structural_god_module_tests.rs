use super::*;

fn reports_god_module(name: &str, path: &str, source: &str) -> bool {
    let root = temp_root(name);
    write(&root, path, source);
    analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full)
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.detector == "god_module" && finding.primary_path() == path)
}

/// `count` functions named `f{index}`, one per C++ type `T{index}`.
fn cpp_overloads(name: &str, scope_prefix: &str, count: usize) -> String {
    (0..count)
        .map(|index| format!("{scope_prefix}void {name}(const T{index}& value) {{}}\n"))
        .collect()
}

fn cpp_distinct(prefix: &str, count: usize) -> String {
    (0..count)
        .map(|index| format!("void {prefix}{index}() {{}}\n"))
        .collect()
}

#[test]
fn cpp_overloads_of_one_function_count_once() {
    // One customisation point overloaded per type, plus two overloaded helpers.
    let source = format!(
        "namespace n {{\n{}{}{}}}\n",
        cpp_overloads("to_value", "template <class J> ", 21),
        cpp_overloads("helper", "", 1),
        "template <class J> void helper(J& j, int index) {}\n",
    );
    assert!(!reports_god_module(
        "god-cpp-overloads",
        "src/convert.hpp",
        &source
    ));
}

#[test]
fn cpp_templates_and_specialisations_of_one_name_count_once() {
    let specialisations = (0..22)
        .map(|index| format!("template <> void render<T{index}>(T{index} value) {{}}\n"))
        .collect::<String>();
    let source = format!("template <class T> void render(T value) {{}}\n{specialisations}");
    assert!(!reports_god_module(
        "god-cpp-specialised",
        "src/render.cpp",
        &source
    ));
}

#[test]
fn cpp_file_with_more_distinct_names_than_the_threshold_is_reported() {
    let source = cpp_distinct("operation_", 21);
    assert!(reports_god_module(
        "god-cpp-distinct",
        "src/ops.cpp",
        &source
    ));
}

#[test]
fn cpp_distinct_names_still_exceed_the_threshold_after_collapsing_overloads() {
    // 21 names, three overloads each: 63 definitions, 21 behaviours.
    let source = (0..21)
        .map(|index| cpp_overloads(&format!("op_{index}"), "", 3))
        .collect::<String>();
    assert!(reports_god_module(
        "god-cpp-collapsed",
        "src/ops.cpp",
        &source
    ));
    // 20 names with many overloads each stay within the threshold.
    let source = (0..20)
        .map(|index| cpp_overloads(&format!("op_{index}"), "", 3))
        .collect::<String>();
    assert!(!reports_god_module(
        "god-cpp-collapsed-ok",
        "src/ops.cpp",
        &source
    ));
}

#[test]
fn cpp_same_name_in_two_namespaces_counts_twice() {
    let names = cpp_distinct("shared_", 11);
    let two = format!("namespace a {{\n{names}}}\nnamespace b {{\n{names}}}\n");
    assert!(reports_god_module(
        "god-cpp-two-namespaces",
        "src/ns.cpp",
        &two
    ));
    // The same eleven names declared twice in one namespace are one set each.
    let one = format!("namespace a {{\n{names}}}\nnamespace a {{\n{names}}}\n");
    assert!(!reports_god_module(
        "god-cpp-one-namespace",
        "src/ns.cpp",
        &one
    ));
}

#[test]
fn cpp_distinct_operators_are_distinct_definitions() {
    let operators = ["==", "<", ">"]
        .iter()
        .map(|symbol| format!("bool operator{symbol}(A left, A right) {{ return true; }}\n"))
        .collect::<String>();
    let source = format!("{}{operators}", cpp_distinct("operation_", 18));
    assert!(reports_god_module(
        "god-cpp-operators",
        "src/ops.cpp",
        &source
    ));
}

#[test]
fn kotlin_overloads_of_one_function_count_once() {
    let overloads = (0..24)
        .map(|index| format!("fun create(value: Type{index}): Any = value\n"))
        .collect::<String>();
    assert!(!reports_god_module(
        "god-kotlin-overloads",
        "src/main/kotlin/Create.kt",
        &overloads
    ));
    let distinct = (0..21)
        .map(|index| format!("fun create{index}(value: Int): Any = value\n"))
        .collect::<String>();
    assert!(reports_god_module(
        "god-kotlin-distinct",
        "src/main/kotlin/Create.kt",
        &distinct
    ));
}

#[test]
fn python_overload_stubs_count_with_their_implementation() {
    let overloaded = "from typing import overload\n\n\
        @overload\ndef load(value: int) -> int: ...\n\n\
        @overload\ndef load(value: str) -> str: ...\n\n\
        def load(value):\n    return value\n\n";
    let distinct = (0..19)
        .map(|index| format!("def op_{index}():\n    pass\n\n"))
        .collect::<String>();
    assert!(!reports_god_module(
        "god-python-overloads",
        "src/ops.py",
        &format!("{overloaded}{distinct}")
    ));
    let distinct = (0..20)
        .map(|index| format!("def op_{index}():\n    pass\n\n"))
        .collect::<String>();
    assert!(reports_god_module(
        "god-python-overloads-over",
        "src/ops.py",
        &format!("{overloaded}{distinct}")
    ));
}

#[test]
fn typescript_overload_signatures_count_with_their_implementation() {
    let overloaded = "export function pick(value: string): string;\n\
        export function pick(value: number): number;\n\
        export function pick(value: unknown): unknown { return value; }\n";
    let distinct = (0..19)
        .map(|index| format!("export function op{index}(): void {{}}\n"))
        .collect::<String>();
    assert!(!reports_god_module(
        "god-ts-overloads",
        "src/ops.ts",
        &format!("{overloaded}{distinct}")
    ));
    let distinct = (0..20)
        .map(|index| format!("export function op{index}(): void {{}}\n"))
        .collect::<String>();
    assert!(reports_god_module(
        "god-ts-overloads-over",
        "src/ops.ts",
        &format!("{overloaded}{distinct}")
    ));
}

#[test]
fn typescript_same_name_in_two_namespaces_counts_twice() {
    let names = (0..11)
        .map(|index| format!("  export function shared{index}(): void {{}}\n"))
        .collect::<String>();
    let source = format!("namespace a {{\n{names}}}\nnamespace b {{\n{names}}}\n");
    assert!(reports_god_module(
        "god-ts-namespaces",
        "src/ns.ts",
        &source
    ));
}

#[test]
fn rust_same_name_in_two_modules_counts_twice() {
    let names = (0..11)
        .map(|index| format!("    pub fn shared_{index}() {{}}\n"))
        .collect::<String>();
    let source = format!("pub mod a {{\n{names}}}\npub mod b {{\n{names}}}\n");
    assert!(reports_god_module(
        "god-rust-modules",
        "src/lib.rs",
        &source
    ));
}

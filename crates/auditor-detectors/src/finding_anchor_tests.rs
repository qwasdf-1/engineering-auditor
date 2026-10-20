use super::*;

fn analyze(name: &str, files: &[(&str, &str)]) -> Vec<auditor_core::Finding> {
    let root = temp_root(name);
    for (path, source) in files {
        write(&root, path, source);
    }
    analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full)
        .unwrap()
        .findings
}

fn anchors_for<'a>(findings: &'a [auditor_core::Finding], detector: &str) -> Vec<&'a str> {
    let mut anchors = findings
        .iter()
        .filter(|finding| finding.detector == detector)
        .filter_map(auditor_core::Finding::anchor)
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    anchors
}

#[test]
fn distinct_same_file_findings_survive_owner_deduplication() {
    let findings = analyze(
        "anchor-dedup",
        &[(
            "src/service.py",
            "def add(items, value):\n    items.append(value)\n    return items\n\n\
             def extend(rows, value):\n    rows.append(value)\n    return rows\n",
        )],
    );
    assert_eq!(
        anchors_for(&findings, "collection_parameter_mutation"),
        ["symbol:add", "symbol:extend"]
    );
}

#[test]
fn typescript_private_helpers_do_not_report_collection_mutation() {
    let findings = analyze(
        "ts-private-mutation",
        &[(
            "src/paths.ts",
            "const handleParent = (paths: string[]): void => { paths.pop() }\n\
             export function add(items: number[], value: number): void { items.push(value) }\n\
             export function outer(): void {\n  function inner(rows: number[]): void { rows.push(1) }\n  inner([])\n}\n\
             export class Node {\n  #push(sets: number[]): void { sets.push(1) }\n  private grow(list: number[]): void { list.push(2) }\n  merge(target: number[]): void { target.push(3) }\n}\n",
        )],
    );
    assert_eq!(
        anchors_for(&findings, "collection_parameter_mutation"),
        ["symbol:Node::merge", "symbol:add"]
    );
}

#[test]
fn kotlin_require_preconditions_and_rust_underscore_items_are_not_findings() {
    let findings = analyze(
        "kotlin-require-rust-underscore",
        &[
            (
                "src/Status.kt",
                "fun parse(status: String): String {\n  require(status.isNotEmpty()) { \"Unexpected status: $status\" }\n  require(status.length > 2) { \"Unexpected status: $status\" }\n  return status\n}\n",
            ),
            ("src/lib.rs", "pub fn used() {}\nfn _assert_send_sync() {}\n"),
        ],
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.detector == "duplicate_import" || f.message.contains("_assert_send_sync")),
        "{:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}

#[test]
fn csharp_extension_receivers_do_not_count_as_parameters() {
    let findings = analyze(
        "csharp-extension-receiver",
        &[(
            "src/RetrySyntax.cs",
            "public static class RetrySyntax {\n\
             public static int Retry(this Builder builder, int a, int b, int c, int d, int e) { return a; }\n\
             public static int Wait(Builder builder, int a, int b, int c, int d, int e) { return a; }\n}\n",
        )],
    );
    assert_eq!(
        anchors_for(&findings, "too_many_parameters"),
        ["symbol:RetrySyntax::Wait"]
    );
}

#[test]
fn foreign_bindings_and_kotlin_default_values_do_not_count_as_parameters() {
    let findings = analyze(
        "foreign-bindings",
        &[(
            "src/Preview1.kt",
            "@WasmImport(\"wasi\", \"path_open\")\n\
             internal external fun path_open(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int): Int\n\
             internal fun String.canon(a: Int = 0, b: Int = length, c: Int, d: Int, e: Int): Int = a\n\
             internal fun wide(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int): Int = a\n",
        )],
    );
    assert_eq!(
        anchors_for(&findings, "too_many_parameters"),
        ["symbol:wide"]
    );
}

#[test]
fn rust_methods_anchor_on_their_impl_type() {
    let params = "a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8, i: u8";
    let source = format!(
        "pub struct Left;\npub struct Right;\n\
         impl Left {{\n    pub fn new({params}) -> u8 {{ a + b + c + d + e + f + g + h + i }}\n}}\n\
         impl Right {{\n    pub fn new({params}) -> u8 {{ a + b + c + d + e + f + g + h + i }}\n}}\n"
    );
    let findings = analyze("anchor-impl", &[("src/lib.rs", &source)]);
    assert_eq!(
        anchors_for(&findings, "too_many_parameters"),
        ["symbol:Left::new", "symbol:Right::new"]
    );
}

#[test]
fn go_methods_anchor_on_their_receiver_base_type() {
    let params = "a, b, c, d, e, f, g, h, i int";
    let body = "{ return a + b + c + d + e + f + g + h + i }";
    let source = format!(
        "package service\n\ntype Left struct{{}}\ntype Right[T any] struct{{}}\n\n\
         func (l *Left) Sum({params}) int {body}\n\n\
         func (r Right[T]) Sum({params}) int {body}\n"
    );
    let findings = analyze("anchor-go-receiver", &[("service/sum.go", &source)]);
    assert_eq!(
        anchors_for(&findings, "too_many_parameters"),
        ["symbol:Left::Sum", "symbol:Right::Sum"]
    );
    // Grouped names count individually and the receiver is not a parameter.
    assert!(findings
        .iter()
        .filter(|finding| finding.detector == "too_many_parameters")
        .all(|finding| finding.message.contains("takes 9 parameters")));
}

#[test]
fn findings_in_unchanged_code_keep_ids_when_lines_shift() {
    let original = "def add(items, value):\n    items.append(value)\n    return items\n";
    let shifted = format!("import os\n\n\ndef helper():\n    return os.sep\n\n\n{original}");
    let ids = |name, source: &str| {
        analyze(name, &[("src/service.py", source)])
            .into_iter()
            .filter(|finding| finding.detector == "collection_parameter_mutation")
            .map(|finding| finding.id)
            .collect::<Vec<_>>()
    };
    let before = ids("anchor-shift-a", original);
    assert_eq!(before.len(), 1);
    assert_eq!(before, ids("anchor-shift-b", &shifted));
}

#[test]
fn renaming_the_anchor_symbol_changes_the_id() {
    let ids = |name, function: &str| {
        let source =
            format!("def {function}(items, value):\n    items.append(value)\n    return items\n");
        analyze(name, &[("src/service.py", source.as_str())])
            .into_iter()
            .filter(|finding| finding.detector == "collection_parameter_mutation")
            .map(|finding| finding.id)
            .collect::<Vec<_>>()
    };
    let original = ids("anchor-rename-a", "add");
    assert_eq!(original.len(), 1);
    assert_ne!(original, ids("anchor-rename-b", "append_value"));
}

#[test]
fn a_common_class_name_in_another_file_is_not_a_child() {
    let children = (0..16)
        .map(|n| format!("class S{n}(State):\n    pass\n"))
        .collect::<String>();
    let findings = analyze(
        "class-name-collision",
        &[
            ("src/cache.py", "class State:\n    pass\n"),
            (
                "src/machine.py",
                &format!("class State:\n    pass\n{children}"),
            ),
        ],
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.detector == "class_structure_metrics"
                && f.locations
                    .iter()
                    .any(|location| location.path == "src/cache.py")),
        "{:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}

#[test]
fn todos_in_build_configuration_are_not_production_todos() {
    let findings = analyze(
        "todo-tooling",
        &[
            (
                "eslint.config.js",
                "// TODO: enable these rules\nexport default [];\n",
            ),
            (
                "src/app.js",
                "// TODO: handle retries\nexport const app = 1;\n",
            ),
        ],
    );
    let paths = findings
        .iter()
        .filter(|f| f.detector == "todo_unimplemented_in_production")
        .flat_map(|f| f.locations.iter().map(|location| location.path.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(paths, ["src/app.js"]);
}

#[test]
fn a_language_scoped_to_full_is_dropped_from_gating_scans_and_marked() {
    let root = temp_root("full-profile-language");
    write(
        &root,
        "src/service.rs",
        "pub fn stop() {\n    std::process::exit(1);\n}\n",
    );
    write(
        &root,
        "src/service.js",
        "export function stop() {\n  process.exit(1);\n}\n",
    );
    let languages = |profile| {
        let analysis =
            analyze_repository(&root, SourceDiscoveryOptions::default(), profile).unwrap();
        let mut languages = analysis
            .findings
            .iter()
            .filter(|f| f.detector == "exit_in_production")
            .map(|f| f.provenance.language.clone())
            .collect::<Vec<_>>();
        languages.sort();
        (languages, analysis.detectors)
    };
    let (full, _) = languages(ScanProfile::Full);
    let (gating, detectors) = languages(ScanProfile::Gating);
    assert_eq!(full, ["javascript", "rust"]);
    assert_eq!(gating, ["rust"]);
    let javascript = detectors
        .iter()
        .find(|d| d.id == "exit_in_production")
        .and_then(|d| {
            d.language_support
                .iter()
                .find(|claim| claim.language == "javascript")
        })
        .unwrap();
    assert!(javascript
        .limitation
        .as_deref()
        .unwrap()
        .starts_with("Full profile only"));
}

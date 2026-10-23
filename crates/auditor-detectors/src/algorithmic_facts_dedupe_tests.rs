use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{analyze_repository, ScanProfile, SourceDiscoveryOptions};

/// The `hot_path_linear_membership` messages for one source file.
fn messages(name: &str, relative: &str, source: &str) -> Vec<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root: PathBuf =
        std::env::temp_dir().join(format!("ea-dedupe-{name}-{}-{nanos}", std::process::id()));
    let absolute = root.join(relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, source).unwrap();
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Full,
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(root);
    analysis
        .findings
        .into_iter()
        .filter(|finding| finding.detector == "hot_path_linear_membership")
        .map(|finding| finding.message)
        .collect()
}

fn dedupe(collection: &str, needle: &str) -> String {
    format!(
        "Function `dedupe` appends loop-derived `{needle}` to linear collection `{collection}` only after scanning it for `{needle}`."
    )
}

#[test]
fn guarded_appends_are_deduplications_in_every_supported_language() {
    let cases = [
        (
            "python",
            "app/dedupe.py",
            "def dedupe(items):\n    out = []\n    for item in items:\n        if item not in out:\n            out.append(item)\n    return out\n",
        ),
        (
            "javascript",
            "src/dedupe.js",
            "function dedupe(items) {\n  const out = [];\n  for (const item of items) {\n    if (!out.includes(item)) {\n      out.push(item);\n    }\n  }\n  return out;\n}\n",
        ),
        (
            "typescript",
            "src/dedupe.ts",
            "function dedupe(items: string[]): string[] {\n  const out: string[] = [];\n  for (const item of items) {\n    if (!out.includes(item)) out.push(item);\n  }\n  return out;\n}\n",
        ),
        (
            "java",
            "src/main/java/app/Dedupe.java",
            "package app;\n\nimport java.util.*;\n\nclass Dedupe {\n    List<String> dedupe(List<String> items) {\n        List<String> out = new ArrayList<>();\n        for (String item : items) {\n            if (!out.contains(item)) {\n                out.add(item);\n            }\n        }\n        return out;\n    }\n}\n",
        ),
        (
            "csharp",
            "src/Dedupe.cs",
            "using System.Collections.Generic;\n\npublic class Dedupe {\n    public List<int> dedupe(List<int> items) {\n        var out = new List<int>();\n        foreach (var item in items) {\n            if (!out.Contains(item)) {\n                out.Add(item);\n            }\n        }\n        return out;\n    }\n}\n",
        ),
        (
            "rust",
            "src/lib.rs",
            "pub fn dedupe(items: Vec<String>) -> Vec<String> {\n    let mut out = Vec::new();\n    for item in items {\n        if !out.contains(&item) {\n            out.push(item);\n        }\n    }\n    out\n}\n",
        ),
        (
            "go",
            "dedupe.go",
            "package app\n\nimport \"slices\"\n\nfunc dedupe(items []string) []string {\n\tout := []string{}\n\tfor _, item := range items {\n\t\tif !slices.Contains(out, item) {\n\t\t\tout = append(out, item)\n\t\t}\n\t}\n\treturn out\n}\n",
        ),
    ];
    let mut missed = Vec::new();
    for (language, relative, source) in cases {
        let found = messages(language, relative, source);
        if found != [dedupe("out", "item")] {
            missed.push(format!("{language}: {found:?}"));
        }
    }
    assert!(missed.is_empty(), "{missed:#?}");
}

#[test]
fn only_a_lone_append_guarded_by_the_negated_scan_deduplicates() {
    let source = "\
def unconditional(items):
    out = []
    for item in items:
        out.append(item)
        if item in out:
            consume(item)


def compound(items, keep):
    out = []
    for item in items:
        if item not in out and keep(item):
            out.append(item)


def set_backed(items):
    seen = set()
    for item in items:
        if item not in seen:
            seen.add(item)


def else_branch(items):
    out = []
    for item in items:
        if item in out:
            consume(item)
        else:
            out.append(item)


def twice(items):
    out = []
    for item in items:
        if item not in out:
            out.append(item)
            out.append(item)


def outside_guard(items):
    out = []
    for item in items:
        if item not in out:
            consume(item)
        out.append(item)


def other_element(items, other):
    out = []
    for item in items:
        if item not in out:
            out.append(other)
";
    assert_eq!(
        messages("negatives", "app/negatives.py", source),
        Vec::<String>::new()
    );
}

#[test]
fn go_rebindings_deduplicate_only_inside_the_guard() {
    let source = "package app\n\nimport \"slices\"\n\nfunc outside(items []string) []string {\n\tout := []string{}\n\tfor _, item := range items {\n\t\tif !slices.Contains(out, item) {\n\t\t\tconsume(item)\n\t\t}\n\t\tout = append(out, item)\n\t}\n\treturn out\n}\n\nfunc other(items []string, fallback string) []string {\n\tout := []string{}\n\tfor _, item := range items {\n\t\tif !slices.Contains(out, item) {\n\t\t\tout = append(out, fallback)\n\t\t}\n\t}\n\treturn out\n}\n";
    assert_eq!(
        messages("go-negatives", "negatives.go", source),
        Vec::<String>::new()
    );
}

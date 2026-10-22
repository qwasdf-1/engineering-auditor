use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::LanguageSupportMode;

use super::{analyze_case, capability_cases, CapabilityCase};

#[path = "capability_language_go_tests.rs"]
mod go;
use go::{go_capability_files, go_fact_capability_files};

#[path = "capability_language_js_tests.rs"]
mod js;
use js::{js_capability_files, js_fact_capability_files};

#[path = "capability_language_csharp_tests.rs"]
mod csharp;
use csharp::csharp_fact_capability_files;

#[path = "capability_language_c_tests.rs"]
mod c;
use c::c_fact_capability_files;

#[path = "capability_language_cpp_tests.rs"]
mod cpp;
use cpp::cpp_fact_capability_files;

#[path = "capability_language_java_tests.rs"]
mod java;
use java::{java_capability_files, java_fact_capability_files};

#[path = "capability_language_python_tests.rs"]
mod python;
use python::{python_fact_capability_files, python_l1_capability_files};

#[path = "capability_language_l2_tests.rs"]
mod l2;
use l2::l2_capability_files;

#[path = "capability_language_rust_tests.rs"]
mod rust;

#[path = "capability_language_hygiene_tests.rs"]
mod hygiene;
pub(crate) use hygiene::hygiene_capability_files;

#[path = "capability_language_l3_rules_tests.rs"]
mod l3_rules;
pub(crate) use l3_rules::l3_rule_capability_files;

#[path = "capability_language_retry_tests.rs"]
mod retry;
pub(crate) use retry::retry_capability_files;

#[path = "capability_language_pass_through_tests.rs"]
mod pass_through;
pub(crate) use pass_through::pass_through_capability_files;

#[path = "capability_language_single_implementation_tests.rs"]
mod single_implementation;
pub(crate) use single_implementation::single_implementation_capability_files;

#[path = "capability_language_unbounded_tests.rs"]
mod unbounded;
pub(crate) use unbounded::unbounded_capability_files;

#[path = "capability_language_eager_log_tests.rs"]
mod eager_log;
pub(crate) use eager_log::eager_log_capability_files;

#[path = "capability_language_float_tests.rs"]
mod float;
pub(crate) use float::float_capability_files;

#[path = "capability_language_mutable_default_tests.rs"]
mod mutable_default;
pub(crate) use mutable_default::mutable_default_capability_files;

#[path = "capability_language_network_timeout_tests.rs"]
mod network_timeout;
pub(crate) use network_timeout::network_timeout_capability_files;

#[path = "capability_language_skip_tests.rs"]
mod skip_tests;
#[path = "capability_language_test_quality_tests.rs"]
mod test_quality;
pub(crate) use test_quality::test_quality_capability_files;

#[path = "capability_language_tier2_tests.rs"]
mod tier2;
use rust::{rust_fact_capability_files, rust_l1_capability_files};
use tier2::tier2_capability_files;

const SEMANTIC_SOURCE_IDS: &[&str] = &[
    "function_complexity",
    "cognitive_complexity",
    "npath_complexity",
    "halstead_information_density",
    "unreachable_code",
    "constant_condition",
    "excessive_nesting_depth",
];

#[test]
fn catalog_language_claims_have_executable_boundaries() {
    let definitions = crate::registry::detector_registry(crate::ScanProfile::Full)
        .into_iter()
        .map(|definition| (definition.id, definition))
        .collect::<BTreeMap<_, _>>();
    let mut failures = Vec::new();

    for case in capability_cases() {
        let definition = &definitions[case.id];
        let repository_level = definition
            .language_support
            .iter()
            .all(|claim| claim.mode == LanguageSupportMode::NotApplicable);
        for claim in &definition.language_support {
            if repository_level {
                continue;
            }
            let variant = retarget_case(&case, claim.language);
            let profile = super::language_profile(case.id, case.profile, claim.language);
            let positive = analyze_case(&variant, true, profile);
            let positive_count = detector_count_for_language(&positive, case.id, claim.language);
            match claim.mode {
                LanguageSupportMode::Semantic
                | LanguageSupportMode::ExactLexical
                | LanguageSupportMode::Partial => {
                    if positive_count == 0 {
                        failures.push(format!(
                            "{} claims {} for {} but its positive boundary did not fire",
                            case.id,
                            claim.mode.as_str(),
                            claim.language
                        ));
                        continue;
                    }
                    let negative = analyze_case(&variant, false, profile);
                    let negative_count =
                        detector_count_for_language(&negative, case.id, claim.language);
                    if negative_count != 0 {
                        failures.push(format!(
                            "{} claims {} for {} but fired {negative_count} time(s) on its negative boundary",
                            case.id,
                            claim.mode.as_str(),
                            claim.language
                        ));
                    }
                }
                LanguageSupportMode::Unsupported | LanguageSupportMode::NotApplicable => {
                    if positive_count != 0 {
                        failures.push(format!(
                            "{} claims {} for {} but fired {positive_count} time(s) on the retargeted positive boundary",
                            case.id,
                            claim.mode.as_str(),
                            claim.language
                        ));
                    }
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn detector_count_for_language(
    analysis: &crate::RepositoryAnalysis,
    detector: &str,
    language: &str,
) -> usize {
    analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == detector)
        .filter(|finding| {
            finding.locations.iter().any(|location| {
                let location_path = location.path.as_str();
                analysis.files.iter().any(|file| {
                    (file.path == location_path || file.path.starts_with(location_path))
                        && file.catalog_language == language
                })
            })
        })
        .count()
}

pub(super) fn retarget_case(case: &CapabilityCase, language: &str) -> CapabilityCase {
    if !SEMANTIC_SOURCE_IDS.contains(&case.id)
        && native_language(case).is_some_and(|native| native == language)
    {
        return case.clone();
    }
    let mut variant = case.clone();
    variant.positive = retarget_files(&case.positive, language);
    variant.negative = retarget_files(&case.negative, language);
    if language == "python" {
        if let Some(files) = python_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = python_capability_files(case.id, false) {
            variant.negative = files;
        }
        if let Some(files) = python_l1_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = python_l1_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "rust" {
        if let Some(files) = rust_l1_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = rust_l1_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "go" {
        if let Some(files) = go_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = go_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if matches!(language, "javascript" | "typescript") {
        if let Some(files) = js_capability_files(case.id, language, true) {
            variant.positive = files;
        }
        if let Some(files) = js_capability_files(case.id, language, false) {
            variant.negative = files;
        }
    }
    if language == "java" {
        if let Some(files) = java_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = java_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if let Some(files) = structure_metric_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = structure_metric_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = algorithmic_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = algorithmic_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = systems_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = systems_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = optimization_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = optimization_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if matches!(language, "javascript" | "typescript") {
        if let Some(files) = js_fact_capability_files(case.id, language, true) {
            variant.positive = files;
        }
        if let Some(files) = js_fact_capability_files(case.id, language, false) {
            variant.negative = files;
        }
    }
    if language == "java" {
        if let Some(files) = java_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = java_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "csharp" {
        if let Some(files) = csharp_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = csharp_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "c" {
        if let Some(files) = c_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = c_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "cpp" {
        if let Some(files) = cpp_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = cpp_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "python" {
        if let Some(files) = python_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = python_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "rust" {
        if let Some(files) = rust_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = rust_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if language == "go" {
        if let Some(files) = go_fact_capability_files(case.id, true) {
            variant.positive = files;
        }
        if let Some(files) = go_fact_capability_files(case.id, false) {
            variant.negative = files;
        }
    }
    if is_tiered_language(language) {
        if let Some(files) = l2_capability_files(case.id, language, true) {
            variant.positive = files;
        }
        if let Some(files) = l2_capability_files(case.id, language, false) {
            variant.negative = files;
        }
    }
    if matches!(language, "kotlin" | "csharp" | "c" | "cpp") {
        if let Some(files) = tier2_capability_files(case.id, language, true) {
            variant.positive = files;
        }
        if let Some(files) = tier2_capability_files(case.id, language, false) {
            variant.negative = files;
        }
    }
    if let Some(files) = retry_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = retry_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = pass_through_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = pass_through_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = single_implementation_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = single_implementation_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = unbounded_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = unbounded_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = eager_log_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = eager_log_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = float_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = float_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = hygiene_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = hygiene_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if let Some(files) = l3_rule_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = l3_rule_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if case.id == "network_call_without_timeout" {
        if let Some(files) = network_timeout_capability_files(language, true) {
            variant.positive = files;
        }
        if let Some(files) = network_timeout_capability_files(language, false) {
            variant.negative = files;
        }
    }
    if let Some(files) = test_quality_capability_files(case.id, language, true) {
        variant.positive = files;
    }
    if let Some(files) = test_quality_capability_files(case.id, language, false) {
        variant.negative = files;
    }
    if SEMANTIC_SOURCE_IDS.contains(&case.id) && is_source_language(language) {
        let path = format!("src/capability.{}", extension(language));
        variant.positive = vec![(path.clone(), semantic_source(language, case.id, true))];
        variant.negative = vec![(path, semantic_source(language, case.id, false))];
    }
    variant
}

fn optimization_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let source = match (detector, language, positive) {
        ("cache_miss_opportunity", "python", true) => {
            "def inspect(data: int):\n    return abs(data) == abs(data) and abs(data) > 0\n"
        }
        ("cache_miss_opportunity", "python", false) => {
            "def inspect(data: int):\n    return abs(data) == abs(data)\n"
        }
        ("cache_miss_opportunity", "typescript", true) => {
            "function inspect(data: number) { return Math.abs(data) === Math.abs(data) && Math.abs(data) > 0 }\n"
        }
        ("cache_miss_opportunity", "typescript", false) => {
            "function inspect(data: number) { return Math.abs(data) === Math.abs(data) }\n"
        }
        ("repeated_format_allocation", "python", true) => {
            "def render(items: list[int], mode: str):\n    for item in items:\n        consume(item, 'mode={}'.format(mode))\n"
        }
        ("repeated_format_allocation", "python", false) => {
            "def render(items: list[int]):\n    for item in items:\n        consume('{}'.format(item))\n"
        }
        ("loop_invariant_regex_compilation", "python", true) => {
            "import re\ndef inspect(items):\n    for item in items:\n        pattern = re.compile('^fixed$')\n        consume(pattern, item)\n"
        }
        ("loop_invariant_regex_compilation", "python", false) => {
            "import re\ndef inspect(items):\n    for item in items:\n        pattern = re.compile(item)\n        consume(pattern)\n"
        }
        ("loop_invariant_regex_compilation", "javascript" | "typescript", true) => {
            "function inspect(items) { for (const item of items) { const pattern = new RegExp('^fixed$'); consume(pattern, item) } }\n"
        }
        ("loop_invariant_regex_compilation", "javascript" | "typescript", false) => {
            "function inspect(items) { for (const item of items) { const pattern = new RegExp(item); consume(pattern) } }\n"
        }
        ("loop_invariant_regex_compilation", "java", true) => {
            "import java.util.regex.Pattern;\nclass Capability { void inspect(String[] items) { for (String item : items) { Pattern pattern = Pattern.compile(\"^fixed$\"); consume(pattern, item); } } }\n"
        }
        ("loop_invariant_regex_compilation", "java", false) => {
            "import java.util.regex.Pattern;\nclass Capability { void inspect(String[] items) { for (String item : items) { Pattern pattern = Pattern.compile(item); consume(pattern); } } }\n"
        }
        ("intermediate_collection_materialization", "python", true) => {
            "def total(items: list[int]):\n    return sum([item for item in items])\n"
        }
        ("intermediate_collection_materialization", "python", false) => {
            "def total(items: list[int]):\n    return sum(item for item in items)\n"
        }
        ("intermediate_collection_materialization", "kotlin", true) => {
            "fun emit(items: List<String>) { consume(*items.toTypedArray()) }\n"
        }
        ("intermediate_collection_materialization", "kotlin", false) => {
            "fun emit(items: List<String>) { consume(items) }\n"
        }
        ("loop_invariant_computation", "python", true) => {
            "def render(items: list[int], limit: int):\n    for item in items:\n        consume(item, abs(limit))\n"
        }
        ("loop_invariant_computation", "python", false) => {
            "def render(items: list[int]):\n    for item in items:\n        consume(item, abs(item))\n"
        }
        ("loop_invariant_computation", "typescript", true) => {
            "function render(items: number[], limit: number) { for (const item of items) consume(item, Math.abs(limit)) }\n"
        }
        ("loop_invariant_computation", "typescript", false) => {
            "function render(items: number[]) { for (const item of items) consume(item, Math.abs(item)) }\n"
        }
        ("bulk_collection_extension", "python", true) => {
            "def extend_all(items: list[str]):\n    target = []\n    for item in items:\n        target.append(item)\n    return target\n"
        }
        ("bulk_collection_extension", "python", false) => {
            "def extend_all(items: list[str]):\n    target = []\n    for item in items:\n        target.append(str(item))\n    return target\n"
        }
        ("bulk_collection_extension", "javascript", true) => {
            "function extendAll() { const items = ['a', 'b']; const target = []; for (const item of items) target.push(item); return target }\n"
        }
        ("bulk_collection_extension", "javascript", false) => {
            "function extendAll() { const items = ['a', 'b']; const target = []; for (const item of items) target.push(String(item)); return target }\n"
        }
        ("bulk_collection_extension", "typescript", true) => {
            "function extendAll(items: string[]) { const target: string[] = []; for (const item of items) target.push(item); return target }\n"
        }
        ("bulk_collection_extension", "typescript", false) => {
            "function extendAll(items: string[]) { const target: string[] = []; for (const item of items) target.push(String(item)); return target }\n"
        }
        ("immutable_string_concatenation_in_loop", "typescript", true) => {
            "function render(items: string[]) { let output: string = ''; for (const item of items) output += item; return output }\n"
        }
        ("immutable_string_concatenation_in_loop", "typescript", false) => {
            "function render(items: string[]) { return items.join('') }\n"
        }
        ("immutable_string_concatenation_in_loop", "go", true) => {
            "package capability\nfunc render(items []string) string { output := \"\"; for _, item := range items { output += item }; return output }\n"
        }
        ("immutable_string_concatenation_in_loop", "go", false) => {
            "package capability\nimport \"strings\"\nfunc render(items []string) string { return strings.Join(items, \"\") }\n"
        }
        ("immutable_string_concatenation_in_loop", "java", true) => {
            "import java.util.List;\nclass Capability { String render(List<String> items) { String output = \"\"; for (String item : items) output += item; return output; } }\n"
        }
        ("immutable_string_concatenation_in_loop", "java", false) => {
            "import java.util.List;\nclass Capability { String render(List<String> items) { return String.join(\"\", items); } }\n"
        }
        ("immutable_string_concatenation_in_loop", "kotlin", true) => {
            "fun render(items: List<String>): String { var output = \"\"; for (item in items) output += item; return output }\n"
        }
        ("immutable_string_concatenation_in_loop", "kotlin", false) => {
            "fun render(items: List<String>): String = items.joinToString(\"\")\n"
        }
        _ => return None,
    };
    Some(vec![(
        format!("src/optimization.{}", extension(language)),
        source.to_string(),
    )])
}

fn systems_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if !matches!(language, "python" | "javascript" | "typescript") {
        return None;
    }
    if detector == "architectural_layering" {
        return Some(architecture_files(language, positive));
    }
    if detector == "package_coupling_metrics" && language != "python" {
        return Some(package_files(language, positive));
    }
    let source = match (detector, language, positive) {
        ("loop_artifact_rewrite", "python", true) => "from pathlib import Path\ndef rewrite(paths):\n    for path in paths:\n        raw = Path(path).read_text()\n        Path('report.json').write_text(raw)\n",
        ("loop_artifact_rewrite", "python", false) => "from pathlib import Path\ndef rewrite(paths):\n    raw = Path('input').read_text()\n    for path in paths:\n        consume(path, raw)\n",
        ("loop_artifact_rewrite", _, true) => "function rewrite(paths) {\n  for (const path of paths) {\n    const raw = fs.readFileSync(path)\n    fs.writeFileSync('report.json', raw)\n  }\n}\n",
        ("loop_artifact_rewrite", _, false) => "function rewrite(paths) {\n  const raw = fs.readFileSync('input')\n  for (const path of paths) consume(path, raw)\n}\n",
        ("latest_only_unbounded_cache", "python", true) => "def update(cache, entry):\n    cache.append(entry)\n    consume(cache.latest())\n",
        ("latest_only_unbounded_cache", "python", false) => "def update(cache, entry):\n    cache.append(entry)\n    cache.set_max_size(10)\n    consume(cache.latest())\n",
        ("latest_only_unbounded_cache", _, true) => "function update(cache, entry) { cache.push(entry); consume(cache.last()) }\n",
        ("latest_only_unbounded_cache", _, false) => "function update(cache, entry) { cache.push(entry); cache.set_max_size(10); consume(cache.last()) }\n",
        ("repeated_parse_scan_serialize", "python", true) => "import json\ndef process(items):\n    for item in items:\n        parsed = json.loads(item)\n        scan(parsed)\n        write(json.dumps(parsed))\n",
        ("repeated_parse_scan_serialize", "python", false) => "import json\ndef process(items):\n    for item in items:\n        parsed = json.loads(item)\n        write(json.dumps(parsed))\n",
        ("repeated_parse_scan_serialize", _, true) => "function process(items) {\n  for (const item of items) {\n    const parsed = JSON.parse(item)\n    scan(parsed)\n    write(JSON.stringify(parsed))\n  }\n}\n",
        ("repeated_parse_scan_serialize", _, false) => "function process(items) {\n  for (const item of items) {\n    const parsed = JSON.parse(item)\n    write(JSON.stringify(parsed))\n  }\n}\n",
        ("blocking_io_in_async_context", "python", true) => "from pathlib import Path\nasync def load():\n    consume(Path('input').read_text())\n",
        ("blocking_io_in_async_context", "python", false) => "from pathlib import Path\ndef load():\n    consume(Path('input').read_text())\n",
        ("blocking_io_in_async_context", _, true) => "async function load() { consume(fs.readFileSync('input')) }\n",
        ("blocking_io_in_async_context", _, false) => "function load() { consume(fs.readFileSync('input')) }\n",
        ("per_item_external_io", "python", true) => "def fetch_all(ids, client):\n    for item_id in ids:\n        consume(client.get(item_id))\n",
        ("per_item_external_io", "python", false) => "def fetch_all(ids, client):\n    response = client.get(ids)\n    for item in response:\n        consume(item)\n    for batch in chunked(ids, 100):\n        consume(client.get(batch))\n    for page in range(10):\n        consume(client.get(page))\n",
        ("per_item_external_io", _, true) => "function fetchAll(ids) { for (const id of ids) consume(fetch(id)) }\n",
        ("per_item_external_io", _, false) => "function fetchAll(ids) { const response = fetch(ids); for (const item of response) consume(item) }\n",
        ("repeated_expensive_initialization", "python", true) => "import re\ndef validate(items):\n    for item in items:\n        parser = re.compile('[a-z]+')\n        consume(parser, item)\n",
        ("repeated_expensive_initialization", "python", false) => "import re\ndef validate(items):\n    parser = re.compile('[a-z]+')\n    for item in items:\n        consume(parser, item)\n",
        ("repeated_expensive_initialization", _, true) => "function validate(items) { for (const item of items) { const parser = RegExp('[a-z]+'); consume(parser, item) } }\n",
        ("repeated_expensive_initialization", _, false) => "function validate(items) { const parser = RegExp('[a-z]+'); for (const item of items) consume(parser, item) }\nfunction perPattern(args) { for (var i = 0; i < args.length; i++) { var value = valueOf(args[i]); consume(new RegExp(value)) } }\n",
        ("request_path_repository_scan", "python", true) => "import os\ndef handle_request():\n    consume(os.scandir('.'))\n",
        ("request_path_repository_scan", "python", false) => "import os\ndef refresh_index():\n    consume(os.scandir('.'))\n",
        ("request_path_repository_scan", _, true) => "function handleRequest() { consume(fs.readdirSync('.')) }\n",
        ("request_path_repository_scan", _, false) => "function refreshIndex() { consume(fs.readdirSync('.')) }\n",
        _ => return None,
    };
    Some(vec![(
        format!("src/capability.{}", extension(language)),
        source.to_string(),
    )])
}

fn architecture_files(language: &str, positive: bool) -> Vec<(String, String)> {
    let extension = extension(language);
    let import = match (language, positive) {
        ("python", true) => "from src.app.service import UserService\ndef check():\n    return UserService\n",
        ("python", false) => "from src.domain.model import User\ndef check():\n    return User\n",
        (_, true) => "import { UserService } from '../app/service.js'\nexport function check() { return UserService }\n",
        (_, false) => "import { User } from './model.js'\nexport function check() { return User }\n",
    };
    let target = if positive {
        "app/service"
    } else {
        "domain/model"
    };
    let target_source = if positive {
        if language == "python" {
            "class UserService:\n    pass\n"
        } else {
            "export class UserService {}\n"
        }
    } else if language == "python" {
        "class User:\n    pass\n"
    } else {
        "export class User {}\n"
    };
    vec![
        (
            format!("src/domain/user.{extension}"),
            import.to_string(),
        ),
        (
            format!("src/{target}.{extension}"),
            target_source.to_string(),
        ),
        (
            "auditor.toml".to_string(),
            format!(
                "[layering.layer.domain]\nglob = src/domain/**/*.{extension}\n\n[layering.layer.app]\nglob = src/app/**/*.{extension}\n\n[layering.rule.domain]\ndeny = app\n"
            ),
        ),
    ]
}

fn package_files(language: &str, positive: bool) -> Vec<(String, String)> {
    let extension = extension(language);
    if !positive {
        return vec![(
            format!("packages/core/src/model.{extension}"),
            "export class Model {}\n".to_string(),
        )];
    }
    vec![
        (
            format!("packages/core/src/model.{extension}"),
            "export class Model {}\n".to_string(),
        ),
        (
            format!("packages/app/src/service.{extension}"),
            if language == "javascript" {
                "import { Model } from '../../core/src/model.js'\nexport class Service { process(model) { return model instanceof Model } }\n"
                    .to_string()
            } else {
                "import { Model } from '../../core/src/model.js'\nexport interface Processor { process(model: Model): void }\nexport class Service {}\n"
                    .to_string()
            },
        ),
        (
            format!("packages/web/src/handler.{extension}"),
            "import { Model } from '../../core/src/model.js'\nexport class Handler { handle(model) { return model instanceof Model } }\n"
                .to_string(),
        ),
    ]
}

fn algorithmic_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if let Some(source) = managed_algorithmic_source(detector, language, positive) {
        return Some(vec![(
            format!("src/capability.{}", extension(language)),
            source,
        )]);
    }
    if language != "typescript" {
        return None;
    }
    let source = match (detector, positive) {
        ("nested_quadratic_iteration", true) => {
            "function inspect(items: Item[]) {\n  for (const left of items) {\n    for (const right of items) { if (left.id === right.id) consume(left, right) }\n  }\n}\n"
        }
        ("nested_quadratic_iteration", false) => {
            "interface Options { left: number[]; right: number[] }\nfunction inspect(options: Options) {\n  for (const left of options.left) { for (const right of options.right) consume(left, right) }\n}\n"
        }
        ("repeated_sort_in_loop", true) => {
            "function inspect(requests: Request[], candidates: number[]) {\n  for (const request of requests) { candidates.sort(); consume(request, candidates[0]) }\n}\n"
        }
        ("repeated_sort_in_loop", false) => {
            "function inspect(groups: number[][]) {\n  for (const group of groups) { const values = [...group]; values.sort(); consume(values) }\n}\n"
        }
        ("sort_for_extremum", true) => {
            "function smallest(items: number[]) { items.sort(); return items.at(0) }\n"
        }
        ("sort_for_extremum", false) => {
            "function ordered(items: number[]) { items.sort(compare); return items }\n"
        }
        ("full_sort_for_top_k", true) => {
            "function smallest(items: number[]) { items.sort(); return items.slice(0, 5) }\n"
        }
        ("full_sort_for_top_k", false) => {
            "function smallest(items: number[]) { items.sort(); return items.at(0) }\n"
        }
        ("repeated_collection_traversal", true) => {
            "function inspect(items: Item[]) {\n  consume(items.some(item => item.ready))\n  consume(items.every(item => item.valid))\n  consume(items.find(item => item.target))\n}\n"
        }
        ("repeated_collection_traversal", false) => {
            "interface Groups { ready: Item[]; valid: Item[]; targets: Item[] }\nfunction distinct(groups: Groups) {\n  consume(groups.ready.some(item => item.ready))\n  consume(groups.valid.every(item => item.valid))\n  consume(groups.targets.find(item => item.target))\n}\nfunction changed(items: Item[]) {\n  consume(items.some(item => item.ready))\n  mutate(items)\n  consume(items.every(item => item.valid))\n  consume(items.find(item => item.target))\n}\nfunction branched(items: Item[], first: boolean) {\n  if (first) {\n    consume(items.some(item => item.ready))\n    consume(items.every(item => item.valid))\n  } else {\n    consume(items.find(item => item.target))\n  }\n}\n"
        }
        ("hot_path_linear_membership", true) => {
            "function select(ids: string[], wanted: string[]) {\n  for (const id of ids) { if (wanted.includes(id)) consume(id) }\n}\n"
        }
        ("hot_path_linear_membership", false) => {
            "function select(ids: string[], wanted: Set<string>) {\n  for (const id of ids) { if (wanted.has(id)) consume(id) }\n}\n"
        }
        ("n_plus_one_query_in_loop", true) => {
            "function load(ids: number[], repo: Repository) {\n  for (const id of ids) consume(repo.find_by_id(id))\n}\n"
        }
        ("n_plus_one_query_in_loop", false) => {
            "function load(ids: number[], repo: Repository) { consume(repo.find_many(ids)) }\n"
        }
        _ => return None,
    };
    Some(vec![("src/capability.ts".to_string(), source.to_string())])
}

fn managed_algorithmic_source(detector: &str, language: &str, positive: bool) -> Option<String> {
    match detector {
        "nested_quadratic_iteration" => nested_same_collection_source(language, positive),
        "repeated_recursive_subproblem" => recursive_subproblem_source(language, positive),
        "loop_carried_copy_accumulation" => loop_copy_source(language, positive),
        "nested_cross_collection_traversal" => cross_collection_source(language, positive),
        "n_plus_one_query_in_loop" if matches!(language, "python" | "javascript") => {
            n_plus_one_source(language, positive)
        }
        _ => None,
    }
}

fn nested_same_collection_source(language: &str, positive: bool) -> Option<String> {
    let source = match (language, positive) {
        ("python", true) => "def inspect(items: list[Item]):\n    for left in items:\n        for right in items:\n            if left.id == right.id:\n                consume(left, right)\n",
        ("python", false) => "def inspect(left_items: list[Item], right_items: list[Item]):\n    for left in left_items:\n        for right in right_items:\n            consume(left, right)\n",
        ("javascript", true) => "function inspect(items) { for (const left of items) { for (const right of items) { if (left.id === right.id) consume(left, right) } } }\n",
        ("javascript", false) => "function inspect(leftItems, rightItems) { for (const left of leftItems) { for (const right of rightItems) consume(left, right) } }\n",
        ("typescript", true) => "function inspect(items: Item[]) { for (const left of items) { for (const right of items) { if (left.id === right.id) consume(left, right) } } }\n",
        ("typescript", false) => "function inspect(leftItems: Item[], rightItems: Item[]) { for (const left of leftItems) { for (const right of rightItems) consume(left, right) } }\n",
        ("go", true) => "package capability\nfunc inspect(items []Item) { for _, left := range items { for _, right := range items { if left.id == right.id { consume(left, right) } } } }\n",
        ("go", false) => "package capability\nfunc inspect(leftItems []Item, rightItems []Item) { for _, left := range leftItems { for _, right := range rightItems { consume(left, right) } } }\n",
        ("java", true) => "class Capability { void inspect(java.util.List<Item> items) { for (Item left : items) { for (Item right : items) { if (left.id == right.id) consume(left, right); } } } }\n",
        ("java", false) => "class Capability { void inspect(java.util.List<Item> leftItems, java.util.List<Item> rightItems) { for (Item left : leftItems) { for (Item right : rightItems) consume(left, right); } } }\n",
        ("kotlin", true) => "fun inspect(items: List<Item>) { for (left in items) { for (right in items) { if (left.id == right.id) consume(left, right) } } }\n",
        ("kotlin", false) => "fun inspect(leftItems: List<Item>, rightItems: List<Item>) { for (left in leftItems) { for (right in rightItems) consume(left, right) } }\n",
        _ => return None,
    };
    Some(source.to_string())
}

fn recursive_subproblem_source(language: &str, positive: bool) -> Option<String> {
    let source = match (language, positive) {
        ("python", true) => "def fib(n: int) -> int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n",
        ("python", false) => "def factorial(n: int) -> int:\n    if n < 2:\n        return 1\n    return n * factorial(n - 1)\n",
        ("javascript", true) => "function fib(n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2) }\n",
        ("javascript", false) => "function factorial(n) { if (n < 2) return 1; return n * factorial(n - 1) }\n",
        ("typescript", true) => "function fib(n: number): number { if (n < 2) return n; return fib(n - 1) + fib(n - 2) }\n",
        ("typescript", false) => "function factorial(n: number): number { if (n < 2) return 1; return n * factorial(n - 1) }\n",
        ("go", true) => "package capability\nfunc fib(n int) int { if n < 2 { return n }; return fib(n - 1) + fib(n - 2) }\n",
        ("go", false) => "package capability\nfunc factorial(n int) int { if n < 2 { return 1 }; return n * factorial(n - 1) }\n",
        ("java", true) => "class Capability { int fib(int n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); } }\n",
        ("java", false) => "class Capability { int factorial(int n) { if (n < 2) return 1; return n * factorial(n - 1); } }\n",
        ("kotlin", true) => "fun fib(n: Int): Int { if (n < 2) return n; return fib(n - 1) + fib(n - 2) }\n",
        ("kotlin", false) => "fun factorial(n: Int): Int { if (n < 2) return 1; return n * factorial(n - 1) }\n",
        _ => return None,
    };
    Some(source.to_string())
}

fn loop_copy_source(language: &str, positive: bool) -> Option<String> {
    let source = match (language, positive) {
        ("python", true) => "def collect(items: list[int]) -> list[int]:\n    result: list[int] = []\n    for item in items:\n        result = result + [item]\n    return result\n",
        ("python", false) => "def collect(items: list[int]) -> list[int]:\n    result: list[int] = []\n    for item in items:\n        result.append(item)\n    return result\n",
        ("javascript", true) => "function collect(items) { let result = []; for (const item of items) { result = [...result, item] } return result }\n",
        ("javascript", false) => "function collect(items) { const result = []; for (const item of items) result.push(item); return result }\n",
        ("typescript", true) => "function collect(items: number[]): number[] { let result: number[] = []; for (const item of items) { result = [...result, item] } return result }\n",
        ("typescript", false) => "function collect(items: number[]): number[] { const result: number[] = []; for (const item of items) result.push(item); return result }\n",
        _ => return None,
    };
    Some(source.to_string())
}

fn cross_collection_source(language: &str, positive: bool) -> Option<String> {
    let source = match (language, positive) {
        ("python", true) => "def compare(left: list[Item], right: list[Item]):\n    for outer in left:\n        for inner in right:\n            if outer.id == inner.id:\n                consume(outer, inner)\n",
        ("python", false) => "def visit(left: list[Item], right: list[Item]):\n    for outer in left:\n        consume(outer)\n    for inner in right:\n        consume(inner)\n",
        ("typescript", true) => "function compare(left: Item[], right: Item[]) { for (const outer of left) { for (const inner of right) { if (outer.id === inner.id) consume(outer, inner) } } }\n",
        ("typescript", false) => "function visit(left: Item[], right: Item[]) { for (const outer of left) consume(outer); for (const inner of right) consume(inner) }\n",
        _ => return None,
    };
    Some(source.to_string())
}

fn n_plus_one_source(language: &str, positive: bool) -> Option<String> {
    let source = match (language, positive) {
        ("python", true) => "def load(ids: list[int], repo: Repository):\n    for item_id in ids:\n        consume(repo.find_by_id(item_id))\n",
        ("python", false) => "def load(ids: list[int], repo: Repository):\n    consume(repo.find_many(ids))\n    for chunk in chunked(ids, 50):\n        consume(repo.find_by_id(chunk))\n",
        ("javascript", true) => "function load(ids, repo) { for (const id of ids) consume(repo.find_by_id(id)) }\n",
        ("javascript", false) => "function load(ids, repo) { consume(repo.find_many(ids)) }\n",
        _ => return None,
    };
    Some(source.to_string())
}

fn structure_metric_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector == "identifier_stutter" {
        return stutter_capability_files(language, positive);
    }
    if !matches!(
        language,
        "rust"
            | "python"
            | "javascript"
            | "typescript"
            | "go"
            | "java"
            | "kotlin"
            | "csharp"
            | "c"
            | "cpp"
    ) {
        return None;
    }
    let source = match detector {
        "too_many_parameters" => parameter_metric_source(language, if positive { 6 } else { 5 }),
        "too_many_locals" => local_metric_source(language, if positive { 16 } else { 15 }),
        // Go, Java, Kotlin, C#, and C wrappers end with their own `return`.
        "too_many_returns" => return_metric_source(
            language,
            if positive { 5 } else { 4 } + usize::from(implicit_return(language)),
        ),
        "too_many_statements" => statement_metric_source(
            language,
            if positive { 50 } else { 49 } + usize::from(implicit_return(language)),
        ),
        "god_module" => god_module_metric_source(language, if positive { 21 } else { 20 }),
        _ => return None,
    };
    Some(vec![(
        format!("src/metric.{}", extension(language)),
        source,
    )])
}

/// A public type in a `config` module, named `ConfigError` (stutter) or `Error`.
fn stutter_capability_files(language: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let name = if positive { "ConfigError" } else { "Error" };
    let (path, source) = match language {
        "rust" => ("src/config.rs", format!("pub struct {name};\n")),
        "go" => (
            "config/errors.go",
            format!("package config\n\ntype {name} struct{{}}\n"),
        ),
        _ => return None,
    };
    Some(vec![(path.to_string(), source)])
}

fn parameter_metric_source(language: &str, count: usize) -> String {
    let names = (0..count)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>();
    let parameters = match language {
        "rust" => names
            .iter()
            .map(|name| format!("{name}: i32"))
            .collect::<Vec<_>>()
            .join(", "),
        "typescript" => names
            .iter()
            .map(|name| format!("{name}: number"))
            .collect::<Vec<_>>()
            .join(", "),
        "go" => names
            .iter()
            .map(|name| format!("{name} int"))
            .collect::<Vec<_>>()
            .join(", "),
        "java" | "csharp" | "c" | "cpp" => names
            .iter()
            .map(|name| format!("int {name}"))
            .collect::<Vec<_>>()
            .join(", "),
        "kotlin" => names
            .iter()
            .map(|name| format!("{name}: Int"))
            .collect::<Vec<_>>()
            .join(", "),
        _ => names.join(", "),
    };
    wrap_metric_function(language, &parameters, "consume(p0);")
}

fn local_metric_source(language: &str, count: usize) -> String {
    let body = (0..count)
        .map(|index| match language {
            "python" => format!("    local_{index} = {index}\n"),
            "go" => format!("    local_{index} := {index}\n"),
            "javascript" | "typescript" => format!("    const local_{index} = {index};\n"),
            "kotlin" => format!("    val local_{index} = {index}\n"),
            _ => format!("    int local_{index} = {index};\n"),
        })
        .collect::<String>();
    wrap_metric_function(language, metric_parameter(language), &body)
}

fn return_metric_source(language: &str, count: usize) -> String {
    let body = (0..count)
        .map(|index| match language {
            "python" => format!("    if value == {index}:\n        return {index}\n"),
            "go" => format!("    if value == {index} {{ return {index} }}\n"),
            "kotlin" => format!("    if (value == {index}) return {index}\n"),
            _ => format!("    if (value == {index}) return {index};\n"),
        })
        .collect::<String>();
    wrap_metric_function(language, metric_parameter(language), &body)
}

fn statement_metric_source(language: &str, count: usize) -> String {
    let body = (0..count)
        .map(|_| match language {
            "python" => "    consume(value)\n",
            _ => "    consume(value);\n",
        })
        .collect::<String>();
    wrap_metric_function(language, metric_parameter(language), &body)
}

fn god_module_metric_source(language: &str, count: usize) -> String {
    let methods = (0..count)
        .map(|index| match language {
            "python" => format!("def operation_{index}():\n    pass\n"),
            "javascript" => format!("function operation_{index}() {{}}\n"),
            "typescript" => format!("function operation_{index}(): void {{}}\n"),
            "go" => format!("func operation_{index}() {{}}\n"),
            "kotlin" => format!("fun operation_{index}() {{}}\n"),
            "java" | "csharp" => format!("    void operation_{index}() {{}}\n"),
            _ => format!("void operation_{index}(void) {{}}\n"),
        })
        .collect::<String>();
    match language {
        "go" => format!("package capability\n{methods}"),
        "java" | "csharp" => format!("class Capability {{\n{methods}}}\n"),
        _ => methods,
    }
}

/// Whether the metric wrapper adds no trailing `return` of its own.
fn implicit_return(language: &str) -> bool {
    matches!(language, "rust" | "python" | "javascript" | "typescript")
}

fn metric_parameter(language: &str) -> &'static str {
    match language {
        "rust" => "value: i32",
        "typescript" => "value: number",
        "go" => "value int",
        "java" | "csharp" | "c" | "cpp" => "int value",
        "kotlin" => "value: Int",
        _ => "value",
    }
}

fn wrap_metric_function(language: &str, parameters: &str, body: &str) -> String {
    match language {
        "rust" => format!("fn inspect({parameters}) {{\n{body}}}\n"),
        "python" => format!("def inspect({parameters}):\n{body}"),
        "javascript" => format!("function inspect({parameters}) {{\n{body}}}\n"),
        "typescript" => format!("function inspect({parameters}): void {{\n{body}}}\n"),
        "go" => format!(
            "package capability\nfunc inspect({parameters}) int {{\n{body}    return 0\n}}\n"
        ),
        "java" => format!(
            "class Capability {{ int inspect({parameters}) {{\n{body}    return 0;\n}} }}\n"
        ),
        "kotlin" => format!("fun inspect({parameters}): Int {{\n{body}    return 0\n}}\n"),
        "csharp" => format!(
            "class Capability {{ int Inspect({parameters}) {{\n{body}    return 0;\n}} }}\n"
        ),
        "c" | "cpp" => format!("int inspect({parameters}) {{\n{body}    return 0;\n}}\n"),
        other => panic!("unsupported metric language {other}"),
    }
}

fn python_capability_files(detector: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let one = |source: String| vec![("src/capability.py".to_string(), source)];
    Some(match detector {
        "too_many_parameters" => one(if positive {
            "def inspect(a, b, c, d, e, f):\n    return a\n"
        } else {
            "def inspect(a, b, c, d, e):\n    return a\n"
        }
        .to_string()),
        "section_divider_comment" => one(if positive {
            "def process(value):\n    # --- validate ---\n    value += 1\n    # --- transform ---\n    value *= 2\n    # --- finalize ---\n    return value\n"
        } else {
            "def process(value):\n    # validate\n    return value\n"
        }
        .to_string()),
        "magic_literal_cluster" => one(if positive {
            "def calculate(value):\n    return value + 11 + 17 + 23 + 29\n"
        } else {
            "def calculate(value):\n    return value + 11 + 17\n"
        }
        .to_string()),
        "god_module" => one(
            (0..if positive { 21 } else { 20 })
                .map(|index| format!("def operation_{index}():\n    pass\n"))
                .collect(),
        ),
        "source_duplicate_detection" => one(if positive {
            "def process_values(items, factor):\n    total = 0\n    for item in items:\n        if item > factor:\n            total += item * factor\n        elif item < 0:\n            total -= abs(item)\n    average = total / max(len(items), 1)\n    if average > factor:\n        return average - factor\n    return average + factor\n\ndef aggregate_scores(entries, multiplier):\n    result = 0\n    for item in entries:\n        if item > multiplier:\n            result += item * multiplier\n        elif item < 0:\n            result -= abs(item)\n    average = result / max(len(entries), 1)\n    if average > multiplier:\n        return average - multiplier\n    return average + multiplier\n"
        } else {
            "def enabled():\n    return True\n\ndef label():\n    return 'distinct'\n"
        }
        .to_string()),
        "package_coupling_metrics" if positive => vec![
            (
                "core/model.py".to_string(),
                "class Model:\n    pass\n".to_string(),
            ),
            (
                "app/service.py".to_string(),
                "from core.model import Model\nclass Service:\n    def load(self) -> Model:\n        return Model()\n"
                    .to_string(),
            ),
            (
                "web/handler.py".to_string(),
                "from core.model import Model\nclass Handler:\n    def load(self) -> Model:\n        return Model()\n"
                    .to_string(),
            ),
        ],
        "package_coupling_metrics" => one("class Model:\n    pass\n".to_string()),
        "churn_complexity_hotspot" => one(if positive {
            "def hotspot(data):\n    if data: return 1\n    if len(data) > 1: return 2\n    if len(data) > 2: return 3\n    if len(data) > 3: return 4\n    if len(data) > 4: return 5\n    if len(data) > 5: return 6\n    if len(data) > 6: return 7\n    if len(data) > 7: return 8\n    if len(data) > 8: return 9\n    if len(data) > 9: return 10\n    if len(data) > 10: return 11\n    return 0\n"
        } else {
            "def hotspot(data):\n    return len(data)\n"
        }
        .to_string()),
        "assert_in_production" => one(if positive {
            "def inspect(value):\n    assert value > 0\n    return value\n"
        } else {
            "def inspect(value):\n    if value <= 0:\n        raise ValueError(value)\n    return value\n"
        }
        .to_string()),
        _ => return None,
    })
}

pub(super) fn native_language(case: &CapabilityCase) -> Option<&'static str> {
    let extension = Path::new(&case.positive.first()?.0).extension()?.to_str()?;
    Some(match extension {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "cs" => "csharp",
        "c" => "c",
        "cpp" => "cpp",
        "rb" => "ruby",
        "jl" => "julia",
        "sh" => "shell",
        "md" => "markdown",
        "json" | "toml" | "yaml" | "yml" => "structured",
        _ => return None,
    })
}

fn retarget_files(files: &[(String, String)], language: &str) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(path, source)| (retarget_path(path, language), source.clone()))
        .collect()
}

fn retarget_path(path: &str, language: &str) -> String {
    let mut target = PathBuf::from(path);
    if Path::new(path).extension().is_some() {
        target.set_extension(extension(language));
    }
    target.to_string_lossy().into_owned()
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
        "ruby" => "rb",
        "julia" => "jl",
        "shell" => "sh",
        "markdown" => "md",
        "structured" => "json",
        other => panic!("uncataloged language {other}"),
    }
}

fn is_tiered_language(language: &str) -> bool {
    matches!(
        language,
        "rust"
            | "python"
            | "javascript"
            | "typescript"
            | "go"
            | "java"
            | "kotlin"
            | "csharp"
            | "c"
            | "cpp"
    )
}

fn is_source_language(language: &str) -> bool {
    !matches!(language, "markdown" | "structured")
}

fn semantic_source(language: &str, detector: &str, positive: bool) -> String {
    let body = match detector {
        "function_complexity" => sequential_branches(language, if positive { 12 } else { 2 }),
        "cognitive_complexity" => nested_branches(language, if positive { 6 } else { 1 }),
        "npath_complexity" => sequential_branches(language, if positive { 12 } else { 2 }),
        "halstead_information_density" => {
            if positive {
                dense_operations(language, 120)
            } else {
                operations(language, 1)
            }
        }
        "unreachable_code" => unreachable_body(language, positive),
        "constant_condition" => constant_condition_body(language, positive),
        "excessive_nesting_depth" => nested_branches(language, if positive { 6 } else { 5 }),
        other => panic!("no semantic source shape for {other}"),
    };
    wrap_function(language, &body)
}

fn unreachable_body(language: &str, positive: bool) -> String {
    match (language, positive) {
        ("python", true) => "    return\n    consume(value)\n".to_string(),
        ("python", false) => "    consume(value)\n    return\n".to_string(),
        ("ruby" | "julia", true) => "  return\n  consume(value)\n".to_string(),
        ("ruby" | "julia", false) => "  consume(value)\n  return\n".to_string(),
        ("shell", true) => "  return 0\n  consume \"$value\"\n".to_string(),
        ("shell", false) => "  consume \"$value\"\n  return 0\n".to_string(),
        (_, true) => "    return;\n    consume(value);\n".to_string(),
        (_, false) => "    consume(value);\n    return;\n".to_string(),
    }
}

fn constant_condition_body(language: &str, positive: bool) -> String {
    match (language, positive) {
        ("python", true) => "    if True:\n        consume(value)\n".to_string(),
        ("python", false) => "    if value:\n        consume(value)\n".to_string(),
        ("ruby" | "julia", true) => "  if true\n    consume(value)\n  end\n".to_string(),
        ("ruby" | "julia", false) => "  if value\n    consume(value)\n  end\n".to_string(),
        ("shell", true) => "  if true; then consume \"$value\"; fi\n".to_string(),
        ("shell", false) => "  if [ \"$value\" -gt 0 ]; then consume \"$value\"; fi\n".to_string(),
        ("c" | "cpp", true) => "    if (1) { consume(value); }\n".to_string(),
        (_, true) => "    if (true) { consume(value); }\n".to_string(),
        (_, false) => "    if (value > 0) { consume(value); }\n".to_string(),
    }
}

fn sequential_branches(language: &str, count: usize) -> String {
    (0..count)
        .map(|index| match language {
            "python" => format!("    if value == {index}:\n        consume({index})\n"),
            "ruby" | "julia" => {
                format!("  if value == {index}\n    consume({index})\n  end\n")
            }
            "shell" => format!("  if [ \"$value\" -eq {index} ]; then consume {index}; fi\n"),
            _ => format!("    if (value == {index}) {{ consume({index}); }}\n"),
        })
        .collect()
}

fn nested_branches(language: &str, count: usize) -> String {
    match language {
        "python" => {
            let mut body = String::new();
            for index in 0..count {
                body.push_str(&format!(
                    "{}if value == {index}:\n",
                    "    ".repeat(index + 1)
                ));
            }
            body.push_str(&format!("{}consume(value)\n", "    ".repeat(count + 1)));
            body
        }
        "ruby" | "julia" => {
            let mut body = String::new();
            for index in 0..count {
                body.push_str(&format!("{}if value == {index}\n", "  ".repeat(index + 1)));
            }
            body.push_str(&format!("{}consume(value)\n", "  ".repeat(count + 1)));
            for index in (0..count).rev() {
                body.push_str(&format!("{}end\n", "  ".repeat(index + 1)));
            }
            body
        }
        "shell" => {
            let mut body = String::new();
            for index in 0..count {
                body.push_str(&format!(
                    "{}if [ \"$value\" -eq {index} ]; then\n",
                    "  ".repeat(index + 1)
                ));
            }
            body.push_str(&format!("{}consume \"$value\"\n", "  ".repeat(count + 1)));
            for index in (0..count).rev() {
                body.push_str(&format!("{}fi\n", "  ".repeat(index + 1)));
            }
            body
        }
        _ => {
            let mut body = String::new();
            for index in 0..count {
                body.push_str(&format!(
                    "{}if (value == {index}) {{\n",
                    "    ".repeat(index + 1)
                ));
            }
            body.push_str(&format!("{}consume(value);\n", "    ".repeat(count + 1)));
            for index in (0..count).rev() {
                body.push_str(&format!("{}}}\n", "    ".repeat(index + 1)));
            }
            body
        }
    }
}

fn operations(language: &str, count: usize) -> String {
    (0..count)
        .map(|index| match language {
            "python" => format!("    consume(value + {index})\n"),
            "ruby" | "julia" => format!("  consume(value + {index})\n"),
            "shell" => format!("  consume \"$((value + {index}))\"\n"),
            _ => format!("    consume(value + {index});\n"),
        })
        .collect()
}

/// Operator-rich statements that reuse operands, so Halstead difficulty (not
/// only volume) exceeds budget.
fn dense_operations(language: &str, count: usize) -> String {
    (0..count)
        .map(|index| {
            let expression = format!(
                "value * value + value * value - value * value + value % ({index} + 1) - value / 3"
            );
            match language {
                "python" => format!("    consume({expression})\n"),
                "ruby" | "julia" => format!("  consume({expression})\n"),
                "shell" => format!("  consume \"$(({expression}))\"\n"),
                _ => format!("    consume({expression});\n"),
            }
        })
        .collect()
}

fn wrap_function(language: &str, body: &str) -> String {
    match language {
        "rust" => format!("fn inspect(value: i32) {{\n{body}}}\n"),
        "python" => format!("def inspect(value):\n{body}"),
        "javascript" => format!("function inspect(value) {{\n{body}}}\n"),
        "typescript" => format!("function inspect(value: number): void {{\n{body}}}\n"),
        "go" => format!("package capability\nfunc inspect(value int) {{\n{body}}}\n"),
        "java" => format!("class Capability {{ void inspect(int value) {{\n{body}}} }}\n"),
        "kotlin" => format!("fun inspect(value: Int) {{\n{body}}}\n"),
        "csharp" => {
            format!("class Capability {{ void Inspect(int value) {{\n{body}}} }}\n")
        }
        "c" => format!("void inspect(int value) {{\n{body}}}\n"),
        "cpp" => format!("void inspect(int value) {{\n{body}}}\n"),
        "ruby" => format!("def inspect(value)\n{body}end\n"),
        "julia" => format!("function inspect(value)\n{body}end\n"),
        "shell" => format!("inspect() {{\n  value=\"$1\"\n{body}}}\n"),
        other => panic!("cannot wrap semantic source for {other}"),
    }
}

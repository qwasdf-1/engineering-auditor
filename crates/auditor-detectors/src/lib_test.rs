use super::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[path = "capability_tests.rs"]
mod capability_contract;
#[path = "finding_anchor_tests.rs"]
mod finding_anchor_contract;
#[path = "test_support.rs"]
mod test_support;

use test_support::{temp_root, write};

#[test]
fn source_discovery_skips_references_by_default() {
    let root = temp_root("discovery");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("references/donor")).unwrap();
    write(&root, "src/lib.rs", "fn main() {}\n");
    write(&root, "references/donor/tool.py", "print('x')\n");

    let files = discover_sources(&root, SourceDiscoveryOptions::default()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("src/lib.rs"));

    let files = discover_sources(
        &root,
        SourceDiscoveryOptions {
            include_references: true,
            method: crate::AnalysisMethod::Auto,
        },
    )
    .unwrap();
    assert_eq!(files.len(), 2);
}

#[test]
fn production_performance_owners_preserve_discovery_but_ignore_nonproduction_roles() {
    let root = temp_root("source-roles");
    let eager = "fn resolve(value: Option<String>) -> String { value.unwrap_or(String::from(\"fallback\")) }\n";
    write(&root, "src/lib.rs", eager);
    write(&root, "tests/performance.rs", eager);
    write(&root, "fixtures/performance.rs", eager);

    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let roles = analysis
        .files
        .iter()
        .map(|file| (file.path.clone(), file.source_role.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(roles.get("src/lib.rs"), Some(&"production"));
    assert_eq!(roles.get("tests/performance.rs"), Some(&"test"));
    assert_eq!(roles.get("fixtures/performance.rs"), Some(&"fixture"));

    let findings = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "eager_fallback_computation")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].primary_path(), "src/lib.rs");
}

#[test]
fn role_views_borrow_sorted_storage_without_changing_public_file_order() {
    let files = vec![
        AuditedFile {
            absolute: PathBuf::from("/repo/src/lib.rs"),
            relative: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            bytes: 1,
            role: SourceRole::Production,
        },
        AuditedFile {
            absolute: PathBuf::from("/repo/tests/lib.rs"),
            relative: PathBuf::from("tests/lib.rs"),
            language: Language::Rust,
            bytes: 1,
            role: SourceRole::Test,
        },
        AuditedFile {
            absolute: PathBuf::from("/repo/references/lib.rs"),
            relative: PathBuf::from("references/lib.rs"),
            language: Language::Rust,
            bytes: 1,
            role: SourceRole::Reference,
        },
        AuditedFile {
            absolute: PathBuf::from("/repo/README.md"),
            relative: PathBuf::from("README.md"),
            language: Language::Markdown,
            bytes: 1,
            role: SourceRole::Documentation,
        },
    ];
    let views = RoleViews::new(&files);
    assert_eq!(views.code.len(), 1);
    assert_eq!(views.test_evolution.len(), 3);
    assert_eq!(views.documentation.len(), 1);
    assert!(std::ptr::eq(views.code.as_ptr(), files.as_ptr()));
    assert!(std::ptr::eq(
        views.documentation.as_ptr(),
        files[3..].as_ptr()
    ));

    let root = temp_root("role-view-order");
    write(&root, "README.md", "# Reference\n");
    write(&root, "src/lib.rs", "fn main() {}\n");
    write(&root, "tests/lib.rs", "#[test] fn works() {}\n");
    let discovered = discover_sources(&root, SourceDiscoveryOptions::default()).unwrap();
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    assert_eq!(
        analysis
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        discovered
            .iter()
            .map(|file| file.path.to_str().unwrap())
            .collect::<Vec<_>>()
    );
}

#[test]
fn sequential_scans_do_not_share_repository_state() {
    let first_root = temp_root("scan-state-a");
    let second_root = temp_root("scan-state-b");
    write(
        &first_root,
        "src/complex.rs",
        "fn complex(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool, g: bool, h: bool, i: bool, j: bool, k: bool) {\n    if a {}\n    if b {}\n    if c {}\n    if d {}\n    if e {}\n    if f {}\n    if g {}\n    if h {}\n    if i {}\n    if j {}\n    if k {}\n}\n",
    );
    write(&second_root, "src/simple.rs", "fn simple() {}\n");

    let scan = |root: &Path| {
        analyze_repository(root, SourceDiscoveryOptions::default(), ScanProfile::Gating).unwrap()
    };
    let first = scan(&first_root);
    let second = scan(&second_root);
    let repeated = scan(&first_root);
    let ids = |analysis: &RepositoryAnalysis| {
        analysis
            .findings
            .iter()
            .map(|finding| finding.id.clone())
            .collect::<Vec<_>>()
    };

    assert!(!first.findings.is_empty());
    assert!(second
        .findings
        .iter()
        .all(|finding| !finding.primary_path().contains("complex.rs")));
    assert_eq!(ids(&first), ids(&repeated));
}

#[test]
fn phase_scoped_index_releases_at_its_last_user() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct DropProbe(Rc<Cell<usize>>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut remaining = 2;
    let mut index = Some(DropProbe(Rc::clone(&drops)));
    release_index(&mut remaining, &mut index);
    assert!(index.is_some());
    assert_eq!(drops.get(), 0);
    release_index(&mut remaining, &mut index);
    assert!(index.is_none());
    assert_eq!(drops.get(), 1);
}

#[test]
fn contextual_roles_resolve_test_references_without_authorizing_test_findings() {
    let unreferenced = temp_root("context-role-unreferenced");
    write(
        &unreferenced,
        "src/lib.rs",
        "fn private_helper() -> usize { 1 }\n",
    );
    let analysis = analyze_repository(
        &unreferenced,
        SourceDiscoveryOptions::default(),
        ScanProfile::Full,
    )
    .unwrap();
    assert!(analysis
        .findings
        .iter()
        .any(|finding| finding.detector == "dead_code"));

    let referenced = temp_root("context-role-referenced");
    write(
        &referenced,
        "src/lib.rs",
        "fn private_helper() -> usize { 1 }\n",
    );
    write(
        &referenced,
        "tests/usage.rs",
        "fn test_only_helper() {}\n#[test]\nfn uses_private_helper() { let _ = private_helper(); }\n",
    );
    let analysis = analyze_repository(
        &referenced,
        SourceDiscoveryOptions::default(),
        ScanProfile::Full,
    )
    .unwrap();
    assert!(analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "dead_code")
        .all(|finding| finding.primary_path() != "tests/usage.rs"));
    assert!(!analysis.findings.iter().any(|finding| {
        finding.detector == "dead_code" && finding.primary_path() == "src/lib.rs"
    }));
}

#[test]
fn repository_detects_promoted_structural_rules() {
    let root = temp_root("parity");
    write(
        &root,
        "src/sample.py",
        r#"
def analyze_user_status(mode: str, alpha, beta, gamma, delta, epsilon, zeta):
    import os
    very_descriptive_scope_specific_identifier = alpha
    another_extremely_verbose_contextual_name = very_descriptive_scope_specific_identifier
    if mode == "fast":
        return alpha
    if mode == "safe":
        return beta
    if alpha:
        if beta:
            if gamma:
                return mode
    return os.name

def terse():
    aa = bb = cc = dd = ee = ff = 3
    return aa + bb + cc + dd + ee + ff

def verbose_names():
    customer_account_processing_context = 1
    customer_account_processing_result = customer_account_processing_context
    customer_account_processing_projection = customer_account_processing_result
    return customer_account_processing_projection

def helper_one(): pass
def helper_two(): pass
def helper_three(): pass

def sibling_a():
    helper_one(); helper_two(); helper_three()

def sibling_b():
    helper_one(); helper_two(); helper_three()
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let detectors: BTreeSet<_> = analysis
        .findings
        .iter()
        .map(|finding| finding.detector.as_str())
        .collect();

    assert!(detectors.contains("too_many_parameters"));
    assert!(detectors.contains("identifier_verbosity"));
    assert!(detectors.contains("identifier_tersity"));
    assert!(detectors.contains("stringly_typed_parameter"));
    assert!(detectors.contains("function_local_import"));
    assert!(detectors.contains("sibling_call_redundancy"));
}

#[test]
fn repository_detects_systems_economics_patterns() {
    let root = temp_root("systems");
    write(
        &root,
        "src/systems.rs",
        r#"
fn rewrite(paths: Vec<PathBuf>) {
    for path in paths {
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::write("report.json", raw).unwrap();
    }
}

fn update_cache(cache: &mut Vec<Entry>, entry: Entry) {
    cache.push(entry);
    let latest = cache.last().unwrap();
    println!("{:?}", latest);
}

fn process(items: Vec<String>) {
    for item in items {
        let parsed = serde_json::from_str(&item).unwrap();
        scan(parsed);
        let encoded = serde_json::to_string(&parsed).unwrap();
        write(encoded);
    }
}

fn handle_request(ids: Vec<String>, wanted: Vec<String>) {
    for id in ids {
        if wanted.contains(&id) {
            println!("{}", id);
        }
    }
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let detectors: BTreeSet<_> = analysis
        .findings
        .iter()
        .map(|finding| finding.detector.as_str())
        .collect();

    assert!(detectors.contains("loop_artifact_rewrite"));
    assert!(detectors.contains("latest_only_unbounded_cache"));
    assert!(detectors.contains("repeated_parse_scan_serialize"));
    assert!(detectors.contains("hot_path_linear_membership"));
}

#[test]
fn repository_detects_core_correctness_patterns() {
    let root = temp_root("l1");
    write(
        &root,
        "src/main.rs",
        r#"
fn main() {
    panic!("not yet");
    todo!("implement this");
}
"#,
    );
    write(&root, "src/check.py", "def check(x):\n    assert x > 0\n");
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let detectors: BTreeSet<_> = analysis
        .findings
        .iter()
        .map(|finding| finding.detector.as_str())
        .collect();
    assert!(detectors.contains("unhandled_termination_in_production"));
    assert!(detectors.contains("todo_unimplemented_in_production"));
    assert!(detectors.contains("assert_in_production"));
}

#[test]
fn repository_detects_algorithmic_and_optimization_patterns() {
    let root = temp_root("alg");
    write(
        &root,
        "src/work.rs",
        r#"
fn compare(items: Vec<Item>) {
    for left in &items {
        for right in &items {
            if left.id == right.id { consume(left, right); }
        }
    }
}

fn rank(requests: Vec<Request>, mut candidates: Vec<i32>) {
    for request in requests {
        candidates.sort();
        consume(request, &candidates);
    }
}

fn render(items: Vec<i32>, mode: &str) {
    for item in items {
        consume(item, format!("mode={mode}"));
    }
}
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let detectors: BTreeSet<_> = analysis
        .findings
        .iter()
        .map(|finding| finding.detector.as_str())
        .collect();
    assert!(detectors.contains("nested_quadratic_iteration"));
    assert!(detectors.contains("repeated_sort_in_loop"));
    assert!(detectors.contains("repeated_format_allocation"));
}

#[test]
fn eval_in_production_detects_eval_and_exec() {
    let root = temp_root("eval");
    write(
        &root,
        "src/main.py",
        r#"
x = eval("1 + 2")
exec("print('ready')")
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let eval_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "eval_in_production")
        .count();
    assert_eq!(
        eval_count, 2,
        "expected 2 eval/exec findings, got {eval_count}"
    );
}

#[test]
fn eval_in_production_skips_comments() {
    let root = temp_root("eval_comment");
    write(
        &root,
        "src/main.rs",
        r#"
fn main() {
    // eval("dangerous") — should not be detected
    let x = 1;
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let eval_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "eval_in_production")
        .count();
    assert_eq!(
        eval_count, 0,
        "expected 0 eval findings from comment-only, got {eval_count}"
    );
}

#[test]
fn cache_miss_detects_above_threshold() {
    let root = temp_root("cache_pos");
    write(
        &root,
        "src/lib.rs",
        r#"
fn process(data: &str) -> bool {
    data.to_ascii_lowercase() == data.to_ascii_lowercase()
        && data.to_ascii_lowercase().is_empty()
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let cache_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "cache_miss_opportunity")
        .count();
    assert_eq!(
        cache_count, 1,
        "expected 1 cache_miss finding (3 lowercase calls), got {cache_count}"
    );
}

#[test]
fn cache_miss_silent_below_threshold() {
    let root = temp_root("cache_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn process(data: &[u8]) -> bool {
    data.contains(&1) && data.len() > 0
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let cache_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "cache_miss_opportunity")
        .count();
    assert_eq!(
        cache_count, 0,
        "expected 0 cache_miss findings (2 calls only), got {cache_count}"
    );
}

#[test]
fn function_complexity_above_threshold() {
    let root = temp_root("complex_pos");
    write(
        &root,
        "src/lib.rs",
        r#"
fn validate(input: &str) -> bool {
    if input.is_empty() { return false; }
    if input.len() > 100 { return false; }
    if input.contains('\n') { return false; }
    if input.contains('\t') { return false; }
    if input.contains('\0') { return false; }
    if input.starts_with('/') { return false; }
    if input.ends_with('/') { return false; }
    if input.contains("..") { return false; }
    if input.contains("//") { return false; }
    if input.contains("{{") { return false; }
    true
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions {
            include_references: false,
            method: crate::AnalysisMethod::Token,
        },
        ScanProfile::Gating,
    )
    .unwrap();
    let fc_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "function_complexity")
        .count();
    assert_eq!(
        fc_count, 1,
        "expected 1 function_complexity finding (10+ branches), got {fc_count}"
    );
}

#[test]
fn function_complexity_below_threshold() {
    let root = temp_root("complex_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn is_valid(input: &str) -> bool {
    !input.is_empty() && input.len() < 100
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions {
            include_references: false,
            method: crate::AnalysisMethod::Token,
        },
        ScanProfile::Gating,
    )
    .unwrap();
    let fc_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "function_complexity")
        .count();
    assert_eq!(
        fc_count, 0,
        "expected 0 function_complexity findings (simple fn), got {fc_count}"
    );
}

#[test]
fn deep_nesting_triggers_cognitive_complexity_via_ast() {
    let root = temp_root("cog_deep_ast");
    write(
        &root,
        "src/lib.rs",
        r#"
fn main() {
    if a {
        if b {
            if c {
                if d {
                    if e {
                        if f {
                            println!("deep");
                        }
                    }
                }
            }
        }
    }
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions {
            include_references: false,
            method: crate::AnalysisMethod::Auto,
        },
        ScanProfile::Full,
    )
    .unwrap();
    let cog_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "cognitive_complexity")
        .count();
    assert_eq!(
        cog_count, 1,
        "expected 1 cognitive_complexity finding via AST (deep nesting), got {cog_count}"
    );
}

#[test]
fn trivial_function_has_no_cognitive_complexity_via_ast() {
    let root = temp_root("cog_triv_ast");
    write(
        &root,
        "src/lib.rs",
        r#"
fn identity(x: usize) -> usize { x }
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions {
            include_references: false,
            method: crate::AnalysisMethod::Auto,
        },
        ScanProfile::Full,
    )
    .unwrap();
    let cog_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "cognitive_complexity")
        .count();
    assert_eq!(
        cog_count, 0,
        "expected 0 cognitive_complexity findings via AST (trivial fn), got {cog_count}"
    );
}

#[test]
fn identifier_stutter_detects_scope_repetition() {
    let root = temp_root("stutter_pos");
    write(&root, "src/lib.rs", "pub mod pdf_document;\n");
    write(
        &root,
        "src/pdf_document.rs",
        r#"
pub fn pdf_document_bytes() {
    let pdf_document_size = 1;
}
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let stutter_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "identifier_stutter")
        .count();
    assert_eq!(
        stutter_count, 1,
        "expected 1 stutter finding (pdf_document::pdf_document_bytes), got {stutter_count}"
    );
}

#[test]
fn identifier_stutter_ignores_unclaimed_languages() {
    let root = temp_root("stutter_signature_neg");
    write(
        &root,
        "src/full_screen.ts",
        "export interface FullScreenState {}\nexport function createFullScreenState(options: FullScreenState): FullScreenState { return options }\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(
        !analysis
            .findings
            .iter()
            .any(|finding| finding.detector == "identifier_stutter"),
        "TypeScript is outside the detector's languages"
    );
}

#[test]
fn identifier_stutter_ignores_private_items() {
    let root = temp_root("stutter_nonlocal_neg");
    write(
        &root,
        "src/slice.rs",
        r#"
const SLICE_EVIDENCE_LIMIT: usize = 8;
struct SliceEvidenceRecord;

fn slice_evidence() {
    let record = SliceEvidenceRecord;
    emit_slice_evidence(record, SLICE_EVIDENCE_LIMIT);
}
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(
        !analysis
            .findings
            .iter()
            .any(|finding| finding.detector == "identifier_stutter"),
        "type, constant, and callee references are not local-name stutter"
    );
}

#[test]
fn repository_applies_slop_compat_stutter_threshold() {
    let root = temp_root("stutter_slop_config");
    write(
        &root,
        ".slop.toml",
        "[rules.lexical.stutter]\nmin_overlap_tokens = 5\n",
    );
    write(
        &root,
        "src/lib.rs",
        "fn process_pdf_document() {\n    let pdf_document_bytes = 1;\n}\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(
        !analysis
            .findings
            .iter()
            .any(|finding| finding.detector == "identifier_stutter"),
        "repository donor thresholds must suppress below-threshold findings"
    );
}

#[test]
fn identifier_stutter_skips_distinct_names() {
    let root = temp_root("stutter_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn validate() {
    let active = Vec::new();
}
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let stutter_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "identifier_stutter")
        .count();
    assert_eq!(
        stutter_count, 0,
        "expected 0 stutter findings (distinct names), got {stutter_count}"
    );
}

#[test]
fn repeated_sort_reports_loop_calls_that_receive_the_collection() {
    let root = temp_root("sort_argument_calls");
    write(
        &root,
        "src/lib.rs",
        r#"
pub fn insert_and_order(values: &mut Vec<i32>, additions: &[i32]) -> Vec<bool> {
    fn append_for_next_iteration(values: &mut Vec<i32>, addition: i32) {
        values.push(addition);
    }

    let mut previously_present = Vec::with_capacity(additions.len());
    for addition in additions {
        values.sort();
        previously_present.push(values.binary_search(addition).is_ok());
        append_for_next_iteration(values, *addition);
    }
    previously_present
}
"#,
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let finding = analysis
        .findings
        .iter()
        .find(|f| f.detector == "repeated_sort_in_loop")
        .expect("the sort still repeats inside the loop");
    let evidence = &finding.evidence;
    assert_eq!(
        evidence.get("direct_mutation_count"),
        Some(&auditor_core::EvidenceValue::Integer(0))
    );
    assert!(!evidence.contains_key("mutation_count"));
    assert_eq!(
        evidence.get("calls_receiving_collection"),
        Some(&auditor_core::EvidenceValue::StringList(vec![
            "append_for_next_iteration(values, *addition)".to_string()
        ]))
    );
}

#[test]
fn repeated_sort_outside_loop_is_silent() {
    let root = temp_root("sort_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn process_items(mut items: Vec<i32>) -> Vec<i32> {
    items.sort();
    for item in &items {
        println!("{item}");
    }
    items
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let sort_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "repeated_sort_in_loop")
        .count();
    assert_eq!(
        sort_count, 0,
        "expected 0 sort-in-loop findings (sort outside loop), got {sort_count}"
    );
}

#[test]
fn nested_loop_single_level_is_silent() {
    let root = temp_root("nested_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn process_items(items: Vec<i32>) {
    for item in items {
        println!("{item}");
    }
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let nested_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "nested_quadratic_iteration")
        .count();
    assert_eq!(
        nested_count, 0,
        "expected 0 nested-loop findings (single level), got {nested_count}"
    );
}

#[test]
fn nested_loops_see_through_transparent_wrappers() {
    let root = temp_root("nested_wrappers");
    write(
        &root,
        "src/report.py",
        "def distinct(repos, added):\n\
         \x20   for repo in sorted(repos):\n\
         \x20       for finding in sorted(added):\n\
         \x20           print(repo, finding)\n\
         \n\
         def union(before, after, items):\n\
         \x20   for key in sorted(set(before) | set(after)):\n\
         \x20       for item in sorted(items):\n\
         \x20           print(key, item)\n\
         \n\
         def pairs(items):\n\
         \x20   for left in sorted(items):\n\
         \x20       for right in enumerate(items, 1):\n\
         \x20           print(left, right)\n\
         \n\
         def grid(height, width):\n\
         \x20   for y in range(0, height):\n\
         \x20       for x in range(width):\n\
         \x20           print(x, y)\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let functions = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "nested_quadratic_iteration")
        .map(|finding| finding.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 1, "{functions:?}");
    assert!(functions[0].contains("`pairs`") && functions[0].contains("`items`"));
}

#[test]
fn all_detectors_have_layer() {
    for profile in &[ScanProfile::Gating, ScanProfile::Full] {
        for detector in registry::detector_registry(*profile) {
            assert!(
                !detector.layer.is_empty(),
                "detector {} has empty layer",
                detector.id
            );
            assert!(
                detector.layer.starts_with('L'),
                "detector {} has unexpected layer format: {}",
                detector.id,
                detector.layer
            );
        }
    }
}

#[test]
fn format_allocation_outside_loop_is_silent() {
    let root = temp_root("fmt_neg");
    write(
        &root,
        "src/lib.rs",
        r#"
fn report(item: &str) -> String {
    format!("item: {item}")
}
"#,
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let fmt_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "repeated_format_allocation")
        .count();
    assert_eq!(
        fmt_count, 0,
        "expected 0 format-allocation findings (format! outside loop), got {fmt_count}"
    );
}

#[test]
fn path_exclusions_come_from_repository_config() {
    let root = temp_root("self_det");
    write(
        &root,
        "crates/auditor-detectors/src/lib.rs",
        r#"
fn main() {
    panic!("uh oh");
    todo!("do it later");
}
"#,
    );
    let detectors = [
        "unhandled_termination_in_production",
        "todo_unimplemented_in_production",
    ];
    let counts = |root: &Path| {
        let analysis =
            analyze_repository(root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
        detectors.map(|det| {
            analysis
                .findings
                .iter()
                .filter(|f| f.detector == det)
                .count()
        })
    };
    // No path is exempt by default, whatever the repository looks like.
    assert!(
        counts(&root).iter().all(|count| *count > 0),
        "{:?}",
        counts(&root)
    );
    write(
        &root,
        "auditor.toml",
        &format!(
            "[exclusions.detector-patterns]\npaths = crates/auditor-detectors/\ndetectors = {}\n",
            detectors.join(", ")
        ),
    );
    assert_eq!(counts(&root), [0, 0]);
}

#[test]
fn shell_function_detection_with_function_keyword() {
    let root = temp_root("shell_func");
    write(
        &root,
        "script.sh",
        "#!/bin/bash\nfunction process_data() {\n  echo \"working\"\n}\nprocess_data\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(!analysis.files.is_empty(), "should discover shell file");
}

#[test]
fn markdown_stale_link_is_detected() {
    let root = temp_root("md_link_pos");
    write(
        &root,
        "README.md",
        "See [details](docs/missing-page.md) for more info.\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let stale_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "stale_link_reference")
        .count();
    assert_eq!(
        stale_count, 1,
        "expected 1 stale link finding, got {stale_count}"
    );
}

#[test]
fn markdown_valid_link_is_silent() {
    let root = temp_root("md_link_neg");
    write(&root, "src/helper.rs", "fn help() {}\n");
    write(&root, "docs/SETUP.md", "# Setup\n");
    write(
        &root,
        "README.md",
        "See [code](src/helper.rs) for the helper.\n",
    );
    write(
        &root,
        "docs/README.md",
        "See [setup](SETUP.md) and [code](../src/helper.rs).\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let stale_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "stale_link_reference")
        .count();
    assert_eq!(
        stale_count, 0,
        "expected 0 stale findings for valid link, got {stale_count}"
    );
}

#[test]
fn markdown_nested_relative_anchor_is_resolved_from_referrer() {
    let root = temp_root("md_nested_anchor_neg");
    write(&root, "docs/guide.md", "## Getting Started\n");
    write(
        &root,
        "docs/rules/README.md",
        "See [guide](../guide.md#getting-started).\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    assert!(
        analysis.findings.iter().all(|finding| {
            !matches!(
                finding.detector.as_str(),
                "stale_link_reference" | "stale_section_anchor"
            )
        }),
        "nested links and anchors must resolve from the referring document"
    );
}

#[test]
fn architectural_layering_fires_on_violation() {
    let root = temp_root("arch_violation");
    write(
        &root,
        "src/domain/user.rs",
        "use crate::app::UserService;\nfn check() {}\n",
    );
    write(&root, "src/app/mod.rs", "pub struct UserService;\n");
    write(
        &root,
        "auditor.toml",
        "[layering.layer.domain]\nglob = src/domain/**/*.rs\n\n[layering.layer.app]\nglob = src/app/**/*.rs\n\n[layering.rule.domain]\ndeny = app\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let arch_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "architectural_layering")
        .count();
    assert_eq!(
        arch_count, 1,
        "expected 1 architectural_layering finding, got {arch_count}"
    );
}

#[test]
fn java_import_used_only_in_javadoc_is_not_a_dependency() {
    let count = |body: &str| {
        let root = temp_root("arch_javadoc");
        write(
            &root,
            "src/main/java/org/acme/domain/Check.java",
            &format!("package org.acme.domain;\n\nimport org.acme.app.UserService;\n\n{body}\n"),
        );
        write(
            &root,
            "src/main/java/org/acme/app/UserService.java",
            "package org.acme.app;\n\npublic class UserService {}\n",
        );
        write(
            &root,
            "auditor.toml",
            "[layering.layer.domain]\nglob = src/main/java/org/acme/domain/**/*.java\n\n[layering.layer.app]\nglob = src/main/java/org/acme/app/**/*.java\n\n[layering.rule.domain]\ndeny = app\n",
        );
        analyze_repository(
            &root,
            SourceDiscoveryOptions::default(),
            ScanProfile::Gating,
        )
        .unwrap()
        .findings
        .iter()
        .filter(|f| f.detector == "architectural_layering")
        .count()
    };
    assert_eq!(
        count("/** See {@link UserService}. */\npublic class Check {}"),
        0
    );
    assert_eq!(
        count("public class Check {\n    Object check() { return new UserService(); }\n}"),
        1
    );
}

#[test]
fn shell_classic_syntax_detected() {
    let root = temp_root("shell_classic");
    write(
        &root,
        "script.sh",
        "#!/bin/bash\nprocess_data() {\n  echo \"working\"\n}\nprocess_data\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(!analysis.files.is_empty(), "should discover shell file");
}

#[test]
fn markdown_section_anchor_missing_is_detected() {
    let root = temp_root("md_section_pos");
    fs::create_dir_all(root.join("docs")).unwrap();
    write(
        &root,
        "docs/intro.md",
        "## Getting Started\n\nWelcome!\n## Advanced\n\nDetails.\n",
    );
    write(
        &root,
        "README.md",
        "See [details](docs/intro.md#missing-section) for more info.\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let stale_count = analysis
        .findings
        .iter()
        .filter(|f| {
            f.detector == "stale_section_anchor"
                && f.title == "Markdown section anchor does not resolve"
        })
        .count();
    assert_eq!(
        stale_count, 1,
        "expected 1 stale section anchor finding, got {stale_count}"
    );
}

#[test]
fn markdown_valid_section_anchor_is_silent() {
    let root = temp_root("md_section_neg");
    fs::create_dir_all(root.join("docs")).unwrap();
    write(&root, "docs/intro.md", "## Getting Started\n\nWelcome!\n");
    write(
        &root,
        "README.md",
        "See [getting started](docs/intro.md#getting-started) for details.\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let stale_count = analysis
        .findings
        .iter()
        .filter(|f| {
            f.detector == "stale_section_anchor"
                && f.title == "Markdown section anchor does not resolve"
        })
        .count();
    assert_eq!(
        stale_count, 0,
        "expected 0 stale section anchor findings for valid anchor, got {stale_count}"
    );
}

#[test]
fn backtick_stale_reference_is_detected() {
    let root = temp_root("btick_stale");
    fs::create_dir_all(root.join("docs")).unwrap();
    write(
        &root,
        "README.md",
        "See `docs/missing-ref.md` for details.\n",
    );
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let stale_count = analysis
        .findings
        .iter()
        .filter(|f| {
            f.detector == "stale_path_reference"
                && f.title == "Backticked path reference does not resolve"
        })
        .count();
    assert_eq!(
        stale_count, 1,
        "expected 1 backtick stale ref finding, got {stale_count}"
    );
}

#[test]
fn duplicate_tokens_detects_renamed_variables() {
    let root = temp_root("dup_rename");
    fs::create_dir_all(root.join("src")).unwrap();
    for (name, var_a, var_b) in &[
        ("alpha", "sum", "acc"),
        ("beta", "total", "running"),
        ("gamma", "count", "accum"),
        ("delta", "value", "cache"),
        ("epsilon", "score", "memo"),
    ] {
        write(
            &root,
            &format!("src/{name}.rs"),
            &format!(
                r#"
pub fn process_{name}(input: Vec<i32>) -> i32 {{
    let mut {var_a} = 0;
    let mut {var_b} = 0;
    for x in input {{
        let scaled = x * 2;
        {var_a} = {var_a}.wrapping_add(scaled);
        {var_b} = {var_b}.wrapping_add(x);
    }}
    let result = {var_a}.wrapping_add({var_b});
    if result > 100 {{
        return result;
    }}
    result
}}
"#,
            ),
        );
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let dup_count = analysis
        .findings
        .iter()
        .filter(|f| f.detector == "source_duplicate_detection")
        .count();
    assert_eq!(
        dup_count, 1,
        "expected 1 duplicate detection (renamed variables), got {dup_count}"
    );
}

#[test]
fn distinct_blocks_are_not_duplicates() {
    distinct_blocks_are_not_duplicates_case();
}

include!("lib_test_metrics.rs");
include!("lib_test_go_api.rs");
include!("lib_test_java_api.rs");
include!("lib_test_js_api.rs");
include!("lib_test_python_api.rs");
include!("lib_test_rust_api.rs");

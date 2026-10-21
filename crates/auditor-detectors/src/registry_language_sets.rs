//! The language and detector-ID sets the support policies select from.

pub(super) const SOURCE_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
    "ruby",
    "julia",
    "shell",
];

pub(super) const CATALOG_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
    "ruby",
    "julia",
    "shell",
    "markdown",
    "structured",
];

pub(super) const RUST: &[&str] = &["rust"];
pub(super) const TIER_ONE: &[&str] = &["rust", "python", "javascript", "typescript", "go", "java"];
/// Tier 1 and C#, whose collection, loop, and LINQ facts the algorithmic
/// index reads.
pub(super) const TIER_ONE_CSHARP: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
];
/// Tier 1, C#, and C++, whose STL algorithm and container facts the
/// algorithmic index reads as well.
pub(super) const TIER_ONE_CSHARP_CPP: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
    "cpp",
];
/// Tier-1 and Tier-2 source languages.
pub(super) const TIERED: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
];
pub(super) const RUST_PYTHON: &[&str] = &["rust", "python"];
pub(super) const MANAGED_LOOP_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "cpp",
];
pub(super) const RECURSIVE_LANGUAGES: &[&str] = TIER_ONE_CSHARP_CPP;
pub(super) const GOD_MODULE_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "kotlin",
    "c",
    "cpp",
];
pub(super) const LOCAL_METRIC_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
    "kotlin",
    "c",
    "cpp",
];
pub(super) const STATEMENT_METRIC_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
    "c",
    "cpp",
    "kotlin",
];
pub(super) const PYTHON: &[&str] = &["python"];
pub(super) const JAVASCRIPT_TYPESCRIPT: &[&str] = &["javascript", "typescript"];
pub(super) const MARKDOWN: &[&str] = &["markdown"];

pub(super) const DOCUMENTATION_IDS: &[&str] = &[
    "stale_path_reference",
    "stale_link_reference",
    "stale_section_anchor",
];
pub(super) const REPOSITORY_METADATA_IDS: &[&str] = &[
    "tooling_overlap",
    "repo_graph_isolated_source_file",
    "repo_graph_high_fanout_source_file",
    "repo_graph_import_cycle",
    "shotgun_surgery_cluster",
    "large_file_split_candidate",
    "small_file_consolidation_candidate",
];
pub(super) const SEMANTIC_IDS: &[&str] = &["function_complexity", "cognitive_complexity"];
pub(super) const NPATH_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
    "c",
    "cpp",
    "kotlin",
];
pub(super) const HALSTEAD_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "csharp",
    "c",
    "cpp",
    "ruby",
    "julia",
    "kotlin",
];
pub(super) const FLOW_LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
];
pub(super) const PYTHON_IDS: &[&str] = &["try_except_pass", "flask_debug_true"];
pub(super) const LITERAL_SECURITY_SEMANTIC_IDS: &[&str] =
    &["hardcoded_credential", "hardcoded_tmp_directory"];
/// Rules resolved through API identity in every Tier-1 and Tier-2 language.
pub(super) const API_IDENTITY_SEMANTIC_IDS: &[&str] = &[
    "bind_all_interfaces",
    "world_writable_file",
    "random_for_security",
    "request_no_cert_verify",
    "ssl_with_bad_defaults",
    "ssh_no_host_key_verification",
    "shell_execution",
    "insecure_cryptography",
    "console_output_in_production",
];
/// Rules every Tier-1 and Tier-2 language resolves.
pub(super) const TIER_ONE_CORRECTNESS_SEMANTIC_IDS: &[&str] =
    &["todo_unimplemented_in_production", "exit_in_production"];
pub(super) const RUST_CORRECTNESS_SEMANTIC_IDS: &[&str] =
    &["unhandled_termination_in_production", "dead_code"];
/// Assertions a release run strips (`python -O`, the JVM without `-ea`, C#
/// release builds).
pub(super) const PYTHON_JAVA_CORRECTNESS_SEMANTIC_IDS: &[&str] = &["assert_in_production"];
pub(super) const PYTHON_JS_CORRECTNESS_SEMANTIC_IDS: &[&str] = &["empty_catch_block"];
/// Statements whose shape the brace languages share with Rust.
pub(super) const BRACE_CORRECTNESS_SEMANTIC_IDS: &[&str] = &["assignment_in_operand"];
/// Statement shapes Rust has as well (`dbg!` is Rust's debugger leftover).
pub(super) const RUST_PYTHON_JS_CORRECTNESS_SEMANTIC_IDS: &[&str] = &["debugger_in_production"];
pub(super) const KOTLIN_IDS: &[&str] = &[
    "kotlin_empty_when",
    "kt_non_null_assertion",
    "kt_require_check_misuse",
    "unsafe_type_cast",
];
/// Test-quality rules over the test-scope facts (test functions, assertion
/// calls, mock stubs) of the Tier-1 frameworks.
pub(super) const TEST_QUALITY_IDS: &[&str] = &["test_without_assertion", "tautological_assertion"];
/// Network clients whose library default never times out, per the Tier-1
/// client API table.
pub(super) const NETWORK_TIMEOUT_IDS: &[&str] = &["network_call_without_timeout"];
/// Front removal from an array-backed sequence in a loop.
pub(super) const FRONT_REMOVAL_IDS: &[&str] = &["quadratic_front_removal"];
/// Regex literals read by a backtracking engine.
pub(super) const REGEX_BACKTRACKING_IDS: &[&str] = &["catastrophic_backtracking_regex"];
/// Discarded failure results, floating promises, and unawaited coroutines.
pub(super) const DISCARDED_RESULT_IDS: &[&str] = &["discarded_fallible_result"];
/// Manifest dependencies against source imports; findings sit on the
/// manifest line and name a package source file for its language.
pub(super) const UNUSED_DEPENDENCY_IDS: &[&str] = &["unused_declared_dependency"];
pub(super) const GO_IDS: &[&str] = &["go_log_fatal", "go_errcheck", "go_repeated_string_literal"];
pub(super) const RUST_ONLY_IDS: &[&str] = &[];
pub(super) const RUST_PYTHON_PARTIAL_IDS: &[&str] = &["churn_complexity_hotspot"];
pub(super) const SYSTEMS_SEMANTIC_IDS: &[&str] = &["blocking_io_in_async_context"];
/// Systems effects that Go, Java, and C# resolve as well.
pub(super) const SYSTEMS_EFFECT_IDS: &[&str] = &[
    "latest_only_unbounded_cache",
    "loop_artifact_rewrite",
    "repeated_parse_scan_serialize",
    "per_item_external_io",
    "repeated_expensive_initialization",
    "request_path_repository_scan",
];
/// Import topology that Go and Java resolve as well.
pub(super) const SYSTEMS_TOPOLOGY_IDS: &[&str] =
    &["architectural_layering", "package_coupling_metrics"];
pub(super) const ALGORITHMIC_IDS: &[&str] = &[
    "repeated_sort_in_loop",
    "sort_for_extremum",
    "repeated_collection_traversal",
    "hot_path_linear_membership",
];
pub(super) const ALGORITHMIC_REASON_IDS: &[&str] = &[
    "nested_quadratic_iteration",
    "repeated_sort_in_loop",
    "sort_for_extremum",
    "full_sort_for_top_k",
    "repeated_collection_traversal",
    "hot_path_linear_membership",
    "n_plus_one_query_in_loop",
    "loop_carried_copy_accumulation",
    "invariant_collection_copy_in_loop",
    "repeated_recursive_subproblem",
    "nested_cross_collection_traversal",
];
pub(super) const OPTIMIZATION_IDS: &[&str] = &[
    "cache_miss_opportunity",
    "repeated_format_allocation",
    "loop_invariant_regex_compilation",
    "immutable_string_concatenation_in_loop",
    "manual_collection_copy_loop",
    "eager_fallback_computation",
    "intermediate_collection_materialization",
    "range_for_each_iteration",
    "loop_invariant_computation",
    "known_size_preallocation",
    "bulk_collection_extension",
    "map_entry_opportunity",
];
pub(super) const FLOW_METRIC_IDS: &[&str] = &[
    "too_many_parameters",
    "too_many_locals",
    "too_many_returns",
    "too_many_statements",
    "god_module",
];
pub(super) const SECURITY_ID_MARKERS: &[&str] = &[
    "credential",
    "cryptography",
    "ssl",
    "ssh",
    "injection",
    "execution",
];

# Language Support Reference

This matrix is generated from the executable detector catalog. The Rust registry
owns detector identity, emission and context roles, and every detector-language
support mode; this document does not apply layer-wide defaults.

Support modes:

- `semantic`: grammar-backed semantic facts are part of the capability.
- `exact_lexical`: a bounded language-aware lexical rule is the capability.
- `partial`: a tested heuristic exists, with a declared limitation.
- `unsupported`: no executable capability is claimed for the pair.
- `not_applicable`: the capability does not exist for the language; each cell carries a reviewed reason.

Language tiers:

| Tier | Languages | Promise |
|---|---|---|
| 1 | Rust, Python, JavaScript, TypeScript, Go, Java | Every applicable detector family is supported or carries a reviewed limitation; precision is measured on the ecosystem corpus. |
| 2 | Kotlin, C#, C, C++ | Layers 1 and 2 at the Tier-1 standard; Layers 3 to 5 are best-effort and documented per cell. |
| 3 | Ruby, Julia, Shell | Structural metrics only (complexity, Halstead density, file size, and Ruby `!!`); no other capability is claimed. |

The catalog is available as JSON with `auditor catalog --profile full --format json`.
Regenerate this page with `python3 tooling/doc_hygiene/generate_language_support.py`.

## Coverage Matrix

### Layer 1 - Correctness

Correctness and security defects.

| Detector | Rust | Python | JavaScript | TypeScript | Go | Java | Kotlin | C# | C | C++ | Ruby | Julia | Shell | Markdown | Structured |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `assert_in_production` | not_applicable | semantic | not_applicable | not_applicable | not_applicable | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `assignment_in_operand` | semantic | not_applicable | semantic | semantic | not_applicable | semantic | not_applicable | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `bind_all_interfaces` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `console_output_in_production` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `constant_condition` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `cpp_raw_ptr_ownership` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `dead_code` | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `debugger_in_production` | semantic | semantic | semantic | semantic | not_applicable | not_applicable | not_applicable | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `discarded_fallible_result` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `empty_catch_block` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | not_applicable | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `empty_conditional_branch` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `eval_in_production` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `exit_in_production` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `flask_debug_true` | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `go_errcheck` | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `go_log_fatal` | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `hardcoded_credential` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `hardcoded_tmp_directory` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `insecure_cryptography` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `kotlin_empty_when` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `kt_non_null_assertion` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `no_await_in_loop` | semantic | semantic | semantic | semantic | not_applicable | not_applicable | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `random_for_security` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `request_no_cert_verify` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `require_atomic_updates` | not_applicable | not_applicable | exact_lexical | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `resource_not_closed` | not_applicable | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `shell_execution` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `sql_injection` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `ssh_no_host_key_verification` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `ssl_with_bad_defaults` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `todo_unimplemented_in_production` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `try_except_pass` | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `unhandled_termination_in_production` | semantic | not_applicable | not_applicable | not_applicable | unsupported | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `unreachable_code` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `unsafe_deserialization` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `unsafe_type_cast` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `unused_assignment` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `world_writable_file` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `xml_xxe_vulnerable` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |

### Layer 2 - Structural Engineering

Structure, complexity, duplication, and maintainability.

| Detector | Rust | Python | JavaScript | TypeScript | Go | Java | Kotlin | C# | C | C++ | Ruby | Julia | Shell | Markdown | Structured |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `class_structure_metrics` | unsupported | partial | partial | partial | unsupported | partial | partial | partial | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported |
| `cognitive_complexity` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported |
| `collection_parameter_mutation` | not_applicable | partial | unsupported | partial | unsupported | partial | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `csharp_linq_enumeration` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `duplicate_import` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `escape_hatch_type_density` | unsupported | partial | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `excessive_nesting_depth` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `function_complexity` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported |
| `function_local_import` | not_applicable | partial | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `generated_vs_source` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | exact_lexical | unsupported |
| `go_repeated_string_literal` | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `god_module` | semantic | semantic | semantic | semantic | semantic | not_applicable | semantic | unsupported | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `halstead_information_density` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported |
| `identifier_stutter` | semantic | not_applicable | not_applicable | not_applicable | semantic | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `identifier_tersity` | unsupported | partial | unsupported | unsupported | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `identifier_verbosity` | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `kt_require_check_misuse` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `large_file` | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | exact_lexical | unsupported | unsupported |
| `magic_literal_cluster` | partial | partial | partial | partial | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `naming_convention` | partial | partial | partial | partial | partial | partial | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `npath_complexity` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `orphan_symbol` | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `out_parameter` | not_applicable | unsupported | unsupported | unsupported | not_applicable | semantic | unsupported | unsupported | not_applicable | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported |
| `ruby_double_bang` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | unsupported | unsupported |
| `section_divider_comment` | partial | partial | partial | partial | partial | partial | partial | partial | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported |
| `sibling_call_redundancy` | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `source_duplicate_detection` | partial | partial | partial | partial | partial | unsupported | unsupported | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `stale_link_reference` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | exact_lexical | unsupported |
| `stale_path_reference` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | exact_lexical | unsupported |
| `stale_section_anchor` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported | unsupported | exact_lexical | unsupported |
| `stringly_typed_parameter` | unsupported | partial | partial | partial | unsupported | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `tautological_assertion` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `test_without_assertion` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `too_many_locals` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `too_many_parameters` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `too_many_returns` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |
| `too_many_statements` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported |

### Layer 3 - Algorithmic Engineering

Algorithmic cost and repeated-work patterns.

| Detector | Rust | Python | JavaScript | TypeScript | Go | Java | Kotlin | C# | C | C++ | Ruby | Julia | Shell | Markdown | Structured |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `catastrophic_backtracking_regex` | not_applicable | semantic | semantic | semantic | not_applicable | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `full_sort_for_top_k` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `hot_path_linear_membership` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `invariant_collection_copy_in_loop` | semantic | semantic | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `loop_carried_copy_accumulation` | semantic | semantic | semantic | semantic | not_applicable | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `n_plus_one_query_in_loop` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `nested_cross_collection_traversal` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `nested_quadratic_iteration` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `quadratic_front_removal` | semantic | semantic | semantic | semantic | not_applicable | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_collection_traversal` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_recursive_subproblem` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_sort_in_loop` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `sort_for_extremum` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |

### Layer 4 - Systems Engineering

Repository and systems-economics behavior.

| Detector | Rust | Python | JavaScript | TypeScript | Go | Java | Kotlin | C# | C | C++ | Ruby | Julia | Shell | Markdown | Structured |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `architectural_layering` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `blocking_io_in_async_context` | semantic | semantic | semantic | semantic | not_applicable | semantic | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `churn_complexity_hotspot` | partial | partial | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `latest_only_unbounded_cache` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `loop_artifact_rewrite` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `network_call_without_timeout` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `package_coupling_metrics` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `per_item_external_io` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_expensive_initialization` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_parse_scan_serialize` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `request_path_repository_scan` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `unused_declared_dependency` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |

### Layer 5 - Optimization Opportunities

Bounded optimization opportunities.

| Detector | Rust | Python | JavaScript | TypeScript | Go | Java | Kotlin | C# | C | C++ | Ruby | Julia | Shell | Markdown | Structured |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `bulk_collection_extension` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `cache_miss_opportunity` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `eager_fallback_computation` | semantic | not_applicable | not_applicable | not_applicable | not_applicable | semantic | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `immutable_string_concatenation_in_loop` | semantic | semantic | semantic | semantic | semantic | semantic | semantic | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `intermediate_collection_materialization` | semantic | semantic | semantic | semantic | not_applicable | semantic | semantic | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `java_inefficient_string_buffer` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | exact_lexical | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | unsupported | unsupported |
| `known_size_preallocation` | semantic | not_applicable | not_applicable | not_applicable | semantic | semantic | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `loop_invariant_computation` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `loop_invariant_regex_compilation` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `manual_collection_copy_loop` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `map_entry_opportunity` | semantic | semantic | not_applicable | not_applicable | not_applicable | semantic | unsupported | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `range_for_each_iteration` | semantic | not_applicable | not_applicable | not_applicable | not_applicable | semantic | semantic | semantic | not_applicable | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |
| `repeated_format_allocation` | semantic | semantic | semantic | semantic | semantic | semantic | unsupported | semantic | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported |

## Source Coverage Summary

Markdown and structured-data cells are excluded from these source-language totals.

| Layer | Semantic | Exact lexical | Partial | Unsupported | Not applicable |
|---|---:|---:|---:|---:|---:|
| L1 | 244 | 10 | 0 | 121 | 132 |
| L2 | 129 | 17 | 53 | 172 | 110 |
| L3 | 83 | 0 | 0 | 77 | 9 |
| L4 | 72 | 0 | 2 | 80 | 2 |
| L5 | 72 | 1 | 0 | 62 | 34 |

## Not-Applicable Source Capabilities

Each not-applicable source cell carries one of these reviewed reasons.

| Reason | Detectors | Languages |
|---|---|---|
| A Java file's top-level unit is its class, whose size `class_structure_metrics` judges; methods are not module definitions. | `god_module` | Java |
| A JavaScript event-loop race (a read-modify-write split by `await`); concurrency here is governed by threads, locks, or ownership instead. | `require_atomic_updates` | C, C++, C#, Go, Java, Kotlin, Python, Rust |
| A Python construct (Flask's debug server, `except: pass`); the language has no counterpart. | `flask_debug_true`, `try_except_pass` | C, C++, C#, Go, Java, JavaScript, Kotlin, Rust, TypeScript |
| A documentation and path capability: it reads Markdown links, anchors, and generated-file markers, not source code. | `generated_vs_source`, `stale_link_reference`, `stale_path_reference`, `stale_section_anchor` | C, C++, C#, Go, Java, JavaScript, Kotlin, Python, Rust, TypeScript |
| A language-specific or repository-metadata detector: it reads another language's construct, or the repository as a whole. | `cpp_raw_ptr_ownership`, `csharp_linq_enumeration`, `go_errcheck`, `go_log_fatal`, `go_repeated_string_literal`, `java_inefficient_string_buffer`, `kotlin_empty_when`, `kt_non_null_assertion`, `kt_require_check_misuse`, `ruby_double_bang`, `unsafe_type_cast` | C, C++, C#, Go, Java, JavaScript, Julia, Kotlin, Python, Ruby, Rust, Shell, TypeScript |
| A pointer or reference parameter declares the write in the signature, and output pointers are the C convention for multiple results. | `out_parameter` | C, C++ |
| Assignment is not an expression in Go, and Python's walrus operator is the deliberate form of it. | `assignment_in_operand` | Go, Python |
| Assignment is not an expression in Kotlin. | `assignment_in_operand` | Kotlin |
| C has no `await`. | `no_await_in_loop` | C |
| C has no `catch`: errors are return codes. | `empty_catch_block` | C |
| C has no async functions. | `blocking_io_in_async_context` | C |
| C has no string objects, lazy sequences, optional type, growable collection, or map in its standard library. | `bulk_collection_extension`, `eager_fallback_computation`, `immutable_string_concatenation_in_loop`, `intermediate_collection_materialization`, `known_size_preallocation`, `map_entry_opportunity`, `range_for_each_iteration` | C |
| C++ releases resources by RAII (`std::ifstream`, smart pointers) when the owner goes out of scope. | `resource_not_closed` | C++ |
| Compiled languages with no runtime evaluation of source text, and no standard deserializer that constructs arbitrary types. | `eval_in_production`, `unsafe_deserialization` | C, C++ |
| Copying a collection only to measure it has no idiom in the language. | `invariant_collection_copy_in_loop` | C#, Go, Java, JavaScript, TypeScript |
| ES imports cannot be function-local; a function-local `require()` or `import()` is CommonJS or ESM lazy loading, the exception the rule allows. | `function_local_import` | JavaScript, TypeScript |
| Effective Go asks for short local names (`r`, `w`, `buf`), so terse locals are the convention. | `identifier_tersity` | Go |
| Exceptions are the language's normal error channel, so a raise or throw is not an unhandled termination. | `unhandled_termination_in_production` | Java, JavaScript, Python, TypeScript |
| Exceptions are the language's normal error channel, so a throw is not an unhandled termination. | `unhandled_termination_in_production` | C#, Kotlin |
| Fallbacks are already lazy (`??`, `||`, `or`, conditional expressions), Go has no optional type, and Python's default-taking APIs have no lazy form. | `eager_fallback_computation` | Go, JavaScript, Python, TypeScript |
| Go drops a slice's front by reslicing (`xs = xs[1:]`), which copies nothing. | `quadratic_front_removal` | Go |
| Go has no lazy iterator chain whose materialization could be skipped. | `intermediate_collection_materialization` | Go |
| Go's `append` grows a slice in amortized constant time; Go has no rebuild-by-concatenation idiom for slices. | `loop_carried_copy_accumulation` | Go |
| Goroutines are not async functions: the runtime parks a goroutine blocked on I/O. | `blocking_io_in_async_context` | Go |
| Idiomatic Rust: a function-scoped `use` limits an import to where it is needed, and `&mut` declares a collection mutation in the signature. | `collection_parameter_mutation`, `function_local_import` | Rust |
| Importing one name twice is a Rust compile error (E0252); separate `use` items of one module are rustfmt's default granularity. | `duplicate_import` | Rust |
| Kotlin has no debugger statement or debug-trap call in its standard library. | `debugger_in_production` | Kotlin |
| Rust releases resources by RAII when the owner goes out of scope. | `resource_not_closed` | Rust |
| Standard and common deserializers (serde, `encoding/*`) construct plain data and run no code. | `unsafe_deserialization` | Go, Rust |
| The Go compiler rejects duplicate imports. | `duplicate_import` | Go |
| The XML parsers in use (`encoding/xml`, quick-xml, roxmltree, xml-rs) resolve no external entities. | `xml_xxe_vulnerable` | Go, Rust |
| The language has no `await`; goroutines and futures are composed explicitly. | `no_await_in_loop` | Go, Java |
| The language has no `catch`: errors are values. | `empty_catch_block` | Go, Rust |
| The language has no assertion that release builds strip: Rust's `assert!` and Node's `assert` always run, and Go has no assert. | `assert_in_production` | Go, JavaScript, Rust, TypeScript |
| The language has no capacity reservation for its growable sequence (`new Array(n)` makes a holey array). | `known_size_preallocation` | JavaScript, Python, TypeScript |
| The language has no debugger statement or debug-leftover macro. | `debugger_in_production` | Go, Java |
| The language has no function-local imports. | `function_local_import` | Go, Java |
| The language has no range object consumed by a `forEach` callback; a counting loop is already the idiom. | `range_for_each_iteration` | Go, JavaScript, Python, TypeScript |
| The language has no runtime evaluation of source text. | `eval_in_production` | Go, Rust |
| The language's map has no entry or compute-if-absent API. | `map_entry_opportunity` | Go, JavaScript, TypeScript |
| The language's naming convention has no package-prefix stutter rule (Rust RFC 430 and Effective Go do). | `identifier_stutter` | C, C++, C#, Java, JavaScript, Kotlin, Python, TypeScript |
| The signature declares every write to a caller's value (`&mut T`, `*T`), so an output argument cannot surprise the reader. | `out_parameter` | Go, Rust |
| The standard engines (Rust's `regex`, Go's `regexp`) are linear-time automata that never backtrack. | `catastrophic_backtracking_regex` | Go, Rust |

## Deferred Source Capabilities

Each unsupported catalog cell carries one of these executable limitation reasons.

| Missing capability | Pairs | Detector families | Languages |
|---|---:|---:|---|
| A C# file's top-level unit is its type; methods belong to classes that `class_structure_metrics` judges, and file-scoped namespaces need type-member ownership before module size means anything. | 1 | 1 | C# |
| An `assert` compiles out under `NDEBUG`, but whether a build defines it lives in the build system; the rule needs the build configuration to tell a stripped check from a live one. | 2 | 1 | C, C++ |
| Below the precision bar on our own Rust (0 of 2): parsers receive string kinds from external APIs (tree-sitter node kinds), which an enum cannot replace. | 1 | 1 | Rust |
| Below the precision bar on the corpus probe: writes to a parameter object's fields were designed output buffers and builders (okio segments, Newtonsoft readers), and C# `out`/`ref` parameters declare the write in the signature. | 2 | 1 | C#, Kotlin |
| Below the precision bar on the corpus sample (1 of 16): one-line predicates and callbacks use conventional short names (`fn`, `xs`, `m`), and Rust inline tests are not excluded from the lexical spans. | 4 | 1 | Java, JavaScript, Rust, TypeScript |
| Below the precision bar on the corpus sample (15 of 24): primitive specializations (int/long, float/double), mirror-image algorithms, and deliberately monomorphic copies repeat by necessity. | 1 | 1 | Java |
| Below the precision bar on the corpus sample (2 of 10): trait implementations, builder setters, and API facades (`get`/`post`) share callees by design. | 2 | 1 | Java, Rust |
| Below the precision bar on the corpus sample (Kotlin 0 of 12, C# 5 of 12; C and C++ probed alike): literals are mostly byte layouts, encoding and hash specification constants (UTF-8, Base64, SHA, IEEE-754), and value-keyed caches, which naming would not clarify. | 4 | 1 | C, C++, C#, Kotlin |
| Below the precision bar on the corpus sample (Kotlin 6 of 12, C# 7 of 12, C++ 2 of 9), for the reasons that deferred Java: per-type overloads and primitive specializations (`Varint32`/`Varint64`, `DateTime`/`DateTimeOffset`), mirror pairs (`Next`/`Prev`), and DSL families (`single`/`factory`/`scoped`) repeat by design. | 3 | 1 | C++, C#, Kotlin |
| C and C++ have no single naming convention (K&R snake_case, Google C++ PascalCase types, Win32 Hungarian); the rule needs a per-project convention to compare against. | 2 | 1 | C, C++ |
| C and C++ have no standard manifest; vcpkg and Conan manifests are not parsed yet. | 2 | 1 | C, C++ |
| C has no classes; a struct-plus-functions module would need ownership by naming convention or first parameter, which the structure facts do not model. | 1 | 1 | C |
| C has no collection library to identify: arrays and hand-rolled lists need cardinality and call-cost facts the frontend does not derive. | 9 | 9 | C |
| C strings are `char *` buffers whose comparison is a `strcmp` call, not an equality the structure facts record; C++ adds `std::string` overloads. The rule needs string-comparison facts for these grammars. | 2 | 1 | C, C++ |
| C# `using` directives name namespaces, not files: layering and coupling need a namespace-to-file index and project (`.csproj`) boundaries. | 2 | 2 | C# |
| C++20 coroutines (`co_await`) appear in none of the corpus repositories, so the cell has no capability evidence or precision sample. | 1 | 1 | C++ |
| Calls to `abort` and `std::terminate` guard broken invariants in libraries much as Go's `panic` does; the Go cell was reviewed and rejected for the same convention. | 2 | 1 | C, C++ |
| Collections are arrays and pointers (or STL containers passed by reference): a mutation through a non-`const` pointer or reference is declared in the signature, and the rule needs `const`-qualification facts to tell a surprising write from a designed output buffer. | 2 | 1 | C, C++ |
| Go mutates slices and maps through indexing and `append`, not method calls; the rule needs index-write facts for slice and map parameters. | 1 | 1 | Go |
| Go structure facts record no string literals and model `switch` cases as plain branches, so a sentinel comparison has no evidence to read. | 1 | 1 | Go |
| Gradle build scripts and MSBuild project files are not parsed yet. | 2 | 1 | C#, Kotlin |
| History-dependent: the depth-1 ecosystem corpus has no commit history, so the cell has no precision sample yet. | 8 | 1 | C, C++, C#, Go, Java, JavaScript, Kotlin, TypeScript |
| Kotlin suspends implicitly: a suspend call in a loop has no `await` marker, so the rule needs suspend-function resolution to find sequential awaits. | 1 | 1 | Kotlin |
| Kotlin's common clients (OkHttp, Ktor engines) and .NET's HttpClient bound requests by default; the unbounded forms (Ktor engines without HttpTimeout, a `Timeout` of `InfiniteTimeSpan`) are not in the client API table yet. | 2 | 1 | C#, Kotlin |
| Lexical occurrences include type and enum-variant names, which carry domain vocabulary (a `match` over long variant names reads as verbose); the rule needs binding-level identifiers before it extends. | 5 | 1 | Go, Java, JavaScript, Rust, TypeScript |
| Needs Kotlin collection and cardinality facts: stdlib extension functions (`sorted`, `contains`, `first`) resolve by receiver type, and `Sequence` chains are lazy, so the Java tables do not transfer. | 9 | 9 | Kotlin |
| Needs Kotlin collection facts: `toList()` and `toMutableList()` copies resolve by receiver type. | 1 | 1 | Kotlin |
| Needs STL identity (`std::sort`, `std::find`, container types) resolved through templates, overloads, and `using` declarations. | 9 | 9 | C++ |
| Needs a per-language escape-hatch table (Kotlin `!!` and `as`, C# `dynamic` and `unsafe`, C `void *` casts, C++ `reinterpret_cast`). | 4 | 1 | C, C++, C#, Kotlin |
| Needs a per-language escape-hatch table (`unsafe`/`transmute`, untyped JavaScript, `interface{}`, raw casts and `@SuppressWarnings`). | 4 | 1 | Go, Java, JavaScript, Rust |
| Needs async-context resolution (Kotlin `suspend` and dispatchers, C++ coroutines) plus blocking-I/O API identity. | 2 | 1 | C++, Kotlin |
| Needs cross-file reference resolution and the language's export model (`__all__`, ES exports, Go capitalization, Java and Kotlin visibility, C# assemblies, C/C++ linkage and headers); only Rust has it. | 18 | 2 | C, C++, C#, Go, Java, JavaScript, Kotlin, Python, TypeScript |
| Needs per-language loop and collection facts (C index loops over arrays, C++ range-for over STL containers). | 4 | 2 | C, C++ |
| Needs receiver-type facts for C++'s string and sequence APIs (`std::string` and ranges). | 3 | 3 | C++ |
| Needs receiver-type facts for the language's optional, sequence, and map APIs (`getOrDefault`, `ArrayList` capacity, `addAll`, `getOrPut`; `reserve`, `insert`, `try_emplace`). | 8 | 4 | C++, Kotlin |
| Needs receiver-type, alias, and evaluation-order facts for the language's string, regex, and collection APIs. | 15 | 5 | C, C++, Kotlin |
| Needs the language's effect, lifecycle, and topology classifiers (I/O, network, and database APIs per ecosystem; module and package graphs from Gradle or CMake). | 24 | 8 | C, C++, Kotlin |
| Needs the receiver-method call shape: shared callees here are mostly methods on different receivers. | 3 | 1 | Go, JavaScript, TypeScript |
| Needs type-member ownership: methods attached to a type through `impl` blocks or receivers. | 2 | 1 | Go, Rust |
| No corpus evidence: the probe over the Kotlin and C# repositories produced no finding, so the cell has no precision sample. | 2 | 1 | C#, Kotlin |
| No grammar-backed capability and adversarial negative boundary has been qualified for this detector-language pair. | 184 | 62 | Julia, Ruby, Shell |
| No qualified language-specific API identity and dataflow boundary exists for this security capability. | 18 | 6 | Julia, Ruby, Shell |
| Qualified on Python only; the Tier-1 extensions were reviewed and deferred (type names in lexical spans, conventional short names, shared receiver callees), and the Tier-2 grammars share those blockers. | 16 | 4 | C, C++, C#, Kotlin |
| Reviewed and rejected: every corpus `panic` was an API-contract guard, the convention that demoted the Rust cell to the full profile. | 1 | 1 | Go |
| Reviewed and rejected: writing a parameter object's fields is how these languages fill, normalize, and configure objects; the corpus probe (347 findings) was dominated by such designed writes. | 3 | 1 | JavaScript, Python, TypeScript |
| SQL text is assembled into buffers (`snprintf`, `strcat`, `std::string` appends) before `sqlite3_exec` or `PQexec`; the rule needs buffer dataflow to connect the build to the sink. | 2 | 1 | C, C++ |
| The Tier-2 discards (Kotlin `runCatching` results, C# unobserved `Task`s, C and C++ `warn_unused_result` returns) are not modelled yet. | 4 | 1 | C, C++, C#, Kotlin |
| The backtracking engines (Kotlin `Regex`, C++ `std::regex`, POSIX `regcomp`) take patterns through APIs the regex literal extraction does not model yet. | 3 | 1 | C, C++, Kotlin |
| The front removals (Kotlin `removeAt(0)`, C++ `erase(begin())`) and their array-backed declarations are not modelled yet. | 3 | 1 | C, C++, Kotlin |
| The frontend lacks a qualified collection, cardinality, mutation, or call-cost proof for this algorithmic capability. | 33 | 11 | Julia, Ruby, Shell |
| The frontend lacks a qualified effect, lifecycle, or resolved-topology classifier for this systems capability. | 27 | 9 | Julia, Ruby, Shell |
| The frontend lacks a qualified receiver type, alias, evaluation-order, or failure-behavior proof for this optimization. | 36 | 12 | Julia, Ruby, Shell |
| The test-scope facts model the Tier-1 frameworks; the Tier-2 forms (Kotlin JUnit and MockK, C# xUnit, NUnit, and MSTest, GoogleTest and Catch2 macros) are not modelled yet. | 8 | 2 | C, C++, C#, Kotlin |
| Untyped parameters: a collection cannot be told from an output sink or builder (a `StringBuilder`-like accumulator), which dominated the typed-language probe. | 1 | 1 | JavaScript |
| libcurl's default is no timeout (`CURLOPT_TIMEOUT` 0), but the client API table does not model curl handles and their option calls yet. | 2 | 1 | C, C++ |
## Role Contracts

`emission_roles` authorize finding locations. `context_roles` authorize read-only
repository observation, such as resolving a production symbol referenced by a test.
Both are emitted by the executable catalog and covered by registry tests.

# Detector Promotion Disposition

Updated: 2026-10-18

This is the maintained result of the v0.8.9 profile audit, with the v1.0
precision gate below. It covers all 56
detectors that were full-only during evaluation: four promoted to gating and 52
retained full-only. None were retired, and none remain undecided. On
2026-10-18 the operator promoted seven more that meet the precision bar (see
[Promoted To Gating](#promoted-to-gating)).

Promotion requires deterministic semantic evidence, executable positive and
adversarial-negative cases for every claimed language, integration coverage,
reviewed repository findings, no competing owner, and exact live donor parity
where a mapped donor exists. A baseline may record accepted repository debt; it
cannot justify a detector false-positive mechanism.

## Precision Gating

Since v1.0 Task 8, a gating detector must also meet the measured precision
bar on the ecosystem corpus ([`detector-precision.md`](./detector-precision.md)):
the 95% lower confidence bound of precision ≥ 85% over final or corroborated
labels plus audited oracle-confirmed findings, holding even were every
unverified pre-label a false positive. A detector is below the bar when its
best-case precision is under 90%.

- **Enforced.** `tooling/detector_gate_lint.py` (CI) reads the committed
  `eval/ecosystem/precision.json` and fails when a gating detector is
  `below_bar` overall or in any language, or holds a live false positive on
  an insufficient sample. The fix is to scope the failing
  languages to `full` (`DetectorSpec::full_profile_in`) and keep the
  detector gating elsewhere; the overall standing is then re-judged over the
  languages it still gates in. A detector is demoted whole only when that
  standing still fails.
- **Scoped languages.** A gating scan drops the detector's findings in a
  scoped language, and the report's language claim says "Full profile only";
  `--profile full` still reports them.
- **Unverified.** The detector keeps its current profile and is flagged
  below; verifying its pending labels decides it.
- **Insufficient sample.** When every corpus finding is labelled and there
  are still too few to decide the bound (rare constructs such as `eval`,
  debugger traps, or broken anchors), a gating detector stays gating only
  with no live false positive in the languages it gates in, plus its
  positive and adversarial negative capability cases (operator decision,
  2026-10-18). The lint enforces this; the fix is to remove the false
  positive's mechanism, scope the language to `full`, or demote the
  detector.
- **Promotion.** Meeting the bar is necessary, not sufficient: a `full`
  detector that meets it still needs the qualification evidence above and a
  risk class that no longer applies. The operator accepted the R4 risk for
  the seven calibrated-metric detectors that meet the bar (2026-10-18): their
  thresholds gate production code, and this repository's own findings were
  refactored or recorded as reviewed debt before they gated.
- **Refresh.** After labels change, regenerate the snapshot with
  `labels.py precision --scan <scan> --json eval/ecosystem/precision.json`.

### Demoted

| Detector | Standing | Reason |
|---|---|---|
| `unhandled_termination_in_production` | below_bar (0/12) | Idiomatic Rust panics on violated contracts and impossible states; no recoverable-error panic in the sample |
| `magic_literal_cluster` | below_bar (57/21) | Scoped to `full` in Java, Python, and TypeScript, then below the bar overall once the strict recount resolved its unverified labels; demoted everywhere (operator decision, 2026-10-18) |
| `console_output_in_production` | below_bar (43/21) | Residual docs-site and root build scripts (`www/`, `mm.mjs`) print by design; the Go form added in Task 10 is unaffected but the detector gates as one |
| `insecure_cryptography` | insufficient_sample (0/3) | SHA-1 where collision resistance is not relied on (HMAC signing, a digest-auth nonce); no true positive on the corpus (operator rule, 2026-10-18) |
| `sql_injection` | insufficient_sample (0/2) | Dapper.Rainbow builds table names from type metadata; no true positive on the corpus |
| `nested_quadratic_iteration` | insufficient_sample (0/2) | Inner scans bounded by their segment, pairwise checks over small lists; no true positive |
| `sibling_call_redundancy` | insufficient_sample (3/3) | Shared helper callees are composition, not redundant work |
| `resource_not_closed` | insufficient_sample (2/1) | fmt's release script under `support/`, a path no role rule reads as tooling |
| `identifier_tersity` | insufficient_sample (1/2) | Domain names (`r1, g1, b1`) and vendored docopt |

### Scoped to full in some languages

| Detector | Full-profile languages | Reason |
|---|---|---|
| `exit_in_production` | JavaScript, Python, TypeScript | Every JavaScript and TypeScript false positive (0/2 each) is a CLI or build entry point's exit; Python's two are vendored docopt exiting after `--help` (2026-10-18); Go holds none |
| `large_file` | C | Below the bar there (26/3): every false positive is vendored third-party code (zlib `contrib/minizip`, jq's David M. Gay dtoa) that path rules cannot tell from the project's own; the other languages still gate |
| `class_structure_metrics` | Python | Below the bar there when promoted; Java meets it and the other languages gate |
| `cognitive_complexity` | C, C++ | Below the bar there when promoted (macro-heavy and vendored code); the other languages gate |
| `npath_complexity` | C, C++ | Below the bar there when promoted, like `cognitive_complexity`; the other languages gate |
| `duplicate_import` | TypeScript | Its one live false positive is zod's module-resolution test package; C, JavaScript, and Python hold none (2026-10-18) |
| `unused_assignment` | C++ | All three false positives are yaml-cpp `util/` and `contrib/` code; the other languages hold none |
| `stringly_typed_parameter` | Python, TypeScript | `mode` mirroring `open()` and a media-type value; JavaScript holds none |

### Gating detectors with a measured standing

Gating detectors absent here produced no labelled corpus finding. TP/FP
counts only final and corroborated labels on scans `t22-a` and `a1-b` (application slice), in the
languages the detector gates in. An `insufficient_sample` detector gates
only with no false positive here.

| Detector | Standing | TP/FP | Accepted | Languages meeting the bar | Full-profile languages |
|---|---|---:|---:|---|---|
| `architectural_layering` | insufficient_sample | 6/0 | 0 | — | — |
| `assert_in_production` | meets_bar | 35/1 | 0 | python | — |
| `bind_all_interfaces` | insufficient_sample | 0/0 | 0 | — | — |
| `class_structure_metrics` | meets_bar | 107/2 | 0 | java | python |
| `cognitive_complexity` | meets_bar | 217/1 | 512 | csharp, java, javascript, python, rust, typescript | c, cpp |
| `debugger_in_production` | insufficient_sample | 0/0 | 0 | — | — |
| `duplicate_import` | insufficient_sample | 13/0 | 0 | — | typescript |
| `empty_catch_block` | insufficient_sample | 18/0 | 0 | — | — |
| `eval_in_production` | insufficient_sample | 5/0 | 0 | — | — |
| `excessive_nesting_depth` | meets_bar | 41/1 | 0 | — | — |
| `exit_in_production` | insufficient_sample | 3/0 | 0 | — | javascript, python, typescript |
| `function_complexity` | meets_bar | 183/2 | 654 | c, cpp, csharp, java, javascript, kotlin, python, rust, typescript | — |
| `function_local_import` | meets_bar | 30/0 | 38 | python | — |
| `go_errcheck` | meets_bar | 27/0 | 0 | go | — |
| `go_log_fatal` | insufficient_sample | 0/0 | 0 | — | — |
| `halstead_information_density` | meets_bar | 138/3 | 521 | c, cpp, csharp, java, javascript, kotlin, python, typescript | — |
| `hardcoded_credential` | insufficient_sample | 0/0 | 0 | — | — |
| `hot_path_linear_membership` | insufficient_sample | 1/0 | 0 | — | — |
| `kt_non_null_assertion` | meets_bar | 30/0 | 0 | kotlin | — |
| `large_file` | meets_bar | 123/4 | 0 | java | c |
| `loop_invariant_regex_compilation` | insufficient_sample | 1/0 | 0 | — | — |
| `naming_convention` | meets_bar | 46/1 | 720 | java | — |
| `npath_complexity` | meets_bar | 181/0 | 45 | java, python, typescript | c, cpp |
| `out_parameter` | meets_bar | 81/1 | 0 | java | — |
| `package_coupling_metrics` | insufficient_sample | 2/0 | 0 | — | — |
| `repeated_collection_traversal` | insufficient_sample | 1/0 | 0 | — | — |
| `repeated_format_allocation` | insufficient_sample | 0/0 | 0 | — | — |
| `section_divider_comment` | insufficient_sample | 4/0 | 0 | — | — |
| `shell_execution` | insufficient_sample | 0/0 | 0 | — | — |
| `source_duplicate_detection` | meets_bar | 169/9 | 36 | c, go, typescript | — |
| `stale_link_reference` | insufficient_sample | 6/0 | 0 | — | — |
| `stale_section_anchor` | insufficient_sample | 14/0 | 0 | — | — |
| `stringly_typed_parameter` | insufficient_sample | 1/0 | 0 | — | python, typescript |
| `todo_unimplemented_in_production` | meets_bar | 143/2 | 0 | java, kotlin, rust | — |
| `too_many_parameters` | meets_bar | 89/0 | 188 | csharp, java, kotlin, python | — |
| `too_many_returns` | meets_bar | 128/0 | 399 | c, java, javascript, kotlin, python, typescript | — |
| `too_many_statements` | meets_bar | 97/2 | 0 | java | — |
| `unreachable_code` | insufficient_sample | 3/0 | 0 | — | — |
| `unsafe_deserialization` | insufficient_sample | 4/0 | 0 | — | — |
| `unused_assignment` | insufficient_sample | 4/0 | 0 | — | cpp |

## Risk Classes

| Code | Reason a correct finding remains review-only |
|---|---|
| R1 | Replacement correctness, profitability, materiality, ordering, or allocation impact still depends on local code contracts. |
| R2 | The finding is an intent-sensitive maintainability smell rather than a deterministic defect. |
| R3 | Framework, reflection, ownership, runtime, configuration, or external-entry boundaries remain unresolved. |
| R4 | The owner applies a calibrated metric threshold whose exceedance is advisory. |
| R5 | Ordering, I/O, lifecycle, concurrency, or operational intent requires system-owner review. |

## Promoted To Gating

| Detector | Qualification |
|---|---|
| `duplicate_import` | Semantic canonical-import ownership, adversarial negatives, exact ESLint parity, and two reviewed real split-import findings. |
| `out_parameter` | Resolved Java writes through a parameter's field or element, rebinding/owner-type/State-handler negatives. |
| `excessive_nesting_depth` | Normalized control depth, compensated `else if`, exact Pylint parity, and reviewed public-donor/self findings. |
| `unreachable_code` | Ten grammar-backed transfer boundaries, malformed/hoisting/comment negatives, and exact Detekt plus Rust compiler/Clippy parity. |
| `cognitive_complexity` | Precision gate (2026-10-18, operator): 266/12 on `t16-v` with a clean oracle agreement audit; production files only; C/C++ stay `full`. |
| `npath_complexity` | Precision gate (2026-10-18, operator): 227/9; production files only; C/C++ stay `full`. |
| `halstead_information_density` | Precision gate (2026-10-18, operator): 138/4; production files only. |
| `class_structure_metrics` | Precision gate (2026-10-18, operator): 133/5; production files only; Python stays `full`. |
| `source_duplicate_detection` | Precision gate (2026-10-18, operator): 169/9 after the extraction-worthiness conventions. |
| `too_many_returns` | Precision gate (2026-10-18, operator): 128/0; production files only. |
| `too_many_statements` | Precision gate (2026-10-18, operator): 97/2; production files only. |

## Retained Full-Only

| Detector | Risk | Disposition reason |
|---|---|---|
| `full_sort_for_top_k` | R1 | `k` materiality and ordering contract remain repository-specific. |
| `loop_carried_copy_accumulation` | R1 | In-place replacement requires ownership and allocation-impact review. |
| `invariant_collection_copy_in_loop` | R1 | Removing a clone still depends on ownership and future-use intent. |
| `repeated_recursive_subproblem` | R1 | Memoization changes space, cache-key, and failure behavior. |
| `nested_cross_collection_traversal` | R1 | Index or sort/merge replacement depends on pair semantics and ordering. |
| `manual_collection_copy_loop` | R1 | Bulk replacement must preserve overlap and element semantics. |
| `eager_fallback_computation` | R1 | Lazy replacement can change evaluation and failure timing. |
| `intermediate_collection_materialization` | R1 | Laziness can change side effects, ownership, and lifetime. |
| `range_for_each_iteration` | R1 | Loop replacement value depends on local style and allocation behavior. |
| `loop_invariant_computation` | R1 | Hoisting must preserve failure and evaluation timing. |
| `known_size_preallocation` | R1 | Capacity benefit depends on cardinality and allocation policy. |
| `bulk_collection_extension` | R1 | Bulk operations require identity-copy and ownership proof. |
| `map_entry_opportunity` | R1 | Entry rewrites must preserve lookup, insertion, and borrow semantics. |
| `csharp_linq_enumeration` | R1 | Enumeration cost depends on provider and deferred execution. |
| `java_inefficient_string_buffer` | R1 | Buffer replacement depends on synchronization and escape behavior. |
| `identifier_stutter` | R2 | Repetition can be intentional domain naming. |
| `try_except_pass` | R2 | Silent exception handling can be an explicit boundary policy. |
| `empty_conditional_branch` | R2 | Empty branches can document intentional no-op behavior. |
| `constant_condition` | R2 | Literal predicates can be deliberate feature or sentinel controls. |
| `ruby_double_bang` | R2 | Boolean coercion is often intentional style. |
| `kotlin_empty_when` | R2 | An empty `when` can be deliberate scaffolding or exhaustiveness documentation. |
| `go_repeated_string_literal` | R2 | Repetition does not always justify a shared constant. |
| `kt_require_check_misuse` | R2 | API choice depends on caller-facing contract intent. |
| `collection_parameter_mutation` | R3 | Caller ownership and mutation contract are not locally complete. |
| `orphan_symbol` | R3 | Dynamic and external references can remain unresolved. |
| `world_writable_file` | R3 | Permission intent and deployment boundary require review. |
| `hardcoded_tmp_directory` | R3 | Platform and sandbox policy determine whether the path is unsafe. |
| `flask_debug_true` | R3 | Deployment configuration and environment substitution remain external. |
| `random_for_security` | R3 | Security sensitivity is inferred from local API context. |
| `request_no_cert_verify` | R3 | Test and controlled-network policy can intentionally disable verification. |
| `xml_xxe_vulnerable` | R3 | Parser defaults and input trust boundaries vary by deployment. |
| `ssl_with_bad_defaults` | R3 | Compatibility and endpoint policy remain external constraints. |
| `ssh_no_host_key_verification` | R3 | Host-key policy depends on deployment trust configuration. |
| `cpp_raw_ptr_ownership` | R3 | Ownership may be governed by an external API or custom allocator. |
| `require_atomic_updates` | R3 | Concurrency, alias, and scheduling boundaries remain incomplete. |
| `dead_code` | R3 | Reflection, generated registration, and external entry points can be unresolved. |
| `unsafe_type_cast` | R3 | Runtime type guarantees may live outside the analyzed function. |
| `god_module` | R4 | Top-level definition count alone does not prove poor cohesion. |
| `too_many_locals` | R4 | Local-count budgets depend on function shape and domain clarity. |
| `escape_hatch_type_density` | R4 | Type-escape density can be legitimate at interoperability boundaries. |
| `n_plus_one_query_in_loop` | R5 | Query batching depends on consistency, latency, and repository semantics. |
| `blocking_io_in_async_context` | R5 | Executor and scheduling policy determine operational impact. |
| `per_item_external_io` | R5 | Batching can change ordering, retries, and rate-limit behavior. |
| `repeated_expensive_initialization` | R5 | Lifecycle and isolation requirements may require repetition. |
| `request_path_repository_scan` | R5 | Request-path cost depends on caching and freshness policy. |
| `no_await_in_loop` | R5 | Sequential awaiting can be required for ordering or backpressure. |

## Qualification Evidence

- The serial live gate passes all 22 parity/status groups across all 12 mapped
  donors. The four promoted owners retain exact donor agreement.
- The 169-file self scan reports 174 candidates and no finding in a
  v0.8.9-created implementation owner; its one promoted nesting finding is
  legitimate maintained debt. Eighteen pre-existing dispatcher-table findings
  remain outside the new Layer 3 routing functions.
- A 156-file locked public donor scan produced 14 promoted nesting findings;
  every reported AST/parser traversal was source-reviewed and legitimate, and
  the four new Layer 3 owners emitted none.
- A 2,421-file untracked repository scan produced two legitimate split-import
  findings and one legitimate growing-array copy finding across the changed
  owners. No changed-owner false-positive mechanism was found.

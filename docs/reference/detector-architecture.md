# Detector Architecture

This is the canonical detector ownership and file-structure contract.

## Catalog

`crates/auditor-detectors/src/registry.rs` owns the static
`DETECTOR_CATALOG`. Its adjacent `registry_owner.rs` and
`registry_language_support.rs` submodules own the exhaustive owner enum and
explicit support-policy tables; `registry_cell_dispositions.rs` holds the
reviewed reason for each Tier-1 cell a policy leaves unsupported (not
applicable, or the specific fact it lacks), and a registry test rejects a
stale one. `registry_types.rs` owns role applicability and
the typed spec shape. Each `DetectorSpec` contains:

- stable detector ID and category
- default severity, minimum profile, maturity, and layer
- one unique `DetectorOwner` enum value
- explicit finding-emission and read-only context role sets
- one explicit language-support policy, expanded into complete pair metadata

The default catalog contains 114 detectors: 52 gating and 62 full-only. The
`graph-analysis` feature adds seven gating detectors. The six graph-dependent
owners share `repository_graph.rs`, while retaining independent catalog and
capability contracts.

There is no second metadata list. `detector_registry(profile)` projects public
definitions from active specs, runtime scheduling iterates those same specs,
and `auditor catalog` serializes the projection for docs and automation.
`language_gate.rs` applies that policy to emitted finding locations, so an
unsupported or non-applicable file cannot leak a raw lexical match.

## Owner Shape

Every detector follows this structure:

1. One catalog spec names one unique owner.
2. `dispatcher.rs::run_detector_owner` maps that owner through one bounded
   concern dispatcher to one detector-named entry point.
3. The entry point may use shared traversal helpers, but its detector-specific
   constructor emits exactly one ID and the catalog category.
4. Runtime validation rejects an undeclared ID, wrong owner ID, or category
   mismatch.
5. Tests provide one positive and one semantic negative capability case for
   that ID.
6. The language contract executes claimed arms and silence boundaries for all
   catalog cells.

Concern modules are allowed, but mixed detector bags are not. A shared helper
may walk files, lines, functions, or AST nodes only when the selected
detector-specific callback owns construction. Target enums that route several
literal detector IDs through one constructor are forbidden.

Representative module boundaries:

- `dispatcher.rs`: exhaustive runtime owner routing split into bounded concern
  functions
- `language_gate.rs`: catalog-backed emission enforcement by evidence location
- `ast_scan.rs`: canonical scan-owned AST visitor, node helpers, and Rust test
  ancestry
- `correctness.rs`: semantic assertion, TODO, and eval owners
- `control_flow.rs`: separate unreachable-code and constant-condition
  constructors over the shared visitor
- `dead_code_semantic.rs`: findings-free Rust definition/reference index;
  `dead_code.rs`: detector-owned confidence and finding construction
- `security_rules.rs`: independent security capability constructors over
  semantic Python calls/assignments plus bounded exact lexical owners
- `security_api_rules.rs`: the call-based security owners (Python callee
  text plus each language's API-identity site finder); `security_rules.rs`
  keeps the literal and lexical owners and the shared rule metadata
- `security_sql.rs`, `security_crypto.rs`, `security_process.rs`: semantic Go,
  Java, and Python owners with no detector-private parser or file-read fallback.
  `security_sql_names.rs` decides whether a query's runtime values only shape
  its structure; `security_sql_templates.rs` reads the format templates and
  concatenations a query is built with (both findings-free)
- `go_api_rules.rs`, `java_api_rules.rs`, `js_api_rules.rs`, `python_api_rules.rs`,
  `rust_api_rules.rs`:
  findings-free L1 site finders over resolved API identity; owners construct
  the findings. `statement_shapes.rs`: findings-free empty-handler,
  empty-branch, and await-in-loop sites read from the grammar
- `errorprone_language_rules.rs`: the Ruby, C++, Kotlin, and C# lexical
  contracts; `async_rules.rs`: the grammar-backed await-in-loop owner and the
  lexical atomic-update owner; `braced_statement_shapes.rs`: Go, Java, and
  JS/TS statement shapes (public-argument asserts, assignment conditions,
  empty catches and branches); `security_literal_rules.rs`: credential and
  temp-directory literals in every Tier-1 language
- `java_api_identity.rs`, `java_syntax.rs`: Java import bindings, and the
  declaration and static type of a Java expression, shared by the L1 tables
  and the structure index; `java_async.rs`: Java callbacks on shared async
  executors (default-pool `CompletableFuture`, unscheduled Reactor/RxJava)
- `js_api_identity.rs`: JavaScript/TypeScript module bindings; `python_api_identity.rs`:
  Python import bindings and shadow-aware builtins; `rust_api_identity.rs`: Rust `use`
  trees
- `cognitive.rs`: one shared complexity fact index with one finding builder per
  metric
- `dead_stores.rs`: the ineffectual-assignment owner; `assignment_shapes.rs`:
  each language's plain store into one local and the enclosing function's
  local names
- `structure_operands.rs`: Go and Python subscripts and Python membership
  tests for the structure index; `optimization_fact_loop_forms.rs`: Python
  and Rust L5 loop forms
- `structure_index.rs`, `structure_build.rs`, and `structure_imports.rs`: one
  findings-free resolution-aware Layer 2 fact index
- `structural_metrics.rs`, `structural_types.rs`, and
  `structural_classes.rs`: detector-owned Layer 2 constructors
- `naming.rs`: naming finding construction; `naming_java.rs` and
  `naming_kotlin.rs`: findings-free language support; `naming_tests.rs`:
  production-path capability regressions over `StructureIndex`
- `duplication.rs` and `duplication_matcher.rs`: bounded normalized clone
  classes and exact occurrence spans
- `lexical.rs`, `structural.rs`, and `systems.rs`: shared
  traversal helpers with detector-specific entry points
- `algorithmic_facts.rs` and its support modules: findings-free Layer 3 effect,
  collection, recursive, copy/allocation, and feasible control-path facts;
  `algorithmic_cost.rs` and `algorithmic_cost_model.rs`: exhaustive cost
  catalog plus typed source-spanned sum/max/product expressions
- `systems_facts.rs`: findings-free Layer 4 index boundary and typed facts;
  `systems_effects.rs` and `systems_effect_classification.rs`: effect extraction,
  context, receiver/value lineage, and classification; `systems_topology.rs`:
  resolved package, inheritance, re-export, cycle, completeness, and provenance
  facts
- `systems.rs` and `layering.rs`: detector-owned Layer 4 constructors over the
  shared systems index
- `optimization_facts.rs`: findings-free Layer 5 index boundary;
  `optimization_fact_flow.rs`, `optimization_fact_operations.rs`,
  `optimization_fact_collections.rs`, and `optimization_fact_syntax.rs`:
  proof-oriented fact support; `optimization_fact_collectors.rs`: fact
  orchestration
- `source_cache.rs`: scan-owned source bytes, line offsets, and canonical span
  handles
- `auditor-ast/src/extract.rs`: function/comment extraction and structural
  fingerprints over the shared tree
- `auditor-ast/src/semantic.rs`, `semantic_controls.rs`, and
  `semantic_fields.rs`: lazy span-only semantic facts plus control and
  grammar-field normalization over the same source allocation
- `auditor-ast/src/dataflow.rs`, `dataflow_bindings.rs`,
  `dataflow_shapes.rs`, and `dataflow_values.rs`: findings-free def-use flow
  within one function (reaching bindings, value origins, value
  destinations), with each grammar's binding shapes (function names,
  parameter lists, loop variables, targets and values) in
  `dataflow_shapes.rs`; detector-side
  request sources and security sinks live in `dataflow_sources.rs`, and
  `bound_reads.rs` gives owners the reads of a bound handle and whether
  it is released or handed on
- `algorithmic.rs` and `optimization.rs`: one literal constructor call per
  Layer 3 or Layer 5 detector owner
- `repository_graph.rs`: typed external graph/provider contract and six
  graph/work-graph detector entry points
- `golang.rs`, `java.rs`, and `kotlin.rs`: language-specific owners

Tests and large stable token tables may use adjacent `_tests.rs` files or
private submodules. This is a size/ownership boundary, not a separate detector
shape: every detector still has one catalog spec, owner, entry point,
constructor ID, and capability case.

## Dual-Path Detectors

A detector with AST and token implementations still has one owner. The owner
uses AST analysis when the method permits it and the language has a semantic
arm. Parse failure fails closed for semantic-only claims. Only findings from a
cataloged `semantic`, `exact_lexical`, or `partial` arm survive the runtime
language gate.

The required routing invariant is:

~~~rust
if method != AnalysisMethod::Token {
    if let Some(ast_language) = map_language_to_ast(&file.language) {
        // Run this detector's AST implementation.
        // Continue after a successful AST analysis for this file.
    }
}

// Run this same detector's token fallback.
~~~

Forbidden forms:

- `method != AnalysisMethod::Ast` as the token gate
- special treatment of `AnalysisMethod::Auto` that overlaps both paths
- construction of the same ID in separate files
- an aggregate AST pass that constructs several detector IDs

Current dual-path owners include `assert_in_production`, `sql_injection`,
`insecure_cryptography`, and the four
complexity metrics. Their shared parsing helpers do not own multiple IDs.

## Profiles

The catalog minimum profile is authoritative:

- a gating scan executes only gating specs
- a full scan executes gating and full-only specs
- an inactive owner cannot contribute findings because it is never scheduled
- emitted IDs are checked against the active spec before entering the report

The capability suite replays every full-only positive case under gating and
requires absence.

## Capability Tests

The adjacent `capability_cases_*` modules define atomic temporary-repository
cases by concern, including the dedicated performance case module.
`capability_tests.rs` requires exact set equality between those cases and
the active catalog, then checks:

- the selected ID is present for the positive source
- it is absent for a semantically clean negative source
- the finding has a recommendation and valid location
- full-only capabilities are absent from gating
- feature-only catalog additions are explicit and executable

These are owner capability tests, not expected-output snapshots. Broader unit
tests remain useful for algorithm boundaries and repository integration.

## Donor Parity

Local capability tests establish Engineering Auditor behavior. Donor parity is
a separate contract and must execute a donor on the same atomic case. See
`docs/reference/donor-parity-spec.md`.

A checked-in EA expected JSON file cannot establish donor agreement.

## Enforcement

Run:

~~~sh
python3 tooling/detector_gate_lint.py
python3 -m unittest tooling.lint.test_detector_arch
python3 tooling/lint/detector_arch.py
~~~

The linter fails on:

- duplicate catalog IDs or owners
- catalog owners absent from runtime scheduling
- missing, extra, or duplicate capability cases
- any function that constructs more than one detector ID
- forbidden public mixed-bag entry points
- unregistered IDs or category mismatches
- the same ID constructed in multiple files
- invalid AST/token gate forms

The second linter independently compares production `Finding::new`, literal
typed shared-constructor calls, and typed security-rule emission sites with
`registry.rs`; inline and adjacent test modules are excluded from that
production ownership check.

Runtime validation and executable tests remain authoritative if static parsing
cannot prove a dynamic property.

## Scan-Owned State

Detector helpers may retain repository facts only inside the active scan:

1. `FileCache` owns one shared contiguous source buffer and line-offset table
   per file; line-oriented detectors borrow views into those bytes.
2. `RepoAst` parses the same shared source allocation and owns the canonical
   function-span array beside the tree.
3. `ComplexityIndex` computes branch, cognitive, NPath, and Halstead facts once
   and also supplies branch totals to churn analysis.
4. `AlgorithmicIndex`, `OptimizationIndex`, `ComplexityIndex`, `SystemsIndex`,
   and `StructureIndex` are created lazily and dropped after their final
   registered owners. Clone token buffers are owner-local and never enter
   retained repository state.
5. Role applicability is implemented with borrowed slices over role-partitioned
   scan storage; the public file list retains discovery order.

Thread-local, process-global, or donor-owned repository caches are forbidden.
Sequential repository scans must produce the same report as isolated scans,
and tests lock source/span allocation sharing, line semantics, role-view
borrowing, public ordering, and A/B/A scan isolation.

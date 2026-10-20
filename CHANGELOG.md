# Changelog

## Unreleased

### Added

- `per_item_external_io` and `blocking_io_in_async_context` follow calls into
  repository functions: a call inherits the external-I/O and blocking-I/O
  effects of the function it invokes, through up to four calls. Findings
  through a helper name it in `via` evidence and have Medium confidence.
  Calls resolve only to certain targets (`call_graph.rs`). On the recall
  benchmark, helper and cross-file detection rises from 0 to 9 of 29 each;
  on the 46-repository corpus the two detectors gain 15 findings.
- `loop_artifact_rewrite` follows calls into repository functions too. A
  helper's artifact and written value bind to the call's arguments by
  position, so a loop that reads an artifact and passes it to a helper
  writing a fixed artifact is reported, as is a loop calling a helper that
  rewrites one artifact itself. Helper and cross-file recall rises to 14 of
  29 each. No corpus finding changes: the detector reports nothing there,
  directly or through calls.
- `repeated_sort_in_loop` and `n_plus_one_query_in_loop` follow calls into
  repository functions: a function that sorts a parameter, or issues a read
  query whose arguments name its parameters, passes the operation to its
  callers by parameter position, through up to four calls. Helper and
  cross-file recall rises to 24 of 29 each. Neither detector reports
  anything on the 46-repository corpus, directly or through calls. The call
  graph is now built once per scan and shared by the indexes that follow
  calls.
- `loop_invariant_regex_compilation` follows calls into repository
  functions: a loop calling a helper that compiles a fixed expression, with
  loop-invariant arguments, is reported at the call site. A helper's pattern
  must read only plain values (parameters, literals, names it never assigns;
  no nested call or member of `self`, `this`, or a parameter), and a helper
  decorated with `@functools.cache` or `@lru_cache` is excluded. Helper and
  cross-file recall rises to 29 of 29 each; the corpus is unchanged.
- `n_plus_one_query_in_loop` recognizes more stores and lookups. Reads of
  one record by key (`getObjectByUuid`, `findUserById`) count as queries, and
  receivers are proven by Java field types, try-with-resources locals, `var`
  locals bound to `new` of a store type, query-manager, persistence-manager
  and entity-manager types, and JavaScript bindings imported from
  `redbean-node` or `knex`. On the 46-repository corpus the detector reports
  its first 6 findings, all real per-item queries (dependency-track and
  uptime-kuma), one of them through a helper.
- A seeded recall benchmark (`tooling/recall/`, `eval/recall/`) for the
  algorithmic, systems, and optimization detectors: 116 cases in Rust,
  Python, TypeScript, Go, and Java, each putting a known problem directly in
  a loop, in a same-file helper, or in an imported helper, with a fixed
  negative. CI fails if a detected case is missed or a negative fires.

## 1.1.0 - First Public Release

The first release from the public repository. Scans are faster and use all
cores, release archives carry third-party license notices, churn follows the
first-parent history, and two detectors specific to one downstream project
are removed from the experimental `graph-analysis` feature. Stable surfaces
are unchanged.

### Fixed

- Recovery of C, C++, and C# files with syntax errors goes further. It
  keeps one configuration of the preprocessor conditionals (the first live
  branch of each `#if` group, without the directive lines), and in C and
  C++ it blanks a macro in a declaration header (`WINAPI`, `GTEST_API_`,
  zlib's `local`) when that lets the declaration parse, before
  quarantining what remains. Reparses during recovery are incremental. On
  the 46-repository corpus, 442 files are recovered and 148 are still
  skipped (from 361 and 234), adding 208 more findings; 27 console-output
  findings in zlib's `miniunz.c` are gone because its `main` is no longer
  quarantined, so the file reads as the program it is.
- A file with syntax errors is no longer dropped from structural analysis.
  Each top-level item or type member holding an error is quarantined:
  blanked with spaces, keeping every offset and line, and the source is
  parsed again until it parses cleanly, with every remaining function whole.
  Structural, algorithmic, systems, and optimization detectors analyze the
  recovered tree; complexity owners keep reading the tree as written. A file
  is skipped only when recovery would quarantine half of it. Report files
  carry an optional `syntax` (`recovered` or `unparsed`), and
  `analysis_coverage` adds optional `syntax_recovered_files` and
  `syntax_unparsed_files`. On the 46-repository corpus, 361 more files get
  structural analysis, adding 599 findings (two of them real N+1 queries in
  dependency-track); six clone findings change ID because a recovered file
  joins their clone class. 234 files are still skipped.
- TypeScript sources the TypeScript grammar rejects are re-parsed with the
  TSX grammar, so `.tsx` files with JSX get structural analysis instead of
  being skipped. On the 46-repository corpus, 247 of the 268 TypeScript
  files that failed to parse now parse.
- Java try-with-resources locals (`try (Store store = open())`) are local
  variables: `too_many_locals` counts them, and detectors that read a local's
  declared type see them. An unnamed resource (`var _ = ..`) declares nothing.
- `hardcoded_credential` no longer reports credential-named constants that
  name something about a credential: a target whose last word is a
  location, kind, owner, version, route, prefix or label
  (`SECRET_STORE_DIR = "secrets"`, `CREDENTIAL_KIND = "credential_material"`),
  a lookup key (`SECRET_BACKEND_CONFIG_KEY`, but not `SECRET_KEY`), or a
  literal equal to the target's name (`api_key: "api_key"`). Target words
  are split on case too, so the existing suffixes (`_env`, `_header`, ..)
  now match camelCase targets.
- `per_item_external_io` and `n_plus_one_query_in_loop` no longer report a
  loop that already walks groups of items: a binding whose last word is
  `batch`, `chunk`, `page`, `window`, `partition`, or `shard`, or an
  iterable that calls a batching function (`chunks`, `chunked`, `batched`,
  `partition`, `windows`, `paginate`, ..). This removes 5 findings from the
  46-repository corpus, all one request per batch or per page.
- `churn_complexity_hotspot` requires at least 50 lines of growth as well as
  the upper growth quartile. In a window where most candidates barely grew,
  the quartile cutoff fell to a few lines or zero, so files with +0 to +20
  lines ranked as hotspots. On this repository's own history the floor
  removes two such findings (+16 and +18 lines) and keeps the one at +63;
  on the private fork's history all nine hotspots (+59 to +188) remain.
- `hot_path_linear_membership` reads a Python set or dict display or
  comprehension by its outer braces, so
  `{(e["path"], e["kind"]) for e in rows.get("allow", [])}` is a set even
  though it contains `[]`.

### Performance

- Complexity metrics look up each function's syntax node in a per-file index
  built in one tree walk, instead of re-walking the file for every function.
  Findings are unchanged; a commons-lang scan drops from 40 s to 13 s and a
  ripgrep scan from 8.6 s to 2.7 s.
- Source files are parsed, and the structure and complexity indexes are
  built, on all available cores, with results merged in file order so
  reports stay byte-identical. Gating scans of commons-lang and eslint drop
  to about 8 s.

### Changed

- Churn follows `HEAD`'s first-parent line and counts a merge once, as its
  change against the first parent, so commits of a merged branch or a linked
  unrelated history are not counted again.
- `tooling/r3_a2_hot_path_measure.py` is now `tooling/scoped_scan_latency.py`,
  and docs and code comments no longer point into the implementation plans,
  which the published repository excludes.

### Removed

- `historical_phase_validator` and `repeated_repo_graph_build`, two
  `graph-analysis` detectors that encoded one downstream project's
  conventions (phase markers and its graph-build function names). The
  feature now adds seven detectors, and `repeated_expensive_initialization`
  no longer treats those function names as expensive initialization.

### Licensing

- `LICENSE` now holds the Apache License 2.0 text (it replaces the
  `LICENSE.md` placeholder). Release archives ship `LICENSE`.
- Release archives include `THIRD-PARTY-LICENSES.txt`, the license notices
  of every crate compiled into `auditor`
  (`tooling/release/third_party_licenses.py`).

## 1.0.0 - Stable Release

The first stable release. The CLI commands and exit codes, report v5,
finding-ID v3, baseline v2, gate result 2, SARIF output, and the stable
`auditor.toml` keys now change incompatibly only in a major version (see
`docs/reference/stability.md`). Layer 6 review, review CLI flags, and MCP
tools stay experimental. Layer 6 has not passed its free-route qualification
matrix; `docs/known-gaps.md` records why.

### Release engineering

- A `v*` tag builds `auditor` for Linux x86_64 and macOS (arm64 and x86_64),
  publishes a GitHub release with `SHA256SUMS`, and takes its notes from the
  matching CHANGELOG section. The tag must equal the workspace version.
- Publishing hygiene: tracked files no longer name the private downstream
  project, and `AGENTS.md`/`CLAUDE.md` are tracked without being ignored. The
  `docs/distribution.md` checklist passes.
- Ruby, Julia, and Shell stay in the catalog as structural-only (Tier 3).
- Release archives include a `review/` directory (case templates, prompt
  bundle, review brief, and an example Mistral routes file) so the optional
  AI review works from a downloaded binary.
- New user guide (`docs/guide.md`): install, first scan, baselines and the
  gate, CI, SARIF, configuration, and opting into AI review. Detector counts
  in the README and references now match the catalog (114: 52 gating, 62
  full-only).

### Algorithmic evidence

- Algorithmic findings reported a hard-coded `mutation_count: 0`, which Layer
  6 reviewers read as proof that a collection never changes. The field is now
  `direct_mutation_count`, and `repeated_sort_in_loop` adds
  `calls_receiving_collection`: loop calls passed the collection itself (for
  example a Rust helper given a `&mut` parameter by reborrow) whose effect is
  unresolved. Finding identities are unchanged.

### Layer 6 prompt caching

- Case conversations keep their system instruction fixed; continuation and
  repair instructions are sent as the turn's trailing user message, so each
  conversation is an append-only prompt prefix. The Mistral adapter sends a
  per-conversation `prompt_cache_key` (a digest of the fixed opening), and
  provider records report `cached_input_tokens`. Adapter version 45.

### Layer 6 offline improvement

- Offline prompt improvement is a manual candidate process: a tracked
  manifest per candidate (`eval/review-improvement/<id>/`) binds the owned
  asset, candidate bundle, routes, training cases, held-out cases, and
  pre-registered decision rules. `compare_prompt_candidate.py` compares
  baseline and candidate arms, scores only qualified runs that reach closure,
  and refuses held-out runs until the candidate is frozen.
  `generate_lifecycle_heldout_repo.py` generates four held-out lifecycle
  fixtures. The stale description of the removed v1 Pareto optimizer is gone
  from the maintained docs.
- First candidate: an idempotence and visible-body clarification for the
  lifecycle `intervening_mutation` negative boundary. Rejected at the
  training gate on Mistral Medium 3.5: it regressed the intervening-mutation
  case (2 of 3 correct against 3 of 3); the production wording is unchanged.
- `intervening-mutation-v2` (repetition-interval definition) and
  `consumer-distinction-v1` (downstream-caller observability) were also
  rejected at their training gates. The intervening-mutation regressions cited
  the finding's `mutation_count: 0`, which led to the algorithmic evidence fix
  above. `intervening-mutation-v3`, the same wording on the corrected
  evidence, was rejected too (intervening-mutation case 1 of 3 against 2 of 3).
- Implementation plans under `docs/v1/plans/` are now tracked.

### Application corpus slice

- The ecosystem manifest gains an `applications` slice: six pinned,
  permissively licensed applications (lychee, apprise, miniflux,
  Dependency-Track, Uptime Kuma, Hoppscotch), one per Tier-1 language, for
  detectors the library corpus cannot measure. `run.py --slice applications`
  scans it and keeps only each repository's named detectors' findings;
  `labels.py precision --scan` accepts both scan labels.
- `network_call_without_timeout`: a bare `axios(x)` reads `x` as the config
  unless it is a URL string, so `axios(config)` is no longer reported.
- Source roles: a directory named `*-examples` (`extra/push-examples/`) holds
  examples.

### Dependency and error hygiene

- Added full-only `unused_declared_dependency` (Layer 4): a production
  dependency in `Cargo.toml`, a published `package.json`, `go.mod`,
  `pyproject.toml`, or `pom.xml` that no source file under the manifest's
  directory imports. Dependencies that load by convention (type packages,
  compiler helpers, tool plugins, drivers, servers, `-sys` crates, Maven
  starters and bindings) and `cargo-machete`/`cargo-udeps` ignore lists are
  respected. Findings sit on the manifest line with a second location in
  the package's sources.
- Added full-only `discarded_fallible_result` (Layer 1): an explicitly
  discarded result that is the only failure signal (Rust `let _ =` on
  durability I/O, Go blank `Sync`/`Commit`/`os.WriteFile`/`os.Rename`, Java
  boolean `File` operations), a floating promise from a same-file `async`
  function or a `.then` without a rejection handler (JS/TS), and a Python
  coroutine never awaited or a dropped `asyncio.create_task` reference.

### Layer 3 additions

- Added full-only `quadratic_front_removal`: front removal (`pop(0)`,
  `shift()`, `ArrayList.remove(0)`, `Vec::remove(0)`) from a provably
  array-backed local sequence inside a loop, for Rust, Python, JavaScript,
  TypeScript, and Java (Go reslices and is not applicable).
- Added full-only `catastrophic_backtracking_regex`: regex literals whose
  pattern admits exponential backtracking (an unbounded repetition whose
  body splits text more than one way), with shared regex-literal extraction
  for Python, JavaScript, TypeScript, and Java and a nested-quantifier
  analyzer. Go and Rust engines are linear-time and not applicable.

### Network timeouts

- Added full-only `network_call_without_timeout` (Layer 4, High) for the
  Tier-1 languages: HTTP calls and clients whose library default never
  times out, from a per-language client table (Python per Bandit B113,
  Go's default client, `java.net.http.HttpClient`, axios and Node
  `http`/`https`, reqwest). Bounded-default clients are not listed; Tier-2
  cells are deferred.
- The v1.0 detector families' catalog entries live in
  `registry_families.rs`.
- A repository-root `support/` directory now has the tooling role, like a
  root `build/` (fmt's release and generator scripts); nested `support`
  directories stay code. The corpus rescan resolved 19 findings there.

### Test-quality detectors

- Added full-only `test_without_assertion` and `tautological_assertion`
  (Layer 2, category `test_quality`) for Rust, Python, JavaScript,
  TypeScript, Go, and Java, on a shared test-scope fact: each framework's
  test functions, a uniform call view, and the repository functions that
  assert on a test's behalf (directly or transitively). The Tier-2
  frameworks are deferred.
- `test_without_assertion` reports a test with no assertion, failure call,
  compile-time check, test-context use, or asserting helper; skipped,
  expected-failure, smoke, constructor-exists, placeholder, and (TypeScript)
  type tests are exempt.
- `tautological_assertion` reports literal self-comparisons, operands
  compared with themselves under an equality they cannot redefine (identity,
  structural, reflective), always-true literal assertions, and assertions
  that a mock returns its own stubbed value. A self-comparison under a
  user-defined `equals` is a reflexivity test and is not reported.
- A new `TestScope` role view lets these detectors emit in test files.

### Precision gate: detectors the corpus cannot measure

- For most gating detectors short of a decisive sample, every corpus
  finding is already labelled. Such a detector now gates only with no live
  false positive in the languages it gates in, plus its positive and
  adversarial negative capability cases; `detector_gate_lint.py` enforces
  it (operator decision).
- Fixed first: eight gating detectors with tooling-script false positives
  emit in production files only; a repository-root `build/` is tooling and
  a root `integration/` is test code; `go_errcheck` skips `bytes.Buffer` and
  `strings.Builder` writes (errcheck's defaults) and result-less methods;
  `out_parameter` skips Java `@Override` methods; `hardcoded_credential`
  skips property-key names and symbol-only character sets;
  `repeated_format_allocation` reports only calls that run every
  iteration; `loop_invariant_regex_compilation` skips lazily cached
  compiles; `debugger_in_production` skips a trap followed by `abort()`.
  The corpus rescan resolved 31 findings, all targeted, and added none.
- `excessive_nesting_depth` (41/1) and `go_errcheck` (27/0) now meet the bar.
- Moved to `full`: `insecure_cryptography`, `sql_injection`,
  `nested_quadratic_iteration`, `sibling_call_redundancy`,
  `resource_not_closed`, `identifier_tersity`. Scoped to `full` by
  language: `duplicate_import` (TypeScript), `exit_in_production` (Python),
  `unused_assignment` (C++), `stringly_typed_parameter` (Python,
  TypeScript). Catalog: 52 gating, 55 full-only.
- `out_parameter`: its remaining 54 Java findings are labelled and the
  operator confirmed its labels as final over 15 second-model disputes; it
  meets the bar at 81/1.

### Precision gate: confidence bound and audited oracle agreement

- The precision bar is now a 95% lower confidence bound of at least 85%
  (Wilson), still counting every unverified pre-label as a false positive;
  a detector is below the bar when its best-case precision is under 90%. A
  thin sample no longer passes: 10 clean labels bound precision at only 72%.
- `oracles.py overlap` runs the reference tools over every corpus finding.
  When a detector's oracle-confirmed labels audit clean (at least 30, no
  false positive), its unlabelled confirmed findings count as true positives
  (`labels.py precision --claims eval/ecosystem/oracle-claims.json`).
  Agreement holds for the metric detectors and fails for judgment detectors
  such as `random_for_security` and `console_output_in_production`.
- ruff no longer refutes `too_many_locals` or `function_local_import`: it
  skips loop and `with` targets and `__import__` calls, which EA counts.
- `too_many_parameters`: Python `*args: T` and `**kwargs: T` no longer count,
  matching their untyped forms, pylint, and ruff.

### Corpus expansion: four repositories per language

- Twenty more pinned repositories, two per Tier-1 and Tier-2 language (fd,
  hyper, flask, black, hono, immer, eslint, fastify, gin, bubbletea,
  commons-lang, jackson-core, moshi, okhttp, Polly, Dapper, zlib, lz4,
  nlohmann/json, yaml-cpp).
- Fixes the new repositories exposed: a Java package named `tools` is no
  longer a tooling directory; `single_include/` and `amalgamation/` builds
  are generated; `unreachable_code` treats `case`/`default`/`goto` labels as
  entry points and preprocessor directives as trivia; `too_many_parameters`
  skips C# extension receivers; constant names may contain digits.
- Second round from labelling the new repositories:
  - Parameters of lambdas, closures, and function-pointer types no longer
    count as the enclosing function's (Polly lambdas, libuv locals).
  - C++ export macros (`class YAML_CPP_API Emitter {`) no longer make
    tree-sitter read a class as a function definition.
  - Generated files are recognised by their notice in any language
    ("autogenerated", "Generated automatically by", `@generated`, Go's
    "Code generated ... DO NOT EDIT").
  - Source roles: `thirdparty/` is vendored; `snippets/` is example code;
    `fuzz/` and hyphenated test modules (`maven-tests`) are tests;
    `profiling/`, `perf-testing/`, and `*-benchmarks` files are benchmarks.
  - `magic_literal_cluster` masks multi-line `/* .. */` comments;
    `console_output_in_production` treats `build/` and `Makefile.js` as
    tooling.
  - Oracles: lizard covers Tier-2 function complexity and parameters, and
    rust-code-analysis covers C/C++ returns and Halstead.
- Third round:
  - Returns inside lambdas and closures belong to the closure
    (`too_many_returns`); class bases resolve to the simple name of a
    qualified type.
  - `naming_convention` honours Rust `#![allow(non_camel_case_types)]` in the
    module or an ancestor, and okhttp's `-Name.kt` file prefix.
  - Kotlin: KDoc and strings are masked for `!!` and casts;
    `unsafe_type_cast` flags hard `as` casts, not `as?`;
    `kt_require_check_misuse` flags only a constant first argument.
  - `cpp_raw_ptr_ownership` skips placement new;
    `java_inefficient_string_buffer` reports `new StringBuffer` in code only.
  - `collection_parameter_mutation`: TypeScript `#private`, `private`,
    nested, and unexported functions are private.
  - Markdown links: escaped brackets, spaced destinations, URI schemes,
    percent-encoding, and directory index pages; anchors follow rendered
    heading text, setext headings, and duplicate suffixes.
  - `unused_assignment` keeps a try body's stores live for its handlers;
    `unreachable_code` skips C# local functions; `constant_condition` allows
    `do { } while (true)`; `dead_code` skips underscore-prefixed Rust items.
- Second-model labeller: `opencode/deepseek-v4.1-flash-free` replaces the
  withdrawn `step-5-preview-free`, whose earlier checks still count.
- Fourth round, from adjudicating the second model's disputes:
  - C# functions are named by their name, not their return type (`Task`);
    a TypeScript arrow callback is not named after its parameter; C++
    namespace macros (`NLOHMANN_JSON_NAMESPACE_BEGIN`) no longer parse as a
    function named `namespace`.
  - `too_many_parameters` skips Kotlin default values, type-level parameters
    in a Rust `where` clause, and Kotlin `external` / C# `extern` bindings.
  - `cognitive_complexity`'s breakdown sums to its total;
    `excessive_nesting_depth` measures a nested function on its own;
    `class_structure_metrics` reads multi-line and attributed class headers
    and resolves a base name to a same-file or uniquely named class.
  - `magic_literal_cluster` keys literals by value (`0x10` is `16`);
    `section_divider_comment` ignores ASCII diagrams;
    `intermediate_collection_materialization` no longer flags
    `filter(..).length` counts; anonymous functions are reported as
    `<anonymous>`.
  - Build and tool configuration (`eslint.config.js`, `build.gradle.kts`,
    `Makefile.js`, Gradle `build-logic/`) is tooling; `test-*.ts` is a test.
  - Review bundles carry the current scan's message; 174 checks given a
    stale message were dropped.
  - `todo_unimplemented_in_production` skips tooling files;
    `stale_link_reference` resolves MyST `(label)=` targets and site routes
    (MkDocs directory URLs, Eleventy `pages/`).
- Fifth round, from labelling the samples no oracle covers (C/C++ and
  Kotlin complexity): C++ access-specifier macros
  (`JSON_PRIVATE_UNLESS_TESTED:`), macros alone before a `namespace` or
  `class` (`FMT_END_EXPORT`), and block comments inside C/C++ `#define`
  bodies are blanked before parsing, so they no longer produce functions
  named `switch`, `namespace`, `elm`, or an include guard.
- Review bundles show a function-level finding's whole function (up to 120
  lines) instead of a 20-line window, which left long functions `unclear`.
- The last 268 pre-labels without a second-model check were labelled blind by
  Claude Sonnet subagents (`model:claude-sonnet-5-5`), since OpenCode's free
  tier kept refusing CLI batches; `model_review.py run --skip-checked-by`
  skips findings the same model already checked through another provider.
  `unsafe_type_cast` (already `full`) is now below the bar.
- Sixth round, after confirming the 217 disputed labels: detectors may
  emit in production files only (`RoleApplicability::Production`), and
  every detector's declared emission roles are enforced on its findings, so
  index-backed detectors no longer report outside them. `duplicate_import`
  and `magic_literal_cluster` skip tooling scripts and configs.
  `large_file` gates C only in the `full` profile: its C false positives are
  vendored code (zlib `contrib/minizip`, jq's dtoa).
- Seventh round, from rechecking the unverified pre-labels on whole
  functions and a strict metric recount:
  - `god_module` counts free functions only; Rust `impl`/`trait` functions,
    C++ qualified definitions, and JavaScript class-expression members are
    methods.
  - `excessive_nesting_depth` puts a `catch` at its `try`'s depth, and
    Rust's `?` nests nothing.
  - Kotlin trailing lambdas and Go `func` literals are scored as their own
    functions for cognitive complexity, cyclomatic count, and nesting depth.
  - `too_many_statements` no longer counts Kotlin comments as statements.
  - `churn_complexity_hotspot` nets moved code out of line growth and does
    not count a file's initial size: a file split, or a refactor that
    shrinks a recently created file, no longer makes it a growing hotspot.
  - Promoted to gating (operator decision): `cognitive_complexity`,
    `npath_complexity`, `halstead_information_density`,
    `class_structure_metrics`, `source_duplicate_detection`,
    `too_many_returns`, and `too_many_statements`; the metric detectors emit
    in production files only. This repository's own findings were refactored
    first, and its remaining duplicate-detector false positives recorded as
    reviewed debt.
  - `escape_hatch_type_density` skips TypeScript `any` inside generic
    brackets or after `extends`; `architectural_layering` drops Java imports
    named only in their declaration (javadoc `{@link}` imports).
  - `magic_literal_cluster` is demoted to `full` in every language.
  - Operator labelling decisions: methods are not top-level definitions;
    `any` as a generic default or constraint is not an escape hatch; a cycle
    through javadoc-only imports is not a cycle; HTTP status and character
    codes are magic.
- Per-language precision gating: a gating detector below the bar in some
  languages runs there only in the `full` profile
  (`DetectorSpec::full_profile_in`), its gating report claims say so, and the
  gate lint re-judges the remaining languages. `magic_literal_cluster`
  (Java, Python, TypeScript) and `exit_in_production` (JavaScript,
  TypeScript) are scoped this way instead of being demoted.

### Tier-2 L1–L2 (v1.0 Task 15, part 2)

- Kotlin, C#, C, and C++ now meet the Tier-1 standard for L1 and L2. Every
  supported cell has a positive capability case, an adversarial negative, and
  a corpus sample. Every other cell, L3–L5 included, has a reviewed
  not-applicable or deferred reason.
- Corpus: eight new pinned repositories, two per Tier-2 language (okio,
  koin, Newtonsoft.Json, Serilog, libuv, jq, fmt, leveldb).
- L1 API identity for the Tier-2 grammars (`kotlin_api_identity.rs`,
  `csharp_api_identity.rs`, `c_api_rules.rs`). It covers exits, console
  output, TODOs, evaluation, shell, bind-all, world-writable modes, insecure
  random, certificate and TLS settings, SSH host keys, unsafe deserialization
  (BinaryFormatter, `TypeNameHandling`), XXE, weak crypto, unclosed streams,
  argument asserts, and debugger traps. Also added: Kotlin and C# SQL sinks
  (JDBC, ADO.NET, Entity Framework raw SQL, Dapper), empty catches and
  branches, and dead stores (C `volatile`, address-taken, and reference-bound
  locals and C# `out`/`ref` arguments are skipped).
- L2: clones for C; class metrics for Kotlin, C#, and C++; duplicate
  imports for all four, with `#include` judged per preprocessor branch;
  collection mutation for Kotlin and C# (.NET mutator names; extension
  receivers exempt); naming for Kotlin and C# (.NET design guidelines);
  section dividers for all four; Kotlin Halstead, NPATH, locals, returns, and
  statements; C/C++ locals.
- Deferred on the corpus sample: Tier-2 magic literals (byte layouts and
  specification constants), and Kotlin, C#, and C++ clones (per-type
  overloads, mirror pairs, DSL families). Kotlin and C# `out_parameter` are
  also deferred: their parameter-object writes are designed output buffers.
- Fixes found by the Tier-2 work:
  - C/C++ function symbols were named after their last parameter
    (`uv__io_poll(…, int timeout)` was `timeout`); this re-keys C/C++
    findings.
  - C++ `class` bodies are class symbols, so their methods are no longer
    counted as top-level functions by `god_module`.
  - Kotlin soft-keyword names (`value`, `data`) count as Halstead operands.
  - Bison, yacc, and flex output is generated source.
  - Comment borders framed by `+` or `|` are diagrams, not section dividers.
  - Source roles: .NET `*.Tests` projects, Kotlin `src/<name>Test` source
    sets, and `*Test` class files are tests; a `.h` header with C++ syntax
    is C++.

### Precision Fixes and Tier-1 Extension (v1.0 Task 15, part 1)

- `source_duplicate_detection`: tokenizes function bodies only (signatures,
  generics, `where` clauses, and Python docstrings no longer form clones);
  pairs occurrences only within one build package; skips row tables, handlers
  of different states of one Java enum state machine, and clones among
  locale files. Extended to Go, JavaScript, and TypeScript; Java deferred
  (15/24 on the corpus sample).
- `magic_literal_cluster`: masks block strings in every Tier-1 language
  (ripgrep's raw-string help text), JS/TS regex literals, object and JSX
  attribute values (ESLint `detectObjects: false`), and literal-only lookup
  tables; skips `hashCode` and locale files. Extended to Go, Java,
  JavaScript, and TypeScript.
- `class_structure_metrics`: NOC alone no longer flags abstract bases or
  mixins. Extended to Java, JavaScript, and TypeScript (56/56 on the sample).
- `duplicate_import`: Python `TYPE_CHECKING` imports are type-only, so a
  runtime `__main__` import beside one is not a duplicate. Extended to Python
  and Java.
- `collection_parameter_mutation`: Java and TypeScript require a
  collection-typed parameter on a non-private, non-recursive function
  (`StringBuilder` sinks and `visited` sets dominated the probe). Extended to
  Java and TypeScript; JavaScript and Go deferred with reasons.

### Tier-1 Parity Complete (v1.0 Task 14, part 4)

- `stringly_typed_parameter` for JavaScript (JSDoc types), TypeScript, and
  Java (Rust deferred after 2 self-scan false positives on external string
  kinds); Java `range_for_each_iteration`
  (`IntStream.range(..).forEach`).
- Every Tier-1 cell is now supported, not applicable with a reviewed reason,
  or deferred with the specific fact or precision fix it waits on: 380
  semantic, 14 exact lexical, 31 partial, 160 not applicable, 57 deferred.

### Tier-1 L2 Qualification (v1.0 Task 14, part 3)

- Qualified on every Tier-1 language: `section_divider_comment`;
  `naming_convention` for Go, JavaScript, and
  TypeScript (JS/TS classes in PascalCase, no snake_case functions);
  `escape_hatch_type_density` for TypeScript; `too_many_returns` and
  `too_many_statements` for Python, JavaScript, and TypeScript;
  `too_many_locals` for Python, counting assigned names and loop targets.
- Not extended, with the reason recorded: detectors already below the
  precision bar (`class_structure_metrics`, `collection_parameter_mutation`,
  `duplicate_import`, `magic_literal_cluster`, `source_duplicate_detection`),
  cells whose corpus sample fell below it (`identifier_tersity`,
  `sibling_call_redundancy`), `identifier_verbosity` (type and variant names
  read as verbose locals in our own Rust), and `out_parameter` for Python/JS/TS (rejected
  after a 347-finding probe of designed parameter writes). Rust duplicate
  and function-local imports, Rust collection mutation, Go tersity, JS/TS
  function-local imports, and Java `god_module` are not applicable.
- `source_duplicate_detection` no longer misses clones that start at an
  exported, decorated, or attributed function (33 new Rust and Python
  findings).
- `sibling_call_redundancy` skips test functions.

### Tier-1 L1 Fill (v1.0 Task 14, part 2)

- Java `assert_in_production` (an `assert` on a public method's parameter),
  `empty_catch_block` (PMD `EmptyCatchBlock`), and `empty_conditional_branch`
  for Go and Java.
- `assignment_in_operand` for JavaScript, TypeScript, and Java (a condition
  that is a bare assignment; ESLint `no-cond-assign` default).
- `hardcoded_credential` and `hardcoded_tmp_directory` for JavaScript,
  TypeScript, Go, and Java (`security_literal_rules.rs`).
- `sql_injection` and `unused_assignment` for JavaScript, TypeScript, and
  Java. `unused_assignment` now judges only function locals in every
  language: a field, package variable, or captured variable may be read by a
  call that never names it.
- `no_await_in_loop` is grammar-backed for JavaScript and TypeScript too,
  replacing the lexical rule: `while` loops and early-exit, sleeping, and
  element-dispatch loops are no longer reported, nor are awaits of promises
  the loop already holds. TypeScript `require_atomic_updates` qualified.
- 11 new corpus findings pre-labelled (8 TP, 3 FP); 1 lexical await finding
  in an axios stream reader resolved.

### Reviewed Tier-1 Cell Dispositions (v1.0 Task 14, part 1)

- 89 Tier-1 cells move from `unsupported` to `not_applicable`, each with a
  reviewed reason (`registry_cell_dispositions.rs`): documentation detectors
  on source, Python-only constructs elsewhere, constructs a language lacks
  (`catch`, `eval`, `await`, debugger statements, function-local imports),
  and the conventions recorded in Tasks 9–13.
- 15 cells keep `unsupported` with a specific reason: `dead_code` and
  `orphan_symbol` need cross-file resolution, `churn_complexity_hotspot`
  needs history, and Go `unhandled_termination_in_production` was rejected.
- `language-support.md` lists the not-applicable reasons.

### Python and Rust Dataflow, Literal, and Naming Rules (v1.0 Task 13, part 4)

- `unused_assignment` for Python and Rust (`dead_stores.rs`), with Rust
  format-string captures counted as reads and `#[cfg]`-alternative bindings
  skipped. Its first self-scan found an ignored `--auditor` flag in
  `tooling/r3_a2_hot_path_measure.py`, now removed.
- `sql_injection` for Python (DB-API `execute*`, Django `raw`, SQLAlchemy
  `text` with f-string, `+`, `%`, or `.format` query building) and Rust
  (sqlx, Diesel, rusqlite, and postgres query methods given `format!`).
- `hardcoded_credential` and `hardcoded_tmp_directory` for Rust.
  Environment-variable names and targets that name a credential's location
  (`API_KEY_ENV`) are no longer credentials, and a temp path must be the
  directory or a path under it (`/tmp:rw,..` mount specs are not).
- `naming_convention` for Rust (RFC 430) and Python (PEP 8), skipping
  trait-impl items, Python methods of subclasses, and `ctypes`/`cffi` binding
  modules.
- 2 new corpus findings pre-labelled (1 TP, 1 FP).

### Python and Rust Collection and Optimization Facts (v1.0 Task 13, part 3)

- Python support for 6 L3 detectors on annotated lists:
  `repeated_sort_in_loop`, `sort_for_extremum`, `full_sort_for_top_k`,
  `repeated_collection_traversal`, `hot_path_linear_membership`, and
  `invariant_collection_copy_in_loop`. Python subscripts (`xs[0]`, `xs[-1]`,
  `xs[:k]`, `sorted(xs)[0]`) are selections and `needle in xs` is a membership
  test (`structure_operands.rs`).
- Python `manual_collection_copy_loop` (`range(len(source))` copies) and
  `map_entry_opportunity` (`setdefault`); Rust
  `immutable_string_concatenation_in_loop` (`format!` rebuilding),
  `known_size_preallocation` (`Vec::with_capacity`), and
  `range_for_each_iteration` (`(a..b).for_each`).
- A collection rebound inside a loop is no longer a repeated sort.
- No corpus change; baseline digest only.

### Rust L1 API Identity and Statement Shapes (v1.0 Task 13, part 2)

- Resolved API identity for Rust (`rust_api_identity.rs`): plain, aliased,
  and grouped `use` trees.
- Rust support for 12 L1 detectors: `shell_execution`,
  `console_output_in_production`, `debugger_in_production` (`dbg!`),
  `bind_all_interfaces`, `world_writable_file`, `random_for_security`,
  `request_no_cert_verify`, `ssl_with_bad_defaults`,
  `ssh_no_host_key_verification`, `insecure_cryptography`,
  `empty_conditional_branch`, and `no_await_in_loop`.
- Rust programs (`main.rs`, `build.rs`, `src/bin`, and packages without a
  library target) may print and exit; `exit_in_production` resolves
  `process::exit` through `use`.
- 6 new corpus findings pre-labelled, all false positives (multipart-boundary
  `fastrand`, the RFC 6455 WebSocket SHA-1, an opt-in proc-macro debug dump).

### Python L1 API Identity and Statement Shapes (v1.0 Task 13, part 1)

- Resolved API identity for Python (`python_api_identity.rs`): `import`,
  `import as`, and `from .. import` bindings, with shadow-aware builtins.
- Python support for 8 L1 detectors: `exit_in_production`,
  `console_output_in_production`, `debugger_in_production`,
  `todo_unimplemented_in_production`, `insecure_cryptography`,
  `empty_catch_block`, `empty_conditional_branch`, and `no_await_in_loop`.
- Python program code (`__main__.py`, `#!` scripts, `if __name__ ==
  "__main__":` blocks, and the functions `pyproject.toml` script entry points
  name, one import hop deep) may print and exit.
- `empty_catch_block` reports only broad `except Exception: pass` handlers in
  Python; narrow handlers are the ask-forgiveness idiom and bare `except:`
  stays with `try_except_pass`.
- 8 new corpus findings pre-labelled (7 true positives, 1 false positive).

### JavaScript/TypeScript Collection and Optimization Facts (v1.0 Task 12, part 2)

- JavaScript support for `repeated_sort_in_loop`, `sort_for_extremum`,
  `full_sort_for_top_k`, `repeated_collection_traversal`,
  `hot_path_linear_membership`, `nested_cross_collection_traversal`,
  `cache_miss_opportunity`, `loop_invariant_computation`, and
  `immutable_string_concatenation_in_loop`; JavaScript and TypeScript support
  for `intermediate_collection_materialization`,
  `manual_collection_copy_loop`, and `repeated_format_allocation`.
- JavaScript parameters typed by a JSDoc `@param {T} name` block carry that
  type as their declaration, as TypeScript annotations do.
- JS/TS calls carry their resolved API identity and the expression that
  consumes them (`xs.filter(p).length > 0`).

### JavaScript/TypeScript API Identity (v1.0 Task 12, part 1)

- Resolved API identity for JavaScript and TypeScript (`js_api_identity.rs`):
  ES imports and CommonJS `require` (including destructuring and `node:`
  specifiers), with shadow-aware runtime globals.
- JavaScript and TypeScript support for 14 L1 detectors: `shell_execution`,
  `exit_in_production`, `todo_unimplemented_in_production`,
  `eval_in_production`, `unsafe_deserialization`, `bind_all_interfaces`,
  `world_writable_file`, `random_for_security`, `request_no_cert_verify`,
  `ssl_with_bad_defaults`, `ssh_no_host_key_verification`,
  `xml_xxe_vulnerable`, `insecure_cryptography`, and `resource_not_closed`;
  TypeScript support for `console_output_in_production`,
  `debugger_in_production`, `empty_catch_block`, and
  `empty_conditional_branch`.
- JS/TS programs (`#!` scripts, private or bin-only packages, `bin` sources)
  and build configuration files no longer report console output or process
  exits; this resolves the 4 labelled docs-site and root-script false
  positives. Process-boundary paths match `scripts/`, `bin/`, and `tooling/`
  as path segments, including at the repository root.
- `blocking_io_in_async_context` (Java) also covers lambdas passed to
  operators of unscheduled Reactor and RxJava chains.

### Java Collection, Effect, and Optimization Facts (v1.0 Task 11, part 2)

- Java support for 26 L3–L5 detectors: `repeated_sort_in_loop`,
  `sort_for_extremum`, `full_sort_for_top_k`, `repeated_collection_traversal`,
  `hot_path_linear_membership`, `n_plus_one_query_in_loop`,
  `loop_carried_copy_accumulation`, `nested_cross_collection_traversal`,
  `architectural_layering`, `package_coupling_metrics`,
  `loop_artifact_rewrite`, `latest_only_unbounded_cache`,
  `repeated_parse_scan_serialize`, `blocking_io_in_async_context`,
  `per_item_external_io`, `repeated_expensive_initialization`,
  `request_path_repository_scan`, `cache_miss_opportunity`,
  `repeated_format_allocation`, `manual_collection_copy_loop`,
  `eager_fallback_computation`, `intermediate_collection_materialization`,
  `loop_invariant_computation`, `known_size_preallocation`,
  `bulk_collection_extension`, and `map_entry_opportunity`; Go support for
  `latest_only_unbounded_cache` (`append`, `cache[len(cache)-1]`, reslicing).
- The structure index records each Java call's resolved identity, each Java
  local's declared type, and the lambdas that run on the shared fork-join
  pool; Java imports resolve to source files by package path.
- Java local variable symbols are named by their declarator
  (`List<String> names = new ArrayList<>()` declared `ArrayList`).
- `nested_cross_collection_traversal` no longer reports an inner collection
  assigned inside the outer loop (it is per element).

### Java API Identity (v1.0 Task 11, part 1)

- Resolved API identity for Java (`java_api_identity.rs`): type names resolve
  through single-type, static, and wildcard imports and `java.lang`, and
  receivers through declared variable types (`var` included) and JDK return
  types, so `rt.exec(..)` after `Runtime rt = Runtime.getRuntime()` is
  `java.lang.Runtime.exec`. A class the file declares shadows implicit
  bindings.
- Java support for 15 L1 detectors: `shell_execution`, `exit_in_production`,
  `todo_unimplemented_in_production`, `console_output_in_production`,
  `eval_in_production`, `unsafe_deserialization`, `bind_all_interfaces`,
  `world_writable_file`, `random_for_security`, `request_no_cert_verify`,
  `ssl_with_bad_defaults`, `ssh_no_host_key_verification`,
  `xml_xxe_vulnerable`, `insecure_cryptography`, and `resource_not_closed`,
  each with Java positive and adversarial negative capability cases. PMD
  semantics where a rule exists (`DoNotTerminateVM`, `SystemPrintln`,
  `CloseResource`).
- Corpus (jsoup, gson): 35 TODO comments, 2 library stderr warnings, and 1
  `java.util.Random` MIME boundary (labelled false positive); no XXE report
  for a transformer serializing an in-memory DOM.

### Go Collection, Effect, and Optimization Facts (v1.0 Task 10, part 2)

- Go support for 18 L3–L5 detectors: `repeated_sort_in_loop`,
  `sort_for_extremum`, `full_sort_for_top_k`, `repeated_collection_traversal`,
  `hot_path_linear_membership`, `n_plus_one_query_in_loop`,
  `nested_cross_collection_traversal`, `loop_artifact_rewrite`,
  `repeated_parse_scan_serialize`, `per_item_external_io`,
  `repeated_expensive_initialization`, `request_path_repository_scan`,
  `cache_miss_opportunity`, `repeated_format_allocation`,
  `loop_invariant_regex_compilation`, `loop_invariant_computation`,
  `bulk_collection_extension`, and `manual_collection_copy_loop`.
- The structure index records each Go call's resolved API identity, every Go
  import spec (grouped imports no longer collapse to the first), Go index and
  slice expressions, and the switch subject of each Go case arm.
- Go standard-library collection functions are read as methods on their
  first argument (`sort.Ints`, `slices.Contains`, `slices.ContainsFunc`); a Go
  call that passes a slice otherwise may mutate it. Go effects are classified
  by resolved identity.
- `repeated_format_allocation` and `loop_invariant_computation` no longer
  report a call guarded by a switch on, or an equality test of, the key of a
  loop over a map: it runs at most once per loop.
- Go topology: an import under the module path of the file's nearest
  `go.mod` resolves to its package directory, so `architectural_layering`
  and `package_coupling_metrics` support Go.
- `package_coupling_metrics` no longer reports a package with fewer than two
  coupling edges (all four labelled corpus false positives).

### Go API Identity (v1.0 Task 10, part 1)

- New shared fact: resolved API identity (`api_identity.rs`). A call's
  callee, or a composite literal's type, is resolved through the file's
  import bindings to a module-qualified name such as `os/exec.Command`, so an
  aliased import cannot hide a call and a value named like a package cannot
  fake one. Go is resolved first.
- Go support for 11 L1 detectors: `shell_execution`, `exit_in_production`,
  `todo_unimplemented_in_production`, `console_output_in_production`,
  `bind_all_interfaces`, `world_writable_file`, `random_for_security`,
  `request_no_cert_verify`, `ssl_with_bad_defaults`,
  `ssh_no_host_key_verification`, and `resource_not_closed`, each with Go
  positive and adversarial negative capability cases. The rules follow gosec
  where one exists (G102, G106, G204, G301/G302/G306, G402, G404).
- Go `panic` is not claimed by `unhandled_termination_in_production`: every
  corpus site guarded an API contract.
- `console_output_in_production` demoted to `full`: with the Go finding the
  sample reached 10, and the JavaScript docs-site and root build scripts
  (`www/`, `mm.mjs`) put it below the bar. Catalog: 52 gating, 55 full.

### Precision Gating

- The detector architecture contract now enforces the 90% precision bar: a
  gating detector that is `below_bar` overall or in any language fails the
  lint. It reads the committed snapshot `eval/ecosystem/precision.json`.
- `unhandled_termination_in_production` demoted to `full`: 0 of 8
  corroborated Rust samples were true positives (idiomatic contract panics and
  `unreachable!()`).
- Second-model pass completed over all live samples; 10 detectors meet the
  bar. `detector-promotion.md` lists each gating detector's standing.
- `model_review.py run` skips findings the model already checked and copies
  their verdicts to the output, so a resumed run re-sends nothing and
  `labels.py checks` keeps the earlier checks.
- Fixed: per-language standings counted corroborated labels as unverified.
- Fixed: the detector architecture lint did not see detectors declared in a
  `FindingText` constant (`class_structure_metrics`, `out_parameter`,
  `collection_parameter_mutation`) and failed.
- The Rust `#[cfg(test)]` module-graph parser moved from `source_discovery.rs`
  to its own `rust_test_modules.rs`.
- `source_duplicate_detection` no longer reports a clone whose occurrences
  differ in more distinct bindings than the `too_many_parameters` budget (the
  extraction would need that many parameters); names declared inside the
  clone do not count. New `differing_bindings` evidence.
- `source_duplicate_detection` demoted to `full`: 38/43 (88%) after that fix.
  The default catalog is now 53 gating and 54 full-only.

### Task 7 Follow-ups
- Path exemptions are no longer built in. EA's own exemptions for detector
  sources and tooling scripts moved to the repository's `auditor.toml`, under
  the new experimental `[exclusions.<name>]` section (`paths`, `detectors`).
- A Rust module declared `#[cfg(test)] mod name;` is test code in its own
  file, along with the modules it declares.

### Dispute Review Fixes
- `npath_complexity` now matches PMD on every Java function the oracle
  disputed. Conditions split into true and false outcomes, a labeled `break`
  rejoins after its labeled statement, each case label is a path (with
  fall-through into the next group), and boolean values outside conditions
  count.
- `cognitive_complexity` no longer scores `return name;` as a labeled jump.
  It detects Java and Python recursion and counts recursion once per
  function, as the whitepaper specifies.
- `source_duplicate_detection` reports clones that can reasonably be
  extracted. Parameter lists, same-name forwarding keyword arguments, and
  literal-only argument lists are not tokenized, and clones pairing async
  functions with their sync twins are not reported.
- `scripts/` and `tools/` directories are tooling at any depth. Files named
  `*Benchmark` take the Benchmark role.
- `naming_convention` exempts `static final` mutable collections, arrays,
  builders, atomics, and loggers (Google Java Style §5.2.4).
  `magic_literal_cluster` skips Python docstrings, and
  `repeated_expensive_initialization` skips constructors built from the loop
  variable.
- `duplicate_import` no longer reports repeated CommonJS member requires
  (`require('./utils').methods`).
- All 47 disputed pre-labels are resolved: the fixes removed 19 and the rest
  were reviewed and confirmed human-final. 7 detectors meet the precision
  bar, and the oracles agree with 582 of 585 checks. The live PMD donor test
  passes against the locked 7.27.0-SNAPSHOT build.

### Baseline Precision Measurement
- Pre-labelled 1,303 corpus findings across 50 detectors and published
  `docs/reference/detector-precision.md`. All verdicts are model pre-labels
  pending human confirmation. Sampled precision rose from 68% to 81%, and
  corpus findings fell from 2,187 to 1,774.
- Added `labels.py confirm` to accept reviewed pre-labels as final human
  verdicts. The precision report now also shows pre-label precision. Stale
  labels no longer fill a sampling bucket.

### Label Corroboration
- Labels gain an optional `checks` list and a `corroborated` status. The
  status is set when an independent check agrees with the pre-label and none
  contradicts it. A disputed pre-label stays `pre_label`.
  `labels.py checks FILE --source oracle:TOOL|model:ID` records checks, and a
  re-run from the same source replaces its earlier checks.
- Added `tooling/ecosystem/oracles.py`. It provisions pinned reference tools
  under `target/oracles/`: lizard, ruff, complexipy, rust-code-analysis,
  gocognit, errcheck, goconst, ESLint with sonarjs, jscpd, and PMD. It
  compares their facts with labelled findings by file, line, function, and
  value. One oracle covers each detector and language.
- Added `tooling/ecosystem/model_review.py`. It asks a second, free model
  for blind labels through a sandboxed `opencode run`: no tools, no
  instruction files, and no view of the first verdict. It records them as
  `model:` checks. A model cannot corroborate its own pre-labels.
- A detector meets the precision bar only when it holds even if every
  uncorroborated pre-label turned out false. The new `unverified` standing
  means the bar depends on labels nobody has checked yet. This stops
  corroboration that happens to cover mostly true positives from inflating
  precision.
- First pass: 610 oracle checks and 848 second-model verdicts on live labels
  corroborate 583 pre-labels. `halstead_information_density`, `large_file`,
  and `naming_convention` meet the bar. `assert_in_production`,
  `function_local_import`, and `identifier_stutter` fall below it. 215
  disputed pre-labels wait for human review. The oracles show that
  `cognitive_complexity` departs from the Sonar specification and that Java
  `npath_complexity` diverges from PMD; neither is fixed yet.
- Split the labelling tool by responsibility: `label_store.py` (label files
  and validation), `label_workflow.py` (sampling and verdict recording),
  `label_precision.py` (standings), and the `labels.py` CLI, whose commands
  are unchanged.

### Convention-Based Detector Fixes
- `cognitive_complexity` now follows the Sonar specification: one increment
  per `switch`/`match`, no nesting credit for `else if`, one per run of like
  boolean operators, and it fires above 15 rather than at 15. Python
  `except`, Java enhanced `for`, and Java/Go bare `else` are counted.
  JavaScript `function` expressions are scored as their own functions, like
  arrow functions.
- `npath_complexity` matches PMD: paths that return, throw, or raise add
  instead of multiplying, `break`/`continue` rejoin after their loop, `&&`/`||`
  conditions and ternaries add paths, brace-less branches and Java `else if`
  count, and `catch` handlers do not.
- `assert_in_production` is Python only and skips type-narrowing asserts.
- `function_local_import` exempts `try/except ImportError` guards,
  `TYPE_CHECKING` blocks, and a reason comment beside the import.
- `identifier_stutter` reports public Go and Rust items that repeat their
  module name instead of locals that repeat their function's words.
- `out_parameter` reports writes through a parameter's field or element, not
  rebinding the parameter, and exempts owner types and enum State handlers.
- `go_errcheck` ignores returned, assigned, and tested results.
  `too_many_parameters` skips `@overload` stubs. `blocking_io_in_async_context`
  skips tooling files.
- Role directories with a leading `_` (`_examples/`) take their role, and
  `.eaignore` files (gitignore syntax) exclude paths from the audit.
- Each decision and its source is documented in the detector reference. On
  the corpus, 6 detectors now meet the precision bar and disputed pre-labels
  fell from 215 to 47.
- The JavaScript/TypeScript `function_complexity` oracle is
  rust-code-analysis; lizard mis-spanned TypeScript functions.
  `labels.py sample` resets a label whose finding moved to another site.
- The self-gating baseline is rekeyed to the new analysis contract digest,
  with resolved entries pruned. One churn hotspot,
  `tooling/ecosystem/label_workflow.py`, is accepted pending the Task 8
  review of the quartile rule: each file split moves the cutoff onto the
  next file.

### Detector Precision Fixes
- `unreachable_code`: a Python `if/elif` chain without `else` is no longer
  treated as exhaustive. A generator-marker `yield` after `raise`, and
  `#[cfg(..)]`-gated returns, no longer count as dead code.
- `constant_condition`: `for (; true; ..)` is accepted as an infinite loop.
- `duplicate_import`: TypeScript `import type` and namespace imports are kept
  apart from value and named imports.
- `go_repeated_string_literal`: quotes inside raw strings, struct tags,
  comments, and rune literals are ignored.
- `stale_link_reference` and `stale_section_anchor`: rustdoc intra-doc links,
  template placeholders, link titles, code spans, fenced code (including
  fences inside list items), extensionless `.md`/`.mdx` doc-site links,
  slug-routed pages, GitHub heading slugs, `{#id}` ids, and HTML anchors now
  resolve correctly.
- `section_divider_comment`: needs a run of three identical rule characters;
  Rust attributes and `quote!` interpolations are not comments.
- `collection_parameter_mutation`: a consumed result of a Python
  `None`-returning mutator (`opts = options.update(..)`) is a copy, not a
  mutation.
- `architectural_layering`: self-imports are no longer graph edges.
- `nested_quadratic_iteration`: `range(..)` has no collection identity.
- `orphan_symbol` and `dead_code`: trait-impl associated items,
  `#[global_allocator]` statics, and `#[allow(dead_code)]` are honoured.
- `console_output_in_production`: `bin/`, `scripts/`, `sandbox/`,
  `examples/`, fixture directories, and gulpfiles count as tooling.
- `too_many_parameters`: JS `function` expression callbacks and TypeScript
  function-type parameters are not credited to the enclosing function.
- `halstead_information_density`: volume alone needs difficulty of at least
  15. Julia operators are now counted.
- `class_structure_metrics`: depth alone no longer flags a method-less leaf.
- `magic_literal_cluster` and the complexity metrics skip Rust test functions
  in `src/`. `magic_literal_cluster` also ignores keyword-parameter defaults.
- `naming_convention`: Java `serialVersionUID` is exempt.

### Precision Labelling
- Added `tooling/ecosystem/labels.py` with these commands:
  - `sample`: seeded samples of at most 30 findings per detector and
    language, round-robin across repositories; top-ups never redraw a
    labelled finding;
  - `context`: samples with finding fields and source excerpts, as JSON
    Lines for a human or model labeller;
  - `import`: apply verdicts; model verdicts stay `pre_label`, and only a
    `human:` labeller can make one `final`;
  - `precision`: per-detector precision from final labels against the
    90% / 10-label gating bar, with stale labels excluded.
- Label files (`eval/ecosystem/labels/<detector>.json`) follow the internal
  `ecosystem-labels.schema.json` and key on finding-ID v3.

### Ecosystem Runner
- Added `tooling/ecosystem/run.py` with these commands:
  - `fetch`: fetch each corpus repository by exact commit;
  - `scan`: per-detector, per-language, and per-repository summaries;
  - `diff`: new, resolved, re-keyed, and unchanged findings;
  - `compare`: build two revisions and diff their corpus scans.
- Added the opt-in `Ecosystem` workflow. The full corpus scans in about 40 s
  in a release build.

### Go Signature Parameters
- Go parameter symbols now come only from a function or method's own
  parameter list. Results, function types in signatures, and closure
  parameters reuse the same node kinds and were counted as parameters, so
  `func SetHeader(key, value string) func(http.Handler) http.Handler` was
  reported with 6 parameters. The corpus scan found this; the fix removes
  22 false `too_many_parameters` findings from chi and cobra.

### Loop Collection Identity
- Algorithmic loop facts took a free-function wrapper's name as the
  collection, so two loops over `sorted(a)` and `sorted(b)` looked like the
  same collection to `nested_quadratic_iteration`. Transparent wrappers
  (`sorted`, `reversed`, `list`, `tuple`, `set`, `frozenset`, `enumerate`,
  `iter`) now resolve to their first argument. A wrapper around a composite
  (`set(a) | set(b)`) has no single identity.

### Deterministic Churn
- Git churn (`churn_complexity_hotspot`) previously used
  `git log --all --since "14 days ago"`. It now covers commits reachable from
  `HEAD` within 14 days of `HEAD`'s commit time. The old wall-clock window and
  all-refs walk made one commit's scan change from day to day and between
  clones, so the gate failed overnight with no code change.

### Ecosystem Corpus
- Added `eval/ecosystem/manifest.json` and
  `contracts/ecosystem-corpus.schema.json` (internal). The manifest pins 12
  permissively licensed repositories at release-tag commits, two per Tier-1
  language, for detector precision measurement. A stdlib validation test runs
  in CI.

### Finding-ID v3 Freeze
- Finding titles must be `&'static str`. `Finding::builder` and the shared
  constructors reject formatted titles at compile time.
- `dead_code` used a formatted title (`Unreferenced <kind> \`<name>\``). Its
  title is now the static "Unreferenced symbol", and the kind and name moved
  to the message and evidence. `dead_code` finding IDs change once.
- Added a golden-ID test that pins 321 finding IDs across every detector's
  supported language arms (`crates/auditor-detectors/testdata/finding-ids-v3.tsv`).
  Any identity change now fails CI.

### Go Identity And Parameters
- Go method findings anchor on `Receiver::Method`, using the receiver's base
  type (pointer and type parameters dropped), so same-named methods on
  different receivers no longer share an anchor. Go finding IDs inside
  methods change once.
- Go semantic facts record one parameter symbol per name (`a, b int` is two
  parameters), record variadic parameters, and no longer count the method
  receiver. `too_many_parameters` previously undercounted grouped Go
  parameters and counted the receiver.

### Stability Policy
- Added `docs/reference/stability.md`, which classifies every CLI command,
  artifact, config surface, MCP tool, and contract schema as stable,
  experimental, or internal and states the versioning policy. A contract test
  fails when a schema is unclassified.
- CLI help marks Layer 6 commands `[experimental]` and donor commands
  `[internal]`; every MCP tool description starts with `Experimental:`.

### Fixed
- Review agenda planning no longer plans finding-free capability-limitation
  candidates as `shared_root_cause` cases. No template can instantiate them, so
  with `graph-analysis` any `missing_input` capability failed the review stage
  with `TriggerMismatch`; they are now suppressed as
  `unsupported_evidence_neighborhood`.
- The detector architecture lint recognises the `finding!` macro and the
  lexical constructor, and the workspace is clippy-clean, so the CI test,
  architecture, and clippy steps pass again.

## 0.9.19 - EA-041 Stable Finding Identity

### Finding-ID v3
- Finding IDs no longer hash line numbers. A scan-owned anchor resolver binds
  each finding to its innermost enclosing declaration (qualified path; Rust
  methods qualified by `impl` type) or a normalized line digest, adds a
  line-content subject for statement-level findings, and assigns occurrence
  ordinals. Unrelated edits that shift lines no longer re-key findings.
- Clone classes, dependency cycles, layer edges, packages, and repeated Go
  literals use path-independent structural anchors. The orphan-symbol,
  naming-convention, and package-coupling titles are static; names, zones,
  and limits moved to messages and evidence.
- Credential findings key on the assigned name, so secret-bearing lines are
  never hashed into identity.
- `FINDING_ID_VERSION` is 3. Reports and baselines from earlier versions are
  rejected; use `baseline migrate`.

### Baselines, Gates, And SARIF
- Baseline v2 entries record title, message, and identity. Comparison carries
  accepted findings across renamed anchors, moved files, and shifted ordinals
  on unique matches only; carried pairs appear in the report baseline
  envelope (schema 2) and gate result (schema 2) and are not new.
- Added `baseline rekey` and `baseline migrate`.
- SARIF results carry `partialFingerprints` (`engineeringAuditor/v3`,
  `primaryLocationLineHash`) set to the finding ID.

### Verification
- Every detector's capability repository keeps identical IDs under
  line-shifting perturbations; a self-scan reproduction that churned 57 of 181
  findings now churns none.

## 0.9.18 - Layer 6 v2 Native Review Pipeline

### Evidence-Constrained Review
- Added the v2 case lifecycle for reviewer, challenger, response, ensemble,
  comparison, portfolio, disposition, and read-only escalation flows, with
  bounded evidence packets, deterministic probes, host-owned finalization, and
  offline verification.
- Replaced model-facing raw JSON and strict-output/text fallbacks with
  provider-native task tools, deterministic Markdown context and corrections,
  append-only native ledgers, explicit revision tools, and lossless transcript
  rendering.
- Added the shared typed Markdown renderer, provider-free case planning and
  inspection, lifecycle evaluation corpus/configuration, deterministic mock
  review (including case-kind-aware shadow-template gaps), live-evaluation
  tooling, and v2-only CLI/MCP/report surfaces.

### Release Evidence And Operations
- The release line is gated by the immutable adapter-version-43 16-run
  anonymous lifecycle evidence and the Spec 17 ensemble evaluation artifacts.
- A post-attribution binary-identity refresh remains an explicitly tracked
  anonymous-route availability follow-up. Its pre-semantic route failures are
  not provider-quality evidence and do not invalidate the immutable gate; see
  `docs/known-gaps.md`.

## 0.9.17 - EA-039 Finding-ID Source-Request Resolution

### Source-Excerpt Type System (EA-039 Part 2 Closeout)
- Added `SourceExcerptKind` enum (`Symbol` / `Finding`) with
  `#[serde(rename_all = "snake_case")]` to `SourceExcerpt`. The kind makes the
  excerpt id's namespace explicit at the type level.
- Renamed `SourceExcerpt.symbol_id` to `SourceExcerpt.id` (honesty rename;
  the field is no longer always a symbol id). `SourceRequest.symbol_id` wire
  field is unchanged.
- `read_symbol` sets `kind: SourceExcerptKind::Symbol`.

### Finding-ID Resolution
- `read_source_excerpt_for_finding` fully rewritten: takes
  `(finding_id, relative_path, line)`, looks up the `MapFile` by relative
  path, enforces digest parity (`StaleSourceSnapshot` on mismatch), reads a
  bounded ±2/+8 line window, enforces per-excerpt `max_excerpt_bytes` clamp,
  and constructs the id as `finding-excerpt:<path>:<start>-<end>`. Dead
  `_byte_start` computation deleted.
- `resolve_source_requests` now accepts non-`symbol:` prefixed requests:
  finding IDs (`EA-<hex16>`) are looked up in `brief.findings` and resolved
  via the bounded source-window path. The existing `symbol:` branch is
  unchanged.
- Egress provenance format updated to `repository-source:<path>:<id>`.
- Digest computation uses `excerpt.id` instead of `excerpt.symbol_id`.

### Documentation
- `docs/reference/report-contract.md`: new "Engine-Resolved Evidence Refs"
  subsection documenting the three evidence-ref kinds (`symbol:`,
  `EA-<hex16>`, `audit-artifact:`) and their resolution semantics.
- `docs/known-gaps.md`: new "Finding-ID Source-Request Resolution" section
  documenting the engine's finding-id -> `SourceExcerpt` resolution and the
  `SourceExcerptKind` enum.
- `docs/v1/plans/v0.9.16-human-digest-format.md`: fixed two stale "SHA-256"
  references to `stable_digest_bytes (FNV-1a 64-bit)`.

### Tests
- 5 new tests: `source_excerpt_kind_serializes_round_trip`,
  `source_request_finding_id_resolves_to_excerpt`,
  `source_request_finding_id_excerpt_kind_is_finding`,
  `source_request_finding_id_with_stale_source_returns_error`,
  `source_request_finding_id_excerpt_over_max_excerpt_bytes`.
- All 404 workspace tests pass; `cargo fmt` and `cargo clippy` clean.

## 0.9.15 - R3-A2 Hot-Path Unblock And Capability-Execution Integrity

### Pre-Analysis Scoping
- Added `ScopeFilter` to `auditor_detectors::source_discovery` and threaded
  `request.scope` into `analyze_repository_run`. A `ScanScope::Changed`
  invocation now prunes source discovery to the requested paths before any
  detector runs, so `scanned_files` is bounded by the scope and not the whole
  repository. Previously a two-file changed scope walked the entire repo and
  reported `scanned_files` equal to the full file count (the downstream R3-A2
  dossier measured ~18 s on a 2,481-file repo where Python finished in ~150 ms).
- `ScanScope::All` is unchanged; `discover_sources` and the existing public API
  are backward-compatible.

### Capability-Execution Integrity
- `RepositoryMetadata` detectors (`historical_phase_validator`,
  `tooling_overlap`, graph-only detectors when supplied with their required
  graph/co-edit inputs, etc.) now report `capability_execution.status =
  "executed"` when any eligible file was visible, instead of `not_applicable`.
  The report validator's `validate_execution_status` accepts the new shape
  (`analyzed == 0` with `not_applicable > 0`).
- Fixes the v0.9.14 graph-enabled full-scan abort
  `finding references a detector that was not executed: historical_phase_validator`
  that prevented the v0.9.14 candidate from replacing the v0.8.11 pass-close pin.

### R3-A2 Contract And Measurement
- Added an executable CLI contract test
  (`r3_a2_changed_scope_bounds_scanned_files_and_validates_with_graph_inputs`,
  `crates/auditor-cli/src/main.rs`, gated on `graph-analysis`) that asserts
  `scope == "changed"`, `scanned_files == 2` (bounded subset), every detector
  has a `capability_execution`, `historical_phase_validator` is `executed`, the
  report validates, and consecutive runs are byte-identical.
- Added `tooling/r3_a2_hot_path_measure.py` for repeatable scoped-vs-full
  latency measurement and `scanned_files` bounding verification.

## 0.9.14 - Live Provider Gate And Tool-Mode Repair

### Provider Qualification And Dispatch
- Added a provider-native `noop` capability probe after strict structured-output
  qualification. Qualification remains valid when the optional tool probe fails,
  while `tools_supported` controls assessment-mode promotion per target. The
  nonce and tool probes each have a bounded 512-token response allowance; this
  does not reduce the separate route or review-stage output budgets.
- Promoted tool-qualified OpenAI Responses, Anthropic, and OpenCode targets to
  the bounded assessment tool vocabulary while preserving strict JSON for later
  stages and unqualified targets.

### Bounded Repair And Evidence
- Preserved failed provider-native tool blocks for one `:repair-1` re-emission.
  Tool repair closes the vocabulary to assessment emission plus `noop`, forbids
  new source requests, and rejects semantically incomplete exact-once question
  coverage. Tool and strict repairs share route budgets, record mode plus repair
  count, and never exceed one repair per shard.
- Normalized `emit_atomic_assessment` and `lookup_symbol` calls into the existing
  assessment contract without adding schema-forbidden diagnostic fields.
- Added transcript mode/repair evidence and reconciled engineering-review v1
  with partial reviews and the additive v0.9.12-v0.9.14 metadata.

### Operator Gate
- Added `tooling/live_provider/run_live_anon_gate.py`, an explicit fail-closed
  anonymous OpenCode gate covering strict plus tool qualification, bounded
  source requests, repair cardinality, tool-mode completion, and byte-identical
  Markdown sidecars across consecutive runs.
- Exercised the gate against the live anonymous endpoint. Nemotron 3 Ultra Free
  and North Mini Code Free passed strict plus `noop` qualification, including
  the question-ID-bearing tool contract, but the gate correctly rejected their
  incomplete shard coverage. The retired `hy3-free` route now rejects anonymous
  authentication; credentialed Mistral remains unqualified in an environment
  without a provisioned key.

## 0.8.11 - Distribution And Test-Lane Closeout

### Public Test Contract
- Made the public workspace all-feature suite hermetic by classifying the 21
  executable donor parity groups as explicit provisioned integration tests.
  Donor status/disposition, adapter, capability, graph, and unit tests continue
  to execute on ordinary hosted runners.
- Kept live parity strict: the explicit lane executes every provisioned group
  serially with both donor and graph features, and missing tools, checkouts,
  snapshot agreement, output, or positives remain hard failures.

### Distribution Boundary
- Made hosted CI test the complete all-feature workspace from a clean checkout
  and made the self-hosted lane preserve its provisioned ignored donor
  checkouts.
- Added a tracked-only export qualification contract and reconciled the v0.8
  release boundary without changing detector semantics or report contracts.

### Qualification
- Passed 195 hermetic all-feature detector tests plus all 21 explicitly invoked
  live parity groups across the 12 mapped executable donors.
- Repeated 185-file full scans are byte-identical at 133 findings and roughly
  125 MiB peak RSS. The reviewed gating set is 78: the prior 76 plus two
  legitimate crate-root churn/complexity hotspots created by the v0.8.10
  implementation history. No v0.8.11-owned file has a finding.
- A staged tracked-only export passes the all-feature workspace suite, Clippy,
  architecture/documentation gates, donor policy without donor checkouts, a
  complete full scan, and the zero-new baseline gate. Hosted CI now fetches
  full history so VCS-dependent detectors are not silently deactivated by a
  shallow checkout.

## 0.8.10 - Deterministic Core And Evidence Freeze

### Report And Evidence Contracts
- Replaced report v3 with report v4 and evidence graph v1 with graph v2. Reports
  now carry typed file/detector capability metadata, finding provenance,
  analysis coverage, digest-bound analysis inputs, graph completeness, and a
  supporting fact or measurement for every finding.
- Added standalone baseline v1 with explicit report/semantic/graph/finding/role
  identity and a nonempty review reason per accepted finding. Report-shaped
  baseline fixtures and incompatible identities fail closed.
- Replaced ad hoc report parsing and serialization with structured JSON owners,
  checked-in schemas, deterministic ordering, and bounded CLI/report modules
  shared by the CLI and MCP adapter.

### Lifecycle And Precision
- Enforced release of all five phase-scoped repository indexes after their last
  registered owner and retained A/B/A sequential-scan isolation coverage.
- Preserved exact fact spans only when detector evidence supplies one; other
  findings explicitly declare line or repository precision without fabricated
  columns. Evidence graphs store digests and keys rather than raw source.
- Bound authoritative repo-graph and co-edit content into analysis identity
  while keeping reviewed baseline artifacts outside semantic analysis identity.

### Repository Gate
- Added a tracked reviewed-debt baseline and CI zero-new gate. Baselines cannot
  accept a detector bug or false-positive mechanism.
- Split pass-owned report and CLI hotspots until the full self-audit reported
  zero findings in new v0.8.10 implementation owners. Declarative Rust `json!`
  and struct field mappings are no longer mislabeled as behavioral source
  clones.

### Real-Repository Precision
- Resolved Markdown links relative to their referring document and reused the
  same parsed target for file and anchor validation.
- Required read-to-write value lineage plus a loop-invariant destination for
  whole-artifact rewrite findings, and rejected parent/child collection walks
  from pairwise quadratic classification.
- Passed all 22 serial live donor/status groups across 12 mapped executables.
  A locked 156-file public donor scan emits 348 reviewed candidates; a
  2,422-file untracked full scan emits 631 in 1m52s at 729,700 KiB peak RSS.
- Refreshed the standalone reviewed baseline to 76 gating findings. The full
  185-file self-audit emits 131 reviewed candidates with no pass-created
  finding in report/CLI/MCP or changed detector/support owners.

## 0.8.9 - Language Coverage, Cost Models, And Promotion

### Typed Algorithmic Cost
- Added source-spanned constant, linear, logarithmic, sort, repeated-call,
  copy/allocation, and unknown terms with deterministic sum, maximum, and
  product composition for every Layer 3 fact.
- Added full-only `loop_carried_copy_accumulation`,
  `invariant_collection_copy_in_loop`, `repeated_recursive_subproblem`, and
  `nested_cross_collection_traversal` with strict ownership, mutation,
  feasible-path, cardinality, and Cartesian-output negatives.

### Language And Profile Qualification
- Expanded proven generic file/metric and managed-loop arms with real
  grammar-specific capability repositories; unsupported cells now carry
  family-specific missing-capability reasons in the generated reference.
- Promoted `duplicate_import`, `out_parameter`, `excessive_nesting_depth`, and
  `unreachable_code` to gating after exact live donor and repository review.
  Every retained full-only owner has an explicit typed risk disposition.

### Precision And Repository Review
- Passed all 22 serial donor parity/status groups across all 12 mapped live
  donors. A locked public donor scan and a 2,421-file untracked scan found no
  changed-owner false-positive mechanism.
- Refactored touched cost, evidence, dispatch, and support-classification code
  until the self-audit reported zero findings in v0.8.9 implementation owners.

## 0.8.8 - Layer 5 Optimization Proofs

### Typed Optimization Facts
- Replaced the line-oriented performance cache with one scan-scoped
  `OptimizationIndex` derived from structure and algorithmic facts. Every fact
  carries explicit invariance, purity, mutation, ordering, failure, receiver,
  size, and result-use evidence.
- Added full-only `loop_invariant_computation`, `known_size_preallocation`,
  `bulk_collection_extension`, and `map_entry_opportunity` owners.
- Split flow, API semantics, collection semantics, syntax helpers, and fact
  collection into bounded findings-free modules; detector-owned constructors
  remain uniform.

### Language And Donor Qualification
- Added proof-backed semantic optimization arms across Rust, Python,
  JavaScript, TypeScript, Go, Java, and Kotlin. Unsafe alias, side-effect,
  nested-control, coercion, and failure variants fail closed.
- Added strict live Ruff PERF402, Pylint `consider-using-generator`,
  GolangCI-Lint `prealloc`, Detekt `SpreadOperator`, and Clippy
  `regex_creation_in_loops` overlap cases. All 17 serial live donor groups pass.

### Precision And Repository Validation
- Added executable positive, adversarial negative, profile, and per-language
  boundaries for all 103 default detectors and all 1,545 language cells.
- Reviewed every Layer 5 candidate from a 2,421-file external scan. The sole
  candidate was an enclosing-call format false positive; the real source shape
  is now a negative regression and the affected crate reports zero Layer 5
  findings.
- Split AST field normalization and registry metadata into adjacent owner
  modules. The 165-file self-audit reports no findings in v0.8.8-owned files.

## 0.8.7 - Layer 4 Systems And Architecture

### Typed Effects And Topology
- Added one compact scan-scoped `SystemsIndex` for typed I/O, network, parse,
  serialization, cache, initialization, state-build, async, request-path, and
  value-lineage facts.
- Replaced path-inferred layering with explicit collectors and resolved import,
  imported-symbol, inheritance, and re-export edges. Incomplete local topology
  fails closed and carries completeness/provenance evidence.
- Added full-only `blocking_io_in_async_context`, `per_item_external_io`,
  `repeated_expensive_initialization`, and `request_path_repository_scan`.

### Language And Donor Qualification
- Added semantic Rust, Python, JavaScript, and TypeScript capability boundaries
  for the shared systems/effect owners and architecture topology.
- Added exact live Import Linter 2.13 forbidden-layer and Dependency Cruiser
  18.1.0 cycle parity.
- Provisioned Ruff 0.15.20 and Pylint 4.1.0-dev0 and promoted their C901
  complexity and nested-block mappings to strict live parity.
- Built the locked PMD 7.27.0-SNAPSHOT checkout against a repo-local Java 21
  runtime and added exact native `AvoidReassigningParameters` parity.
- Reduced the typed donor status to 12 live mapped donors, no unavailable
  mapped donors, and two explicit non-parity references (Semgrep and Deptrac).

### Qualification And Resource Lifecycle
- Added exact positive, semantic-negative, language-boundary, repository
  integration, and live-donor tests for every new owner. The all-feature serial
  detector gate passes 203 tests, including all 12 mapped live donors.
- Split effect classification, extraction, and topology construction into
  findings-free support modules while retaining one typed systems index and one
  finding constructor per detector owner.
- Released the algorithmic index after its final owner and retained the existing
  early-release lifecycle for complexity, performance, systems, and structure
  indexes. Changed-range filtering no longer allocates a range vector per
  finding.
- Calibrated the four new effect owners on a 2,421-file external repository.
  Eight invalid call/loop/request-path classifications became semantic-negative
  regressions; the remaining five Layer 4 cycle findings were verified against
  bidirectional source imports.
- The 157-file all-feature self-scan completes in 12.2 seconds at 110,756 KiB
  peak RSS and emits 169 maintained review candidates, with no findings in
  v0.8.7-owned implementation files.

## 0.8.6 - Layer 3 Algorithmic Semantics

### Typed Cost Facts
- Added one compact `AlgorithmicIndex` derived from the resolution-aware
  structure index. It proves loop dependence, collection shape and cardinality,
  alias-aware mutation, selection ownership, callback effects, and repository
  query dependence without retaining another source or AST copy.
- Migrated all six existing Layer 3 owners off line-oriented performance
  heuristics and added full-only `full_sort_for_top_k`. Extremum and top-k
  selection now have disjoint `k = 1` and `k > 1` ownership.
- Split fact orchestration, structure navigation, mutation/effect proof, and
  semantic classification into bounded findings-free modules. Detector owners
  retain their uniform finding constructors and typed evidence contracts.

### Precision And Verification
- Added positive and semantic-negative Rust capability cases plus executable
  TypeScript boundaries for all seven owners. Mutation, aliases, comparator
  ordering, unknown callback effects, bounded membership, invariant queries,
  and write/batch APIs fail closed.
- Preserved member-path collection identities, excluded loop-local values and
  intervening mutable calls, and added branch-aware control paths so mutually
  exclusive match/switch and conditional arms cannot be aggregated.
- Added exact live ESLint 10.6.0 `no-duplicate-imports` parity and JSCPD 4.2.5
  minimum-token clone parity. Both adapters verify executable versions and
  locked source snapshots and require positive and clean native agreement.
- Corrected the remaining donor status table: Ruff, PMD, Pylint, and Deptrac
  are environment-blocked; Semgrep remains explicitly non-parity.
- The 149-file full self-audit emits 202 maintained review candidates and zero
  findings in v0.8.6-owned implementation files.
- A 4,191-file external full scan emits 844 review candidates. Direct source
  review validated all 19 surviving Layer 3 findings and drove six false
  positives from collapsed member paths, loop-local sorts, intervening mutable
  calls, and mutually exclusive branches into executable regressions.

## 0.8.5 - Layer 2 Structural Semantics

### Resolution-Aware Structure Index
- Added one lazy scan-owned index for symbols, references, imports, calls,
  assignments, literals, control regions, and compact function metrics.
  Explicit aliases, re-exports, same-name symbols, tests-as-context, and
  unresolved dynamic imports have executable regressions.
- Migrated parameter width, sentinel string parameters, local imports,
  sibling-call redundancy, parameter mutation, class metrics, god modules,
  structural counts, naming declarations, and orphan symbols off private
  line parsers and onto shared semantic facts.
- Added full-only `duplicate_import`, `out_parameter`, and
  `excessive_nesting_depth` owners with positive, semantic-negative, profile,
  and language-boundary coverage.

### Bounded Clone Classes
- Replaced three overlapping duplicate-source implementations with one
  function-bounded rolling-hash engine. It preserves exact spans, validates
  consistent identifier/literal renaming, extends maximal matches, suppresses
  overlap, requires repeated semantic anchors, and emits one canonical finding
  per clone class.
- Excluded Rust test ancestry and top-level data tables, raised the minimum
  useful span to eight lines, and suppressed homogeneous unique-call/data lists.
  The self-scan fell from 204 initial clone candidates to 47 without losing
  normalized, intra-file, or multi-file capability cases.

### Donor And Audit Evidence
- Added source-backed mappings for ESLint `no-duplicate-imports`, PMD
  `AvoidReassigningParameters`, Pylint `too-many-nested-blocks`, and JSCPD's
  rolling clone engine. These verify locked donor rule sources and execute EA
  positive/clean cases without mislabeling unavailable tools as live parity.
- Corrected package abstractness for visible declarations and suppressed
  main-sequence claims for packages with no classifiable types.
- Compensated direct `else if` branches, excluded implicit `self`/`cls`/`this`
  receivers from parameter-mutation findings, and required sentinel literals
  to share conditional or match regions with the named parameter.
- The 144-file self-scan completes in 11.42 seconds at 104,236 KiB peak RSS,
  emits 202 full-profile candidates, and reports zero findings in pass-owned
  v0.8.5 implementation files.
- A 4,190-file external full scan completes in 108.78 seconds at 710,912 KiB
  peak RSS and emits 838 review candidates. Precision regressions removed all
  passive string-conversion and flat `else if` findings; the remaining dominant
  families are 453 bounded clone classes and 218 formal-parameter width
  findings, which remain review inputs rather than automatic defect claims.

## 0.8.4 - Layer 1 Correctness And Security

### Bounded Control Flow
- Added full-only `unreachable_code` and `constant_condition` owners over ten
  grammar-backed languages. Both fail closed on parser errors and token-only
  scans.
- Reported one finding per contiguous unreachable segment after `return`,
  `throw`, `break`, or `continue`, while preserving non-executing hoisted
  declarations and comment trivia.
- Detect literal constant branch and loop predicates while allowing intentional
  `while true`/`while (1)` sentinels.

### Semantic L1 Owners
- Rebuilt the previously partial or syntax-sensitive correctness and security
  arms over the canonical scan-owned AST cache. The migrated owners cover Rust/Python assertions,
  Python security calls, Rust termination and dead-code analysis, JavaScript
  control/error statements, and Go SQL, crypto, and local dead-store analysis.
- Removed detector-private file reads, parser construction, token fallbacks,
  and immutable pre-populated AST dependencies from the migrated paths.
- Added typed Rust attribute references and trait-implementation boundaries to
  dead-code analysis, including serialization callback and inline-test scope
  regressions.

### Verification
- Added real positive/negative repository cases for both IDs and executable
  language boundaries for every catalog cell.
- Added exact live `unreachable_code` parity with Rust Clippy and Detekt full
  analysis on atomic Rust and Kotlin projects.
- Converted an external-repository false positive on a trailing comment after
  `continue` into a focused regression test.
- Dispositioned every changed finding in a 4,190-file external full scan. Test
  attribute ancestry and serialization callback false positives are fixed;
  the 51 residual findings are direct rule matches.
- The final 134-file self-scan reports 144 gating and 313 full findings, with
  zero findings from the migrated v0.8.4 owners.

## 0.8.3 - Semantic Frontends And Capability Truth

### Semantic Frontends
- Added compact scan-owned semantic facts for symbols, references, calls,
  assignments, control regions, loops, imports, and literals. Facts retain
  source spans into the canonical AST buffer rather than copied source text.
- Added a Kotlin tree-sitter frontend and conformance coverage across all 13
  grammar-backed source languages.
- Kept parse errors typed and non-panicking while preserving lazy per-tree
  fact construction.
- Source discovery now honors repository `.gitignore` and `.ignore` files
  without applying machine-global excludes, preventing ignored runtime output
  from entering repository-wide analysis.

### Executable Catalog
- Added explicit `semantic`, `exact_lexical`, `partial`, `unsupported`, and
  `not_applicable` modes for every detector-language pair.
- Added an executable positive/negative boundary for every claimed language
  arm and a runtime emission gate that rejects matches from unsupported arms.
- Replaced broad language defaults with conservative, capability-proven
  policies; known NPath and Halstead frontend gaps are now reported as
  unsupported instead of implied coverage.
- Added `auditor catalog` JSON output and rebuilt language documentation from
  that runtime catalog, removing layer-wide Python defaults.
- Separated finding emission roles from read-only context roles. Dead-code and
  orphan-symbol analysis now resolves references from tests and examples
  without authorizing findings in those files.
- Extended the documentation gate to compare all 1,335 default-catalog
  detector-language claims against executable Rust metadata.

## 0.8.2 - Scan-Scoped Repository State

### Repository State
- Replaced persistent per-line strings with one shared contiguous source buffer
  and compact line offsets per file; detector line views now borrow source text.
- Made repository ASTs share the file cache's source allocation and cache one
  function-span array beside each parsed tree.
- Removed process-global and thread-local span/body caches. All retained state
  is owned by one scan and released through normal scope teardown.
- Replaced cloned source-role file vectors with borrowed slices over
  role-partitioned scan storage while preserving public discovery order.

### Shared Analysis
- Added one phase-scoped complexity index that computes branch, cognitive,
  NPath, and Halstead facts together and supplies churn-complexity scoring.
- Kept the existing phase-scoped performance index and releases both fact
  indexes after their final registered owner.
- Added allocation-sharing, line-semantics, role-view, public-order, and
  sequential scan-isolation regression tests.

## 0.8.1 — Role-Aware Performance Qualification

### Detector Contract
- Added source-role contract v1 and role-aware owner scheduling; report schema
  v3 now records repository role counts and detector role applicability.
- Added full-only `n_plus_one_query_in_loop`,
  `manual_collection_copy_loop`, `eager_fallback_computation`,
  `intermediate_collection_materialization`, and `range_for_each_iteration`.
- Reclassified and hardened `hot_path_linear_membership` as an L3 algorithmic
  detector using typed parameter and loop-dependence evidence.
- Added exact live Clippy parity for `manual_memcpy`, `or_fun_call`, and
  `needless_collect`, plus Detekt `ForEachOnRange` parity.

### Resource Safety
- Bounded repeated clone families to one deterministic representative finding.
- Ordered clone hash buckets so consecutive scans select the same canonical
  representative and stable finding identity.
- Reduced `FileCache` to one persistent source representation and scoped the
  lazy performance index to its detector phase.
- Replaced definition-by-definition repository rescans in dead-code and orphan
  analysis with linear identifier-frequency passes.
- Rejected loop-variant indexed collections from `repeated_sort_in_loop`,
  removing a self-audit false positive in generated documentation tooling.
- Added opt-in owner timing and explicit scan-boundary cache cleanup.

## 0.8.0 — Donor-Parity Calibration Overhaul

### Detector Calibration
- Recalibrated detector thresholds and gating logic against donor tool output
  (ruff, semgrep, bandit, detekt, pmd, eslint, pylint, rust-clippy, deptrac,
  jscpd, golangci-lint, agent-slop-lint) to close false-positive gaps
  identified by direct donor-parity comparison
- Fixed previously noisy families that over-fired relative to donor baselines:
  `sql_injection`, `hardcoded_credential`, `cache_miss_opportunity`,
  `hot_path_linear_membership`, and `source_duplicate_detection`
- Added `donor_parity` module and capability-contract test scaffolding
  (`capability_cases_correctness`, `capability_cases_structural`,
  `capability_contract`) to lock in gating vs. full-profile detector behavior
  against oracle-derived expectations
- Added `oracle_runner` for running donor oracles against fixtures and
  comparing findings deterministically
- Split `security.rs` into focused submodules (`security_sql`,
  `security_crypto`, `security_process`, `security_rules`) with dedicated
  test modules
- Added `dispatcher` module and `design_metrics` module; added AST Halstead
  complexity support (`complexity::halstead`)
- Reorganized fixtures under `fixtures/donor-parity/<tool>/` per donor tool,
  replacing the prior ad hoc detector fixture layout

### Validation
- Live dogfood scan of a large production repository (full profile) produced 2,569
  findings, ~1,560 of which are code findings, across 4,130 scanned files,
  confirming the calibration overhaul reduces false positives without
  regressing true-positive coverage

## 0.6.8 — Clone Detection & AST Complexity

### Item 2: Cognitive Complexity — AST-Precise
- Rewrote `cognitive_complexity` in `auditor-ast` with detekt-inspired AST-precise algorithm
- Removed token-based heuristic fallback (`calculate_cognitive_complexity`, `token_cognitive_complexity_findings`)
- Added lambda/closure nesting increment
- Added boolean short-circuit operator scoring (`&&`/`||`) per condition
- Added labeled break/continue/return detection
- Added recursion detection via self-call matching
- Fixed `else`/`else if` double-counting by skipping anonymous keyword tokens

## 0.6.6 — Remaining Detector Merges + Structural Improvements

### Detector Merges
- `java_assert_in_production` → merged into `assert_in_production` (Item 1)
- `kt_empty_when` → merged into `empty_conditional_branch` (Item 2)
- `kt_unsafe_cast` → renamed to shared `unsafe_type_cast` (Item 3)
- `go_ineffassign` → renamed to shared `unused_assignment` (Item 4)
- `java_array_stored_directly` confirmed fully removed (Item 5)

### Structural Improvements
- `FileCache` adopted by all token-level detectors — single-pass file reading (Item 6)
- Language partitioning: Go/Java/Kotlin detectors receive pre-filtered slices (Item 7)
- Structured dispatch: `analyze_repository` organized into 5 documented phases (Item 8)
- `tooling_overlap`, `historical_phase_validator`, `repeated_repo_graph_build` gated behind `graph-analysis` feature (Item 9)

## 0.6.5 — Detector Consolidation (18 → 5 Shared IDs)

### Merged Detectors
- `go_os_exit`/`java_exit_in_production`/`kt_exit_in_production` → `exit_in_production`
- `go_unhandled_signal`/`java_unhandled_exception`/`kt_unhandled_exception` → `unhandled_termination_in_production`
- `java_sqli`/`go_sqli`/`kt_sqli` → `sql_injection`
- `go_exec`/`java_exec`/`kt_exec` → `shell_execution`
- `go_weak_crypto`/`java_weak_crypto`/`kt_weak_crypto` → `insecure_cryptography`
- `console_output_in_production` extended to catch Java `System.out.print`
- `empty_catch_block` extended to catch Java empty catch blocks

### Cleanup
- Removed 7 unused functions, 14 tests, 18 registry entries
- 90 → 75 detectors

## 0.5.0 — Initial Workspace Release

### Infrastructure
- Rust workspace with 6 crates: auditor-cli, auditor-core, auditor-detectors, auditor-donors, auditor-report, auditor-mcp
- Go/Java/Kotlin language frontends
- AST-powered analysis via tree-sitter
- JSON audit reports with typed evidence and baseline diff
- Donor policy framework with 13 registered donors

### Detectors (75 total)
- 28 gating + 47 full profile detectors across L1-L5 layers
- Correctness, security, structural, lexical, algorithmic, systems-economics, and optimization categories
- 5 shared cross-language correctness detectors
- Go-specific: log.Fatal, errcheck, cyclomatic complexity, repeated string literals
- Java-specific: StringBuffer detection
- Kotlin-specific: non-null assertion, require/check misuse
- Naming convention analysis
- Bloat/slop analysis
- Systems economics detectors

## 0.2.0 — Wired & Widened

[Content unchanged from original]

## 0.1.0 — Initial Release

[Content unchanged from original]

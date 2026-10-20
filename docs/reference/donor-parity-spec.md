# Donor Capability and Live Parity Contract

## Purpose

Donor parity means that Engineering Auditor and a locked donor execute against
the same generated source case and agree on a documented overlapping
capability. An EA-only scan compared with hand-written expected JSON is an
integration test, not donor parity.

The live harness is feature-gated in
`crates/auditor-detectors/src/donor_parity_tests.rs` and
`crates/auditor-detectors/src/oracle_runner_tests.rs`. It creates atomic temporary
projects, executes the donor, parses native JSON diagnostics, runs EA, and
requires one-to-one agreement at detector, relative path, and line. Selected
donor output and EA output must both be nonempty. Recall and precision must
both equal `1.0`; missing tools, missing checkouts, snapshot drift, invalid
output, and empty donor output are failures.

The 21 executable parity groups are marked as provisioned integration tests.
Ordinary all-feature CI compiles them but does not execute them because donor
checkouts and runtimes are intentionally untracked. Release qualification
invokes all 21 explicitly; their bodies contain no environment-sensitive early
return or soft-skip path.

Line agreement is exact whenever both tools emit a line. A path-only match is
permitted only when one side's typed finding has no line; the current sole case
is EA's file-level `god_module` finding.

There is no checked-in donor expected-output corpus or parity-fixture schema.
Detector unit tests and repository integration tests own EA behavior that does
not execute a donor.

## Current Status

| Donor | Status | Live entry point or reason |
|---|---|---|
| agent-slop-lint | live, pre-provisioned Python required | Locked checkout imported through `EA_AGENT_SLOP_PYTHON`; git snapshot is verified |
| Rust Clippy | live, installed tool required | Generated dependency-free Cargo project; compiler JSON diagnostics |
| Bandit | live, pre-provisioned Python required | Locked checkout imported through `EA_BANDIT_PYTHON`; git snapshot is verified |
| Ruff | live, versioned binary required | Ruff 0.15.20 native JSON; executable version and git snapshot are verified |
| ESLint | live, versioned binary required | ESLint 10.6.0 native JSON through `EA_ESLINT_BIN`; executable version and git snapshot are verified |
| PMD | live, repo-local Java 21 required | PMD 7.27.0-SNAPSHOT native JSON; executable commit/version and git snapshot are verified |
| Detekt | live, pre-provisioned jar required | Locked checkout imported through `EA_DETEKT_JAR`; native SARIF and git snapshot are verified |
| Pylint | live, versioned binary required | Pylint 4.1.0-dev0 JSON2; executable version and git snapshot are verified |
| JSCPD | live, versioned binary required | JSCPD 4.2.5 native JSON through `EA_JSCPD_BIN`; executable version and git snapshot are verified |
| Deptrac | not parity | PHP is outside EA's declared language catalog; local resolved-topology tests and architecture donors cover supported arms |
| Semgrep | not parity | EA has no general custom-rule engine; ad hoc rules would restate local lexical tests |
| GolangCI-Lint | live, pre-provisioned binary required | Locked checkout imported through `EA_GOLANGCI_BIN`; native JSON and git snapshot are verified |
| Import Linter | live, versioned binary required | Import Linter 2.13 forbidden-layer output; executable version and git snapshot are verified |
| Dependency Cruiser | live, versioned binary required | Dependency Cruiser 18.1.0 cycle JSON; executable version and git snapshot are verified |

The typed table in `oracle_runner_tests.rs` is exhaustive over
`auditor-donors.lock.json`. No mapped donor is unavailable. Semgrep names the
local capability tests that replace a misleading custom-rule parity claim.

## Mapped Live Capabilities

- Bandit: B101, B103, B108, B110, B301, B311, B501, B507, and B602. Mappings are
  limited to the selected native rule in each atomic case; unrelated Bandit
  output is not relabeled as EA parity.
- agent-slop-lint: all 24 Tool-A-mapped families. Twenty-one have exact live
  parity: local imports, section comments, magic literals, Halstead volume and
  difficulty, verbosity, tersity, god module, cognitive/cyclomatic/NPath,
  WMC/CBO/DIT/NOC, hidden mutators, escape-hatch density,
  duplication, dependency cycles, package distance, and hotspots. Stutter and
  sibling redundancy are verified precision extensions. String sentinels are
  also a verified precision extension: EA requires the named string parameter
  to participate in conditional or match logic with at least two distinct
  string values, avoiding donor findings on passive boundary-conversion
  parameters.
- Clippy: `clippy::too_many_arguments`, `clippy::todo`, `clippy::panic`,
  `clippy::regex_creation_in_loops`, `clippy::manual_memcpy`,
  `clippy::or_fun_call`, `clippy::needless_collect`, and compiler
  `unreachable_code`. General Rust
  assertions are deliberately excluded because Clippy has no equivalent
  general assertion rule.
- GolangCI-Lint: native `errcheck`, `ineffassign`, `goconst`, and `prealloc`
  findings map to `go_errcheck`, `unused_assignment`,
  `go_repeated_string_literal`, and `known_size_preallocation`.
- Detekt: `detekt.complexity.CyclomaticComplexMethod` maps to
  `function_complexity`; `detekt.performance.ForEachOnRange` maps to
  `range_for_each_iteration`; `detekt.performance.SpreadOperator` maps to the
  typed-spread arm of `intermediate_collection_materialization`; full-analysis
  `detekt.potential-bugs.UnreachableCode` maps to `unreachable_code`.
  Atomic unreachable parity uses one unreachable statement because EA
  intentionally coalesces a contiguous segment while Detekt reports each
  expression. `CouldBeSequence` is not claimed because EA has no equivalent
  sequence-pipeline capability.
- ESLint: native `no-duplicate-imports` maps exactly to `duplicate_import` on
  the shared JavaScript capability case.
- PMD: native `ClassNamingConventions` maps exactly to `naming_convention` on
  the shared Java capability case.
- JSCPD: exact minimum-token function-body clones map to
  `source_duplicate_detection`. EA additionally recognizes consistent
  identifier/literal renaming; that precision extension is outside the exact
  shared live case.
- Ruff: C901 maps exactly to `function_complexity` at a shared complexity
  threshold and declaration line; PERF402 maps to
  `bulk_collection_extension` on the shared typed identity-copy case.
- Pylint: `too-many-nested-blocks` maps exactly to
  `excessive_nesting_depth`; both report the outer control block and retain the
  maximum depth as evidence. `consider-using-generator` maps to the typed
  identity-comprehension arm of `intermediate_collection_materialization`.
- Import Linter: a forbidden Python layer contract maps exactly to
  `architectural_layering` over the same resolved import.
- Dependency Cruiser: `no-circular` maps to `architectural_layering` over the
  same two-file JavaScript cycle.

The v0.8.9 promotion gate re-executed every mapped donor. ESLint duplicate
imports, PMD parameter reassignment, Pylint nesting, and Detekt/Clippy
unreachable-code cases remain exact after their EA owners moved to gating.

## Repository-Scale Interpretation

Atomic parity proves a selected overlapping capability. It does not imply that
two tools have identical discovery scopes, thresholds, or candidate sets on an
arbitrary repository. Repository qualification must therefore classify EA
findings directly and record any discovered noise mechanism as a semantic
negative capability case.

## Execution

The status contract is safe for ordinary hosted CI:

```sh
cargo test -p auditor-detectors --features donor-oracle \
  donor_capability_statuses_cover_the_manifest_without_claiming_unavailable_parity
```

The live gate is intentionally explicit and has no soft skips:

```sh
EA_BANDIT_PYTHON=/path/to/bandit/bin/python \
EA_AGENT_SLOP_PYTHON=/path/to/agent-slop-lint/bin/python \
EA_GOLANGCI_BIN=/path/to/golangci-lint \
EA_DETEKT_JAR=/path/to/detekt-cli-all.jar \
EA_ESLINT_BIN=/path/to/eslint-10.6.0 \
EA_JSCPD_BIN=/path/to/jscpd-4.2.5 \
EA_RUFF_BIN=/path/to/ruff-0.15.20 \
EA_PYLINT_BIN=/path/to/pylint-4.1.0-dev0 \
EA_PMD_JAVA=/path/to/java-21 \
EA_PMD_CLASSPATH='/path/to/pmd/lib/*' \
EA_IMPORT_LINTER_BIN=/path/to/lint-imports-2.13 \
EA_DEPCRUISE_BIN=/path/to/dependency-cruiser-18.1.0 \
cargo test --locked -p auditor-detectors \
  --features donor-oracle,graph-analysis \
  -- --ignored --test-threads=1
```

The GitHub Actions live job uses a pre-provisioned self-hosted runner and is
manual. Checkout cleanup is disabled only for that job so provisioned ignored
donor checkouts survive; the public jobs still use a clean checkout. The job
does not install donor packages or fetch donor dependencies. A donor must not
move from `unavailable` to `live` until its locked checkout has executed
successfully and the harness parses its native output.

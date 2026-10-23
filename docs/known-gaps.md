# Known Gaps

What Engineering Auditor does not do, or does only partly, as of 1.5.0.

## Analysis Depth

- Analysis is syntax-based. There is no type checking, whole-program
  dataflow, alias analysis, or general control-flow graph, so detectors that
  would need them stay silent rather than guess. Your language's own linters
  (Clippy, Ruff, ESLint, golangci-lint, and others) use type information and
  catch defects Engineering Auditor cannot.
- Algorithmic, systems, and optimization detectors follow calls into
  repository functions only when the call resolves to one certain target,
  through at most four calls. Calls on values whose type is unknown, such as
  methods of an injected service (`this.userService.find(..)`), are never
  followed, so the work such a call does stays hidden.
- Data flow is followed within a function and through certain calls only,
  in all 13 parsed grammars. It does not model aliasing: a C or C++ local
  whose address is taken, a C# `ref`/`out` parameter, or a Bash variable
  set by `read` or `export` from another function is followed by its name
  alone. Bash functions have positional parameters only, so a value from
  `$1` is reported as unbound rather than as a parameter.
  `sql_injection` reads a query held in a local through the values bound to
  it, and reports a name or a clause fragment spliced into a query when it
  comes from request input in the same function (a framework request member
  or accessor, or an annotated request parameter); a fragment that reaches
  the query through a field, a helper, or a caller is treated as
  configuration, and a query a helper returns is not followed. `random_for_security` follows a weak random value to where
  it goes (names, keys, cookie, HMAC, cipher, and credential-header calls)
  and from a helper that returns it to its callers, through up to four
  resolved calls; a secret neither named nor used as one, or reached through
  a call on an untyped receiver, is missed.
- A file with syntax errors is analyzed after recovery. For C, C++, and C#,
  one configuration of the preprocessor conditionals is chosen (the first
  live branch of each `#if` group), and in C and C++ a macro in a
  declaration header (`WINAPI`, `GTEST_API_`, zlib's `local`) is blanked
  when that lets the declaration parse; detectors that read the recovered
  tree see only that configuration. Any declaration still in error is
  quarantined (blanked and parsed again), and its findings are missed. A
  file is skipped entirely when quarantine would cover half of it or errors
  remain. The report marks such files `recovered` or `unparsed`. On the
  46-repository corpus, 442 files are recovered and 148 of 17,185 are still
  skipped, mostly C++ test files built from framework macros
  (`TEST_CASE("..") { .. }`, `MATCHER`, `MOCK_METHOD`), and syntax that the
  grammars do not read yet (Java record patterns, TypeScript variance
  annotations and `export type *`, Kotlin annotated function types).
- Reflection, plugin loading, and entry points defined outside the scanned
  code are not resolved, so reachability findings such as `dead_code` and
  `orphan_symbol` exclude what they cannot prove.
- Support differs by language.
  [`reference/language-support.md`](reference/language-support.md) is the
  authority for each detector and language. Of the 42 algorithmic, systems,
  and optimization cells, C# supports 31, C++ 24, and C 10; the rest are
  unsupported (each with its reason) or not applicable, and the supported
  ones have no corpus precision record yet. The 1.4.0 systems and logging
  detectors are Tier 1 only. Ruby, Julia, and Shell get
  structural metrics only. NPath complexity is unavailable for Kotlin, Ruby,
  Julia, and Shell, and Halstead metrics for Kotlin and Shell.

## Inputs

- `churn_complexity_hotspot` reads Git history; in a shallow clone it has
  none and reports nothing.
- The `graph-analysis` detectors read an externally produced repository
  graph and co-edit journal (`--repo-graph`, `--co-edit-journal`). Without
  them they report `missing_input` and no findings.

## Baselines

- The baseline carries a finding across a renamed symbol or a moved file only
  when the match is unique and the finding's message is unchanged. A rename
  that also changes the finding's numbers (for example, a complexity score) is
  reported as one resolved and one new finding.

## Performance

- Parsing and the structure and complexity indexes use all cores, but
  detectors run one at a time, so a scan's later stages use one core. A
  repository of a few hundred thousand lines takes about 8 seconds on a
  16-core machine.

## AI Review (Experimental)

- The 16-run lifecycle qualification matrix passed once, on an OpenCode free
  route that has since closed to EA; no route available now passes it.
  Mistral Medium 3.5 completes reviews but answers one probe inconsistently (about a third of
  runs answer `unknown` although the evidence is visible). Four candidate
  rewordings of that probe were tested and rejected. OpenCode's free
  tier refuses clients other than OpenCode, and other free routes are rate
  limited below one evaluation matrix.
  [`reference/layer6-evaluation.md`](reference/layer6-evaluation.md#qualification-record)
  keeps the dated record.
- Mode E (three blind reviewers plus comparison), the shared-root and
  consolidation case templates, and the comparator are implemented but stay
  in shadow until their evidence and manual approval are recorded.
- Provider calls are synchronous. A timeout bounds each request, and
  cancelling a review stops later stages, but cannot interrupt a request
  already in flight.
- Cost ceilings are exact only for routes that declare a per-request cost or
  are free. A route with unknown cost needs explicit consent and is bounded
  by call and token counts, not money.
- Live qualification needs credentials and network access; it cannot run in
  an offline CI job.

## Donor Parity Tests

- The 21 donor-parity test groups compare results with locked versions of
  external tools. They are ignored by default and need those tools and source
  checkouts provisioned (see
  [`reference/donor-parity-spec.md`](reference/donor-parity-spec.md)).

## Intentional Non-Capabilities

- The AI review produces assessments and recommendations, never findings. It
  cannot suppress, relabel, or change deterministic results, and scans and
  gates never need it.
- The review has no open-ended tools, arbitrary file access, open
  conversations, hidden fallback routes, or self-modifying prompts. Its only
  tools are EA's typed assessment, bounded symbol lookup, and `noop`.
- Static expected-output snapshots do not count as detector capability or
  donor parity. Grammar fixtures exist only where a stable parser input is
  needed.

## Downstream

- A downstream project pins 0.9.19 through a sibling checkout. Moving it to a
  `>=1.0,<2.0` version requirement is that project's change.

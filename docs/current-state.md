# Current State

What Engineering Auditor does today, by area, as of 1.3.0. Limitations are
in [`known-gaps.md`](known-gaps.md).

## Release

1.0.0 (2026-10-19) is the first stable release. It freezes the surfaces in
[`stability.md`](reference/stability.md): CLI commands and exit codes, report
v5, finding-ID v3, baseline v2, gate result 2, SARIF 2.1.0, and the stable
`auditor.toml` keys. Pushing a version tag builds Linux x86_64 and macOS
archives with checksums, the license, and third-party license notices
([`distribution.md`](distribution.md)). 1.1.0 is the first release from the
public repository; it adds parallel parsing and the license notices and keeps
every stable surface unchanged. 1.2.0 analyzes files with syntax errors after
recovery, adds the data-flow layer below, and adds optional syntax-coverage
fields to the report. 1.3.0 extends the algorithmic, systems, and
optimization detectors to C#, C++, and C.

## Scanning

- **Catalog.** 116 detectors by default (52 gating, 64 full-only); the
  `graph-analysis` build feature adds seven. Each detector's languages and
  roles are declared in the catalog and enforced on every finding.
- **Discovery.** Repository ignore files and `.eaignore` bound the scan;
  every file gets a source role. `--path` and changed-range inputs limit
  discovery itself, so a scan of two changed files reads two files.
- **Analysis method.** `--method auto` (the default) uses syntax trees where
  a grammar exists and falls back to token rules; `ast` and `token` force
  one.
- **Syntax errors.** A file that does not parse cleanly is analyzed after
  recovery: declarations still in error are blanked and the file parsed
  again, and for C, C++, and C# one preprocessor configuration is chosen.
  The report marks each such file `recovered` or `unparsed`.
- **Data flow.** Within a function, every parsed grammar answers the same
  def-use questions (which bindings reach a read, where a value comes from
  and goes). `sql_injection`, `random_for_security`, `resource_not_closed`,
  `unused_assignment`, and the structure index's assignment facts use it.
- **Determinism.** The same inputs give byte-identical reports, including
  with parallel parsing.
- **Speed.** Parsing and the structure and complexity indexes use all cores;
  detectors run one at a time. On a 16-core machine, gating scans take about
  0.4 s for zlib (45,000 lines), 2 s for ripgrep (53,000), and 8 s for
  commons-lang (208,000) and eslint (542,000), using up to about 540 MB.

## Finding Identity And Baselines

- Finding IDs are anchored to the enclosing declaration's qualified name, so
  edits that only shift lines keep every ID. A self-scan with prepended
  lines, an inserted function, and a reordered pair of functions reports 0
  new and 0 resolved findings.
- The baseline follows a finding across a renamed symbol, a moved file, or a
  shifted occurrence when the match is unique and the message is unchanged;
  such pairs are reported as carried, not new.
- `baseline rekey` rebinds carried entries and can drop resolved ones;
  `baseline migrate` moves a baseline across finding-ID versions.
- SARIF results carry the ID in `partialFingerprints`.

## Reports And The Repository Map

- Report v5 records every detector's execution status (executed, missing
  input, unsupported, not applicable), coverage, input digests, the baseline
  comparison, and a complete evidence graph.
- A full JSON scan with `--output` streams a repository map beside the report
  (hashes, symbols, spans, metrics, imports, bounded calls; no source text).
  An unchanged repeat scan reuses identical report and map content, and the
  scanner never reads its own outputs.
- `--format human` and `auditor digest` render the same human report from a
  scan or a saved JSON report.

## Precision

Gating detectors are measured on a 40-repository corpus at pinned release
commits, with a precision bar of a 95% lower bound of at least 85%. 18
detectors have cleared the bar; most others have too few corpus findings to
decide it and gate only while no live false positive is known. Detectors with
residual false positives run only in `full`.
[`detector-precision.md`](reference/detector-precision.md) has the
per-detector record.

Recall is measured separately on seeded cases
([`eval/recall/`](../eval/recall/README.md)). For six algorithmic and systems
detectors in five languages, every case with the problem directly in a loop
is caught (29 of 29). All six follow calls into repository functions,
through up to four calls that resolve to one certain target, and also catch
every case with the problem in a same-file helper or an imported helper (29
of 29 each).

## AI Review (Experimental)

- **Implemented.** The full staged review described in
  [`architecture.md`](architecture.md#ai-review-experimental): case planning,
  bounded evidence packets, deterministic probes, single-reviewer review,
  independent challenge and response, host-owned finalization, offline
  verification, report-last publication, dispositions, and transcripts.
  Mode E (three blind reviewers and a structural comparison), the shared-root
  and consolidation templates, and portfolio ordering are implemented and in
  shadow.
- **Providers.** OpenAI Responses, Anthropic Messages, Mistral, keyed and
  anonymous OpenCode, and OpenAI-compatible Chat Completions, all through
  provider-native tool calls. Mistral requests carry a prompt-cache key.
- **Quality.** The 16-run lifecycle matrix passed once, on a free route that
  has since closed to Engineering Auditor; no available route passes it now.
  [`layer6-evaluation.md`](reference/layer6-evaluation.md#qualification-record)
  keeps the record. Prompt and template wording changes only through the
  manual offline-improvement process described there.
- **Safety.** Nothing is sent without `--allow-hosted-review` (and
  `--allow-anonymous-provider` for anonymous routes). Requests are bounded in
  count, tokens, time, and cost, and records keep hashes and counts, never
  credentials or hidden reasoning.

## CLI And MCP

The CLI and `auditor-mcp` call the same scan and review services. MCP exposes
scan start and status, and review start, status, result, and cancel, with one
review active at a time. Cancelling stops later review stages.

## Reference Tools

Twelve external tools have strict parity adapters: Bandit, agent-slop-lint,
Clippy, Ruff, ESLint, PMD, golangci-lint, Pylint, JSCPD, Detekt, Import Linter,
and Dependency Cruiser. Their 21 test groups pass at the versions pinned in
`auditor-donors.lock.json` when the tools are provisioned; they are ignored in
ordinary test runs.

## Repository Health

The repository scans itself in CI and fails on any finding not in its
baseline. The full test suite (647 tests, plus the 21 ignored parity groups)
passes on stable Rust and on the minimum supported Rust, 1.86.

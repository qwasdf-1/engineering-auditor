# Architecture

How Engineering Auditor is put together, for contributors. Code, schemas, and
tests are the authority when this page disagrees with them.

## Pipeline

```text
repository
  -> source discovery and file roles          (auditor-detectors)
  -> parse once, build shared indexes         (auditor-ast, auditor-detectors)
  -> detector owners, in catalog order        (auditor-detectors)
  -> language gate and finding validation     (auditor-detectors)
  -> report v5, evidence graph, map sidecar   (auditor-report, auditor-scan)
  -> optional AI review                       (auditor-review)
```

The AI review reads the finished report and repository map. Nothing in the
deterministic layers depends on it.

## Crates

| Crate | Owns |
|---|---|
| `auditor-core` | Finding, severity, confidence, and location types; finding IDs; digests; `parallel_map`. |
| `auditor-ast` | Tree-sitter frontends for 13 languages, the per-scan `RepoAst` tree cache, and compact semantic facts. |
| `auditor-detectors` | Source discovery, the detector catalog, every detector, and the shared indexes. |
| `auditor-report` | The report v5 contract: construction, JSON and SARIF output, parsing, evidence graph, baselines. |
| `auditor-scan` | The one scan service used by both the CLI and MCP: runs detectors, assembles and publishes reports atomically. |
| `auditor-review` | The experimental AI review: repository map, providers, cases, verification, evaluation. |
| `auditor-donors` | The reference-tool ("donor") manifest checks. |
| `auditor-cli` | The `auditor` binary. |
| `auditor-mcp` | The `auditor-mcp` binary: JSON-RPC tools over the scan and review services. |

## Source Discovery

`source_discovery.rs` walks the repository honoring checked-in `.gitignore`
and `.ignore` files and the repository's own `.eaignore` files (gitignore
syntax). Machine-global Git excludes are ignored so results do not depend on
the machine. Hidden source files are still scanned.

Every file gets a role: production, tooling, test, fixture, benchmark,
example, generated, documentation, or reference. Roles come from path
conventions (`tests/`, `benches/`, `scripts/`, `tooling/`, `*_test.go`, Go's
leading-underscore directories, a Rust `#[cfg(test)] mod name;` file, and
others). Each detector declares which roles it may report on and, separately,
which roles it may read for context, so a dead-code check can see a test's use
of a function without reporting findings in tests. `auditor.toml`
`[exclusions.<name>]` adds path exclusions for named detectors; none are built
in.

## Detector Catalog And Dispatch

`registry.rs` holds the catalog: one `DetectorSpec` per detector with its ID,
category, severity, profile, layer, roles, and owner. There is no second list;
`auditor catalog`, the report's detector metadata, and the generated language
page all project from it. Each spec names one `DetectorOwner`, and
`dispatcher.rs` routes the owner to its code. After an owner runs, the engine
checks every finding's detector ID and category against the spec, and
`language_gate.rs` drops findings in languages or roles the spec does not
claim. [`detector-architecture.md`](reference/detector-architecture.md) has
the rules a new detector must follow.

## Shared State And Indexes

One scan owns all repository state; nothing is cached across scans or held in
globals.

- `source_cache.rs` (`FileCache`) keeps one shared copy of each file's text
  and its line offsets.
- `RepoAst` caches one syntax tree per file over that same text. Function
  spans and semantic facts are computed lazily once per tree.
- Five indexes are built lazily from those trees and dropped after their last
  user:

| Index | Built from | Used by |
|---|---|---|
| `StructureIndex` | Semantic facts: symbols, imports, calls, assignments, references, literals, control regions, loops, per-function counters | L2 structural detectors; source of the other indexes |
| `ComplexityIndex` | Function spans | Cyclomatic, cognitive, NPath, Halstead, and churn detectors |
| `AlgorithmicIndex` | `StructureIndex` | L3: loops, collections, mutation, composed cost terms |
| `SystemsIndex` | `StructureIndex` | L4: I/O, network, parse, cache, async, and request-path effects; local import topology |
| `OptimizationIndex` | `StructureIndex` and `AlgorithmicIndex` | L5: invariance, purity, known sizes, result use |

Parsing and the per-file work of the structure and complexity indexes run on
all cores through `auditor_core::parallel_map`, which returns results in input
order, so reports stay byte-identical. Detector owners themselves run one at a
time, in catalog order. `EA_TRACE_OWNERS=1` prints each owner's time.

Analysis is syntax-based: there is no type checker, whole-program dataflow, or
alias analysis. Detectors that would need them stay silent rather than guess.

Within one function, `auditor_ast::FunctionFlow` answers def-use questions
the same way for every language: which bindings (parameters, declarations,
assignments, loop variables) reach a read, where a value comes from, and
where it goes (names, call arguments, properties, the return), and which
expressions a name's value is made of (whole values and parts appended
with `+=`). It reads the
tree on demand, follows anonymous functions inside their enclosing function,
and takes a `CallFlow` policy for how calls pass values through. It has no
flow through fields or aliases. Between functions, owners carry what a
function returns through the call graph's certain targets.
`dataflow_sources.rs` names request input (framework request members and
accessors, request-bound parameters) and security sinks for the owners that
use the layer (`sql_injection`, `random_for_security`).

Graph-dependent detectors (the `graph-analysis` feature) read optional
repository-graph and co-edit inputs through a `GraphProvider`; without them
they report `missing_input` and emit nothing.

## Reports

`auditor-report` owns the report contract. A report v5 contains the findings,
each detector's metadata and execution status, coverage, the digests of
external inputs, the baseline comparison, and an evidence graph v2 whose
typed nodes (file, symbol, fact, measurement, finding, detector, input) and
edges must all resolve. Analysis inputs contribute to
`analysis_contract_digest`; the baseline does not.

Finding IDs (v3) hash the detector, the path, and an anchor: the qualified
name of the innermost enclosing declaration, or a normalized line digest
outside declarations, plus an occurrence ordinal. Line shifts therefore do not
change IDs. A baseline v2 stores accepted IDs with their reasons, titles, and
messages; comparison carries an entry across a renamed symbol or moved file
only on a unique match with the same message.

A full JSON scan written with `--output` also streams a repository map v1 to
a digest-named sibling file: paths, content hashes, roles, symbols, spans,
metrics, imports, and bounded calls, never source text. The AI review needs
it.

[`report-contract.md`](reference/report-contract.md) is the full contract.

## AI Review (Experimental)

`auditor-review::case_review` runs the review in fixed stages. Each stage is
deterministic host code except where a model is called, and model output is
admitted only through typed tool calls that the host validates.

1. **Input.** Validate the report, map, and live source hashes, and the case
   templates, prompts, routes, and brief, into one immutable input.
2. **Agenda and cases.** Select connected findings and instantiate engineering
   cases from versioned templates.
3. **Evidence packets.** Compile a bounded, digest-addressed packet of evidence
   per case. Source lookups are limited to mapped symbol neighborhoods.
4. **Probes.** Answer each case's required questions, deterministically where
   possible; a model can never override a deterministic answer.
5. **Deliberation.** One reviewer (Mode S), or three blind reviewers plus a
   structural comparison (Mode E, currently in shadow), produces claims and
   recommendations.
6. **Challenge and response.** An independently routed challenger objects;
   the reviewer answers each objection once.
7. **Finalize, verify, publish.** The host decides final status, the verifier
   replays everything offline, and the reviewed report is published last so a
   failure leaves the previous report in place.

Providers (OpenAI Responses, Anthropic Messages, Mistral, OpenCode, and
OpenAI-compatible Chat Completions) are configured by routes that name the
model, the credential's environment variable, privacy and cost classes, and
hard limits. Every request and response is recorded without secrets.
[`layer6-llm-review.md`](reference/layer6-llm-review.md),
[`layer6-providers.md`](reference/layer6-providers.md), and
[`layer6-evaluation.md`](reference/layer6-evaluation.md) describe the
protocol, the provider boundary, and how quality is measured.

## Tests

- **Capability tests** build a temporary repository for every detector with
  one positive and one negative case, then check every language the detector
  claims and silence in every language it does not.
- **Identity tests** shift lines, insert functions, and reorder code, and
  require unchanged finding IDs.
- **Catalog tests** check every default detector-language mode (1,710 today)
  and both role sets against the generated documentation.
- **Integration tests** run real scans through the CLI and MCP, including
  scripted-provider reviews, cancellation, and gate exit codes.
- **Donor parity tests** run a locked external tool and Engineering Auditor on
  the same generated project. They are ignored by default and need the tools
  provisioned ([`donor-parity-spec.md`](reference/donor-parity-spec.md)).
- **Self-gate.** CI scans this repository and fails on any finding not in
  `config/auditor/gating-baseline-v1.json`.

Donor source checkouts live under the ignored `references/` directory and are
never runtime dependencies.

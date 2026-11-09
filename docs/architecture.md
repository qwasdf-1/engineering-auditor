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
| `auditor-review` | The experimental AI review, being rebuilt: repository map, providers, cases, evidence, claim and recommendation records. |
| `auditor-donors` | The reference-tool ("donor") manifest checks. |
| `auditor-cli` | The `auditor` binary. |
| `auditor-mcp` | The `auditor-mcp` binary: JSON-RPC tools over the scan service. |

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

Repository-local type facts (`type_facts.rs`) stand in for type information
where a repository states it. Each structure symbol carries what its
declaration says (`structure_typing.rs`: Java, C#, Go, Kotlin, and
TypeScript): the declared type or a method's return type, the initializer,
whether it binds once (`final`, `const`, `readonly`, `val`), and whether a
method has a body. `TypeFacts` resolves a member chain read inside a
function (`sessionId`, `table.getObjectId()`, `Constants.Aliases.X`) through
the function's locals and parameters, its class's fields and first base,
type names for static members and nested classes, and return types; a name
several classes declare resolves only when every candidate agrees. It has no
generics, no inference beyond declared return types, and no library types
apart from a few members that return a number (`length()`, `size()`,
`Count`); TypeScript class fields and Go struct members are not symbols, so
they stay unknown. `sql_injection` reads values declared numbers, enums, or
literal constants as unable to carry SQL; `hot_path_linear_membership`
bounds a local read from a constant collection no code can grow; and
`n_plus_one_query_in_loop` drops a query-named call whose receiver's class
implements it, and every repository function that implementation calls,
without a read query.

Within one function, `auditor_ast::FunctionFlow` answers def-use questions
the same way for every language (all 13 grammars, with a conformance test
that runs one program shape in each): which bindings (parameters, declarations,
assignments, loop variables) reach a read, where a value comes from, and
where it goes (names, call arguments, properties, the return), and which
expressions a name's value is made of (whole values and parts appended
with `+=`). It reads the
tree on demand, follows anonymous functions inside their enclosing function,
and takes a `CallFlow` policy for how calls pass values through. It has no
flow through fields or aliases. Between functions, owners carry what a
function returns through the call graph's certain targets.

The structure index also owns the repository facts that owners and the map
share. `type_hierarchy.rs` resolves each type's first written base once: a
type of the same file, then through the import that binds the name or its
qualifier (a repository type, or an external one by canonical name such as
`multiprocessing.Process`), then, in Java, Kotlin, and C#, the only type of
that name in the package directory or the repository. Class metrics (DIT,
NOC), type facts, and the topology's inheritance edges read it. `call_graph.rs`
splits a dotted callee into receiver and name (`self.load`, `pkg.Func`,
`Type::new`) and resolves `self`/`this`, implicit-`this`, and `super` calls
through the hierarchy, `Type.method` through a type's ancestors, and
`module.function` through imports (in Go, any file of the imported package);
dispatch is static. `entry_points.rs` records where control enters the
repository: mains, CLI commands, HTTP route handlers, signal handlers, and
the `run` of a process subclass, its bases followed across files.
`exit_in_production` and the repository map read it.
`dataflow_sources.rs` names request input (framework request members and
accessors, request-bound parameters) and security sinks for the owners that
use the layer (`sql_injection`, `random_for_security`). `bound_reads.rs`
gives the resource owners (`resource_not_closed` in every language) the
reads a handle's binding reaches, and `unused_assignment` takes a
function's declared locals from the layer's bindings, keeping only its own
alias rules (C# by-reference parameters; C and C++ `volatile`,
address-taken, and reference-bound locals). The algorithmic, systems, and
optimization families read assignments from the structure index's semantic
facts, which take their stores (which node kinds assign, and each one's
target and value) and the loop variables their fields do not name from the
layer's `dataflow_shapes.rs`, so both agree on what is assigned where.

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

A full JSON scan written with `--output` also streams a repository map v2 to
a digest-named sibling file: paths, content hashes, roles, symbols, spans,
metrics, imports, the call graph's bounded calls, inheritance, and entry
points, never source text. The AI review needs it.

[`report-contract.md`](reference/report-contract.md) is the full contract.

## AI Review (Experimental)

The AI review is being rebuilt (v1.6 plan): the v2 case protocol, with its
probes, ensemble, challenge and response stages, finalization, and offline
verification, was removed. `auditor-review` keeps the parts the new reviewer
builds on:

1. **Providers and routes.** Adapters, consent, transport, retry, and
   transcripts, with a single-reviewer route chain and review-wide and
   per-question budgets.
2. **Repository map and history.** The map sidecar, checked against the
   report and the live source, and the optional history digest.
3. **Claims and recommendations.** Typed records that cite evidence IDs the
   host issued, with validators that reject unknown citations.

The new reviewer is in `reviewer/`, run by `review run
--experimental-reviewer` until a route qualifies on it. `RepositoryView` joins the report's findings with the map and
divides the repository into modules (directories, split while one holds more
than a third of the files). From it, the orientation pack renders shape,
module structure and cycles, hotspots, finding distribution, and coverage
within 24 KiB, and the retrieval tools (`read_source`, `symbol_info`,
`search_symbols`, `findings_in`, `module_overview`) answer tool calls within
a review's call and byte budget, of which one question may take a quarter
of the calls. Every row cites an evidence ID (`finding:`, `file:`,
`symbol:`, `module:`, `edge:`, `entry:`, `source-span:`, and others) that
only a shown row issues, and source reads re-check the file's hash.

Two commands feed the reviewer. `review history` writes a history digest:
per-file commits, growth, and co-change partners over a window of `HEAD`'s
first-parent line, so a review can rank change hotspots and coupling no
import shows after the clone is gone. With a digest of the repository's
`HEAD`, the pack adds those rankings, and a sixth tool, `history`, answers
for one file (`history:` and `co-change:` IDs). `review check-patch` judges
a recommendation's patch in `auditor-scan`: it applies the diff to a scratch
copy, re-scans it against a scan of an unpatched copy, and runs the brief's
test command in a cleared environment. The verdict is verified only when the
cited findings are gone, none is added, and the tests pass.

Questions come from checked-in templates
(`config/auditor/review-question-templates-v1.json`: wording, limits, claim
slots, recommendation families) and triggers over the same view: the top
production files of each hotspot ranking, symbols whose findings come from
two or more detectors, module cycles and structure findings, and always,
last, a remediation plan that depends on the other questions. A question
renders its subjects with evidence IDs, and `Question::check_answer` holds
an answer to it: the claim and recommendation validators within the
question's slots, families, and scope, then the kind's shape (every subject
cited, rankings complete, a shared cause explaining two or more findings,
every plan step resolving one).

The driver asks the questions in order, one conversation each, over the
route's provider chain. A conversation opens with the pack, the operator's
brief, and the question; the plan also sees the earlier answers, with their
claim IDs, so its steps can rest on them. The reviewer reads with the
retrieval tools (`findings_in` shows each finding's evidence values) and
answers with one `answer_question` call: claims and recommendations named by
refs of its own, which the host turns into content-derived IDs before
`check_answer`. A rejected answer goes back with its reasons, naming the
reviewer's refs, and is held: a revision sends only the claims and
recommendations it replaces or adds, and the refs it drops. A question's
last two requests offer only the answer tool, so a reviewer that reads
until its budget runs out still answers. What the reviewer sees names
hash IDs (files, symbols, findings, cases, claims, recommendations) by
short aliases such as `file:7`, numbered per kind in the order the review
first shows them, so repeated openings stay identical for prompt caching;
the host expands them before any check, and the record keeps the table. With a
checker, `check_patch` runs the patch checker on a proposed diff, and the
host checks every accepted recommendation that carries one. A request that
returns no model output is retried, a target that fails for good gives way
to the next, and a spent question or review budget leaves questions
unanswered with the reason. The review record
(`contracts/engineering-review-v1.schema.json`) holds each answer, every
provider call and retrieval, the checks, and what the review spent.

`reviewer/qualification/` checks recorded reviews offline, from the report,
map, and history alone (`review qualify`). Each run must be bound to those
inputs and to its routes and brief, answer every question, pass its answer
checks again with every citation resolving in the inputs, contradict no
measured value it cites, carry the host's verdict on every accepted patch,
and add up to its recorded spend within every budget. Two runs must agree on
the top hotspots and plan steps. `review rubric` writes the usefulness sheet
a person, or a second model checked by a person, scores
([`layer6-evaluation.md`](reference/layer6-evaluation.md)).

Providers (OpenAI Responses, Anthropic Messages, Mistral, OpenCode, and
OpenAI-compatible Chat Completions) are configured by routes that name the
model, the credential's environment variable, privacy and cost classes, and
hard limits. Every request and response is recorded without secrets.
[`layer6-llm-review.md`](reference/layer6-llm-review.md),
[`layer6-providers.md`](reference/layer6-providers.md), and
[`layer6-evaluation.md`](reference/layer6-evaluation.md) describe the
provider boundary, the removed v2 protocol (kept for reference until the
new path ships), and how a route qualifies on the rebuilt review.

## Tests

- **Capability tests** build a temporary repository for every detector with
  one positive and one negative case, then check every language the detector
  claims and silence in every language it does not.
- **Identity tests** shift lines, insert functions, and reorder code, and
  require unchanged finding IDs.
- **Catalog tests** check every default detector-language mode (1,710 today)
  and both role sets against the generated documentation.
- **Integration tests** run real scans through the CLI and MCP, and gate exit
  codes.
- **Donor parity tests** run a locked external tool and Engineering Auditor on
  the same generated project. They are ignored by default and need the tools
  provisioned ([`donor-parity-spec.md`](reference/donor-parity-spec.md)).
- **Self-gate.** CI scans this repository and fails on any finding not in
  `config/auditor/gating-baseline-v1.json`.

Donor source checkouts live under the ignored `references/` directory and are
never runtime dependencies.

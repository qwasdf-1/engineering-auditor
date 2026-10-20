# Report, Map, Review, Graph, And Baseline Contract

This reference defines the deterministic output boundary consumed by gates,
MCP, and optional Layer 6 review. Machine-readable authority lives in the
schemas under `contracts/`; Rust parsers additionally enforce cross-field and
live-artifact invariants that JSON Schema cannot express.

## Versions And Compatibility

Which of these contracts are stable, experimental, or internal is defined in
`stability.md`.

| Contract | Version | Compatibility rule |
|---|---:|---|
| Audit report | 5 | Earlier reports and incomplete v5 identities are rejected. |
| Semantic analysis | 1 | Changes only for incompatible detector semantics not represented elsewhere in identity. |
| Evidence graph | 2 | Unknown kinds, dangling edges, or incomplete finding support are rejected. |
| Finding identity | 3 | IDs hash detector, static title, primary path (omitted for structural anchors), and the identity map; never a line number. Baselines compare stable IDs only under an identical report identity. |
| Source role | 1 | Emission and context roles are both published. |
| Repository map | 1 | The sidecar must match the report, live source snapshot, and artifact digest. |
| Engineering review | 2 | The case artifact must bind all deterministic and policy inputs by digest and carry current offline verification. |
| Evidence packet | 1 | The case-scoped semantic payload, typed items, indexes, closure, and budgets must match its canonical ID and digest. |
| Probe definition/result | 1 | Case/template identity, compatible evidence classes, resolver authority, citations, and canonical result identity are revalidated. |
| Standalone baseline | 2 | Report-shaped files, duplicate IDs, empty reasons, missing entry identity fields, and incompatible identities are rejected. |
| Review dispositions | 1 | Current decisions are unique per material recommendation and bound to one review. |

Finding identity v3 is frozen. Titles are `&'static str` at the
`Finding::builder` boundary, so the compiler rejects a formatted title. The
golden file `crates/auditor-detectors/testdata/finding-ids-v3.tsv` pins the ID
of every positive capability finding in every supported language, and
`finding_ids_match_the_frozen_v3_golden` fails on any change. An intended ID
change requires a migration path (`baseline migrate`) and follows the stable
rule in `stability.md`. Identity values taken from source text (for example a
repeated call expression) change when that finding's own code is reformatted.
That is within the rule that edits to a finding's own code may re-key it.

There is no compatibility serializer for earlier reports. A baseline moves to
a new finding-ID version with `baseline migrate` (see Baseline And Gates);
otherwise create a new standalone baseline after human review.

## Report V5

The report publishes profile, scope, requested method, repository root,
contract versions, and `analysis_contract_digest`, plus:

- deterministic language and source-role counts and one typed record per file;
- every active detector's metadata and complete detector-language matrix;
- one capability-execution record per active detector, distinguishing
  `executed`, `missing_input`, `unsupported`, and other non-execution states;
- findings, grouped findings, baseline diff, external inputs, analysis
  coverage, and complete evidence graph v2;
- a repository-map reference and an engineering-review envelope.

A file whose syntax tree has errors carries `syntax`: `recovered` when it
parsed after the declarations holding the errors were quarantined (blanked
and parsed again, keeping every offset), so structural owners analyzed the
rest of it; `unparsed` when recovery failed and structural owners skipped
it. Lexical and complexity owners read every parsed file either way.
`analysis_coverage` counts them as `syntax_recovered_files` and
`syntax_unparsed_files`. All three fields are omitted when zero or absent.

Analysis-affecting inputs, including an authoritative repository graph or
co-edit journal, are content-digested into analysis identity. Baseline policy,
repository-map publication, review policy, prompts, provider calls, and review
output do not change `analysis_contract_digest`, finding IDs, findings, or
evidence-graph support. Tests compare those values before and after both
envelopes are attached.

Finding identity v3 keys each finding on a code anchor instead of its line.
The scan records the anchor in the reserved `identity.anchor` key as
`symbol:<qualified path>` (the innermost enclosing declaration, excluding
locals, parameters, fields, and the unnamed file module; Rust methods are
qualified by their `impl` self type, as `<Type as Trait>` for trait impls;
Go methods by their receiver's base type, without pointer or type parameters), `content:<digest>` (the whitespace-normalized
flagged line, when no declaration encloses it or the method is `token`), or
`structural:<key>` (owner-declared and path-independent: clone classes by
normalized token hash, dependency cycles by canonical rotation, layer edges,
packages, and repeated literals). Finding titles are static per rule;
values such as names, zones, and limits live in the message and evidence. Statement-level
findings without a detector subject add `identity.line_content`, and
otherwise-identical findings add a source-ordered `identity.ordinal`.
Detectors may not set the reserved keys, except that an owner may declare a
structural anchor. Detector subjects never contain secret-bearing source:
credential findings key on the assigned name.

SARIF results carry `partialFingerprints` whose `engineeringAuditor/v3` and
`primaryLocationLineHash` values are the finding ID, so SARIF consumers such as
GitHub code scanning track alerts by the line-free identity rather than by
recomputed line-content hashes.

Arrays and object keys use stable ordering. Repeating an unchanged scan with
the same explicit output path produces byte-identical report and map content.
Managed report, map, review, and temporary output artifacts are excluded from
source discovery so a scan cannot ingest its own products.

## Repository Map V1

A full-profile, full-scope, AST-capable JSON scan with explicit `--output`
streams a digest-addressed sibling map. The report reference records its
relative path, digest, source-snapshot digest, completeness, and file, symbol,
and edge counts. The map contains:

- normalized repo-relative paths, language, role, byte count, and source hash;
- compact symbol identity, kind, visibility, parent, span, and metrics;
- containment, resolved-import, and bounded call edges;
- explicit unresolved import/call counts and complete map accounting.

It contains no source bodies. Loading rejects path traversal, a sidecar outside
the report directory, digest/count mismatch, duplicate IDs or paths, invalid
parents/endpoints/kinds, spans beyond file bytes, source symlink escape, stale
source hashes, and aggregate snapshot mismatch.

Authorized source lookup accepts mapped symbol IDs only. It is limited to two
graph hops and fixed request, excerpt, symbol, per-excerpt, and total-byte
ceilings that a review brief may tighten but never expand. Every read rechecks
path containment and the mapped content hash; stale or invalid UTF-8 source
fails closed.

## Engineering Review V2

The reviewed report is a distinct atomic output. Its envelope records schema
version 2, complete status, review/input identity, the digest-addressed sibling
artifact, and case, accepted-recommendation, blocked-case, contested-case, and
challenge-response counts. A sibling review-plan sidecar retains the exact
input paths and deterministic plan used by offline verification.

The artifact binds the agenda, cases, evidence packet history, probes, reviewer
runs, claims, recommendations, single-reviewer and comparison-scoped
challenges, ensemble comparisons, finding dispositions, production portfolio,
execution truth, qualification/provider/egress records, semantic revisions,
projection audits, and current offline-verification record. Each ensemble
challenge binds its reviewer review ID, challenger run ID, and exact comparison
target set. V1
artifacts have no compatibility parser and return an explicit unsupported
version error.

`review verify --report <reviewed-report> --repo-root <root>` derives the v2
artifact and bound plan from the report, reloads the current inputs, and
validates them without contacting a provider. A report with an absent review
remains valid: Layer 6 is supplemental policy output, not deterministic
analysis.

## Engineering Review V2 Input Foundation

The v2 path constructs an immutable `ReviewInput` before planning or
provider qualification. Its `InputBinding` binds canonical SHA-256 identities
for report v5, evidence graph v2, repository map v1, the live source snapshot,
case-template bundle, prompt bundle, route configuration, an optional operator
brief, and a sorted set of typed evidence attachments.

Content digests use `sha256:<64 lowercase hex>`. Host-owned IDs hash their
typed prefix, schema version, and canonical identity fields. Object order and
attachment set order cannot change identity; semantic array order remains
significant. Paths, timestamps, call duration, provider usage, and
presentation-only text are excluded from semantic identity.

Template, prompt, route, and evidence inputs enter through typed validators;
the verifier has no raw arbitrary-JSON or provider/network interface. It
rejects unsupported versions, binding mismatches, duplicate attachment
identity, missing adapters or mandatory inputs, unsafe paths, and stale source
snapshots. Review publication changes only the supplemental engineering-review
envelope; deterministic report identity and findings remain immutable.

The case pipeline uses the following supplemental contracts without changing report v5:
`review-candidate-v1`, `review-agenda-v1`, `case-template-v1`,
`case-template-bundle-v1`, `engineering-case-v1`, and `evidence-packet-v1`.
Agenda records cite existing finding and evidence-graph IDs. Engineering cases
retain immutable finding references and selection provenance, bind the verified
input and checked-in template bundle, and carry no replacement finding
severity, confidence, or validity judgment.

The case-template bundle is a mandatory typed `ReviewInput` contract. Runtime
case construction requires its verified digest to match the input binding and
its semantic content to equal the checked-in bundle. Template edits therefore
change the input and case identities; runtime prompts cannot add or replace a
template.

## Engineering Review V2

Report v5 supports two mutually exclusive engineering-review envelope shapes.
Schema 1 retains assessment, material-recommendation, contested, and revision
counts. Schema 2 instead requires case, accepted-recommendation, blocked-case,
contested-case, and challenge-response counts; v1 count fields are not emitted
for a v2 envelope.

The digest-addressed `engineering-review-v2` artifact contains the complete
case lifecycle, including evidence packets, probe definitions/results,
reviewer runs with one host-derived typed case conclusion, optional claims and typed
recommendations, challenge/response outcomes,
non-mutating finding dispositions, execution truth, qualification/provider
records, and egress manifests. The artifact ID excludes only its derived
verification record, avoiding recursive identity while binding every semantic
and process input.

Reviewer artifacts also contain claim-slot dispositions,
recommendation-family dispositions, challenge-target dispositions, task
completion records, typed semantic revision records, and model-projection audit
records. A projection audit binds renderer version, source digest, rendered
Markdown digest and bytes, and sorted excluded field paths. A revision record
binds task identity, monotonically increasing revision, before/after semantic
digests, non-empty reason, request ID, and provider tool-call ID. Full
superseded arguments remain transcript material rather than duplicate semantic
artifact records.

Offline verification reloads no provider and accepts no transport. It
recomputes the artifact identity and process counts, re-admits case reviews,
challenges, responses, and host finalizations, and proves that every report
finding is unchanged and represented by exactly one disposition. Atomic
publication writes the artifact and Markdown sidecar before replacing the
reviewed report; rollback preserves pre-existing outputs.

All maintained human-readable report surfaces use the shared typed Markdown
document engine. `scan --format human` renders a freshly scanned report;
`auditor digest --report <report.json>` parses and renders the same complete
document from persisted machine output; and v2 publication uses the same field,
section, escaping, list, code-block, and integrity primitives for its review
sidecar. The JSON report and review artifact remain authoritative. Persisted
Markdown is deterministic and intended for both operator inspection and the
same evidence-reading workflow used by the model-facing projections.

An evidence packet binds one engineering case and input binding. Its canonical
semantic payload contains sorted typed evidence items, required, optional,
blocking, counterevidence, and unavailable indexes, the authorized
finding/graph/map neighborhood, monotonic budgets, closure, and every omission.
The packet ID and content digest are recomputed from that payload, and
validation rejects stale item digests, duplicate or mistyped IDs, dangling
indexes, altered case templates, and packets beyond the hard byte ceiling.
Runtime hard limits are code policy rather than schema compatibility constants.

The v2 case lookup contract is narrower than the production-v1
`source_requests` compatibility path described below. A lookup names its case,
probe, typed request kind, target ID, and decision relevance; only selected
finding IDs and repository-map symbols anchored inside the precomputed two-hop
case neighborhood are accepted. Raw paths, grep/search expressions, duplicate
requests, and ambient filesystem authority are absent from the API.

`probe-definition-v1` and `probe-result-v1` remain supplemental v2 contracts.
Definitions bind their case and template, retain mandatory or optional origin,
fix result vocabulary and resolver precedence, and carry explicit evidence and
decision boundaries. Results bind one definition, host-recorded resolver
identity/version, packet-resident evidence IDs, explanation, limitations, and
model reviewer-run identity only when model interpretation supplied the
answer. Unknown fallback is an explicit successful resolution record.

`review-routes-v2`, `review-prompt-bundle-v2`, and
`review-qualification-v2` are also supplemental v2 contracts. They define the
case-based provider registry and target chains, content-bound prompts, and
role/case/probe-specific qualification records without changing report v5.
The v2 qualification contract records native protocol and semantic validity;
v1 strict-output fields do not authorize a v2 route.

## Provenance And Evidence Graph V2

Every finding declares language, source role, requested analysis method,
extractor version, native origin, and span precision. A support node is exact
only when detector evidence contains a valid positive ordered
`line:column-line:column` span; columns are never inferred.

Graph node kinds are `file`, `symbol`, `fact`, `measurement`, `finding`,
`detector`, and `external_input`. Edge kinds are `contains`, `references`,
`derived_from`, `supports`, and `produced_by`. Validation requires unique IDs,
known kinds, valid endpoints, and support plus producer coverage for every
finding. Support nodes retain evidence keys and a digest, never source text.

## Engine-Resolved Evidence Refs (v0.9.17)

The Layer 6 review engine resolves three kinds of evidence references from the
provider's `source_requests` payload. The model emits `source_request.symbol_id`
for all three; the engine dispatches by prefix:

| Prefix | Engine resolution | Id format on `SourceExcerpt` |
|---|---|---|
| `symbol:` | `RepositoryMap.symbol_neighborhood` — AST-derived symbol graph walk, bounded by `max_hops`. | `symbol:<MapSymbol.id>` |
| `EA-<hex16>` | Finding-id resolution — looks up the `FindingBrief` by id, reads a bounded source window at `path:line`. | `finding-excerpt:<path>:<start>-<end>` |
| `audit-artifact:` | Documentation-only provenance prefix for review artifacts; not a `source_request` input. | N/A (egress only) |

`SourceExcerpt` carries a `kind: SourceExcerptKind` enum (`symbol` / `finding`)
to make the id's namespace explicit. The `SourceRequest.symbol_id` wire field
is unchanged; only in-engine construction uses the typed id.

Both `symbol:` and finding-id paths enforce the same budget ceiling
(`max_excerpts`, `max_excerpt_bytes`, `max_total_bytes`) and digest-parity
check (`digest_file` against the `MapFile.sha256`). Stale or missing source
files fail closed with `StaleSourceSnapshot` or `Contract`.

## Baseline And Gates

A standalone baseline (v2) stores the full comparable identity plus sorted
entries containing finding ID, detector, static title, path, message, the
finding's identity map, and a nonempty human review reason. It does not copy
evidence or source.

```sh
cargo run -p auditor-cli -- scan --repo-root . --profile gating \
  --format json --output report.json
cargo run -p auditor-cli -- baseline create --report report.json \
  --reason "Source-reviewed legitimate maintained debt." > baseline.json
```

Baseline comparison first matches exact finding IDs, then pairs the
remainder in three passes. A pass pairs a key only when exactly one accepted
entry and one current finding share it:

| Pass | Same | May differ |
|---|---|---|
| `ordinal` | detector, title, path, identity except ordinal | occurrence ordinal |
| `renamed` | detector, title, path, identity except anchor/ordinal, normalized message | anchoring symbol name, edited flagged line |
| `moved` | detector, title, identity except ordinal, normalized message | path |

Normalized messages replace the anchoring symbol's name and `line N`
positions; metrics and other values must still agree. Structural anchors are
path-independent, so they only match exactly. Ambiguous candidates never
match, so a fixed finding plus an unrelated new finding in the same file
stays resolved plus new. The report baseline envelope (schema 2) and the gate
result (schema 2) list carried pairs as `{baseline_id, current_id, pass}`.

`gate --baseline` rejects any new finding (carried findings are not new) or
incompatible identity; `gate --max-findings` enforces an absolute budget.
Exit code 0 passes, 1 is a structured policy failure, and 2 is invalid input
or execution failure. Review gating separately requires valid dispositions
for every material recommendation; it never requires accepting a
recommendation.

`baseline rekey --baseline <b> --report <r> [--prune-resolved]` rebinds exact
and carried entries to their current findings, keeping review reasons; it
tolerates a changed analysis contract digest but never accepts new findings.
`baseline migrate --baseline <old> --from-report <previous> --report <current>`
moves a baseline across finding-ID versions: both reports must scan the same
source tree, findings correspond by detector, location, and message, and
entries without a unique counterpart are listed for review instead of being
carried.

The maintained deterministic policy is
`config/auditor/gating-baseline-v1.json`. A baseline may record legitimate
debt, but cannot justify a detector defect, false-positive mechanism, missing
capability test, donor divergence, or pass-owned bloat/slop finding.

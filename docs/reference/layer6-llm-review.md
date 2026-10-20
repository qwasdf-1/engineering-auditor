# Layer 6 - Evidence-Constrained Engineering Review

Profile: optional post-scan workflow

Detectors: 0

Layer 6 is the only probabilistic layer. It consumes immutable deterministic
evidence from Layers 1-5 and produces structured engineering assessments and
recommendations. A normal scan, baseline, and gate remain complete without it.

## What It Is

Layer 6 is an evidence-constrained reviewer for questions that require
cross-layer judgment: interacting risks, architectural tradeoffs, operational
consequences, coordinated changes, public-contract impact, and the relative
value of alternative remediations. It is related to LLM-as-a-judge work because
it uses structured rubrics, independent criticism, repeated evaluation, and
calibrated abstention. It is not a scalar judge of repository quality.

The stable epistemic categories are `evidence`, `assessment`, `inference`,
`recommendation`, and `evidence_gap`. Every assessment cites deterministic IDs
or authorized excerpts, and insufficient evidence is a valid result.

## What It Is Not

- It is not a detector and cannot create, suppress, relabel, or mutate an L1-L5
  finding.
- It does not assign a global quality score or decide whether code is good.
- It does not replace type, dataflow, graph, cost, or effect analysis that can
  be deterministic.
- It does not inspect arbitrary repository paths or accept arbitrary provider
  tools; it uses only EA-declared provider-native tools.
- It does not treat repository prose as executable instruction or proven
  implementation truth.
- It does not require an operator to accept model advice.

## Inputs And Eligibility

A review requires a valid report-v5 JSON file with a complete repository-map-v1
reference, a matching live source snapshot, the checked-in case templates,
v2 provider routes, applicable consent, and the v2 prompt bundle. A review
brief and typed evidence attachments are optional inputs. `scan --review`
forces a full-profile, full-scope, JSON, AST-capable scan. `review run`
consumes an existing eligible report and writes a distinct sibling reviewed
report.

The brief supplies prioritized goals, must/prefer constraints, risk posture,
optional allowlisted documents with declared intent, and optional tighter
context/lookup budgets. Briefs cannot widen hard runtime ceilings.

## Deterministic Review Agenda

The v2 path validates every input before building a provider-free review
agenda. Five versioned selectors identify exact-subject concentration,
cross-layer lifecycle opportunities, same-subject recommendation overlap,
cross-layer intersections, and eligible missing or unavailable capabilities.
Selectors cite immutable finding and evidence-graph IDs; they cannot emit
claims, confidence, root causes, or recommendations.

Candidate identities are stable under report, graph-node, and selector-return
ordering. Normalization merges exact decision/case/subject matches, retains
every typed source-candidate reference, and splits subjects that lack a direct
graph relationship. Merely sharing a file does not connect sibling symbols.

The agenda retains planned and suppressed candidates. Suppression uses typed
reasons with fixed semantic precedence, so budget deferral cannot hide a
stronger exclusion. Template-aware materiality, privacy, completion, priority,
and optional case-count budgets enter through a deterministic host policy;
until the template layer lands, the foundation policy makes no semantic
suppression and invents no default budget. Invalid selector output or a
dangling reference rejects the agenda transactionally.

## Versioned Engineering Cases

The v2 case registry accepts only the digest-bound checked-in
`review-case-templates-v1.json` bundle. The bundle has explicit ownership for
all recognized case kinds: `lifecycle_mismatch` is production-enabled, while
`shared_root_cause` and `recommendation_consolidation` are implemented and
produce shadow products until their production-escalation gates pass.

Lifecycle candidates instantiate into deterministic `EngineeringCase`
records. A case binds the template version, bundle-bound input identity,
normalized candidate, connected subjects, immutable finding references, and
normalized operator constraints. Candidate evidence remains selection
provenance; it is not promoted to claim support.

The lifecycle template fixes eight mandatory probe slots covering repetition,
input equivalence, mutation, material cost, consumer requirements, wider
ownership, invalidation, and retention risk. Relevant mutation, distinct
configuration, test-only execution, freshness requirements, and retention
risk remain probe boundaries and inference guards rather than premature
conclusions. Optional probes are limited to named, case-bounded
counterexample, alternative-feasibility, and verification-availability
questions.

Probe admission distinguishes capability from observed behavior. A function or
type signature that permits mutation, including an opaque callable receiving a
mutable reference, does not prove that mutation occurs or that values differ.
Mutation, per-execution consumer state, and retention-risk answers require a
visible implementation, an explicit behavioral contract, or runtime evidence;
otherwise the model must preserve the fact as unknown.

Material-cost polarity is also enforced at admission rather than left solely
to prompt compliance. A definitive `yes` or `no` must cite a runtime
measurement or an operator-bound constraint. Structural detector output alone
can explain the cost shape, but without a bound it admits only `unknown`.

## Case Evidence Packets

Before deliberation, the v2 path compiles one
`evidence-packet-v1` record per engineering case. The packet contains only the
selected findings, their bounded evidence-graph neighborhood, mapped symbols
anchored through verified finding spans, applicable capability records,
operator constraints, typed attachments, and authorized source excerpts. It
does not project unrelated findings or the complete report.

Evidence items have compiler-owned typed IDs (`finding:`, `fact:`,
`measurement:`, `source-span:`, `symbol:`, `edge:`, `capability:`,
`brief-constraint:`, or `test:`), trust, provenance, content digest, and
canonical content. Required, optional, blocking, counterevidence, and explicit
unavailability indexes make closure auditable. Packet identity and digest cover
the normalized semantic payload, so input collection order does not affect
identity.

Packet admission prioritizes required evidence, blocking counterevidence,
optional evidence, then presentation detail. The default target is 64 KiB and
the hard ceiling is 128 KiB; source expansion is limited to two hops and 16
total excerpts. Brief and future route policy may only tighten those values.
Closure is exactly `closed`, `partially_closed`, `blocked`, or `over_budget`;
unresolved counterevidence is never treated as proof of absence.

`EvidenceLookup` requires the bound case ID, one of its probe IDs, a typed
request kind, an in-neighborhood finding or symbol ID, and a
decision-relevance reason. It wraps the existing hash-validating source reader
and rejects raw paths/searches, unknown or duplicate requests, stale source,
excess hops, and exhausted budgets. Spec 06 owns probe-driven lookup
scheduling; source items returned by lookup can already be recompiled into the
same immutable packet contract.

Planning also uses that source owner to admit complete selected-finding source
items into the initial packet, up to `initial_source_excerpts`. The primary
decision source therefore does not depend on whether a provider successfully
uses its optional evidence-request turn. Complete items are admitted or fail
explicitly; they are never returned as prefixes. Lookup authority advertises
only source targets not already satisfied by a complete packet item. If a
provider nevertheless repeats an already-satisfied read, the evidence round is
idempotent: the host records and services the turn without requiring a
fabricated packet-digest change. A genuinely new read still must produce a new
packet digest before final output can be admitted. Evidence union is monotonic:
recompilation seeds the union with every complete source item already admitted
to the current packet, then adds new lookup results. A later read cannot erase
eager or earlier source evidence.

## Probe Planning And Resolution

`probe-definition-v1` expands every enabled case into its template-mandatory
probe set plus at most the template's bounded optional count. Mandatory probes
reuse the case-derived IDs and cannot be renamed, replaced, or waived.
Optional probes must use an allowed kind, cite only in-packet targets, name a
claim slot or recommendation family they could change, and record why that
decision needs the probe.

Each definition fixes the five allowed answers (`yes`, `no`, `unknown`,
`not_applicable`, and `conflicting_evidence`), compatible evidence classes,
positive/negative boundaries, blocked claim slots, and resolver precedence.
Template validation rejects probe-kind/evidence-class combinations the runtime
cannot interpret safely. The model projection omits the duplicated per-probe
class list: each evidence item already carries its classes, while the probe's
exact `admissible_evidence_ids` owns citation authority. Those IDs are
alternative sources, not a conjunctive checklist. Complete source can decide
lifecycle, consumer, mutation, invalidation, and retention boundaries when
those properties are visible in the addressed code.
Deterministic cross-field output rules are encoded in the strict tool schema
where practical. In particular, structural, qualitative, and unknown expected
benefits omit numeric, unit, and extrapolation fields; they cannot reach host
semantic admission as numeric claims without measurement provenance.

The provider projection describes each permitted recommendation family with a
typed `use_when` rule. Lifecycle reownership includes moving invariant work
from a repeated operation into an enclosing function activation;
explicit-retention-avoidance is reserved for intentionally keeping work local
when retention or hoisting is unsafe or infeasible; measurement-before-change
defers action pending named measurement. Bare enum names are not expected to
carry these semantics.

Resolvers run by authority: existing deterministic fact, evidence graph,
repository map, bound measurement, operator brief, bounded model
interpretation, then host-owned unknown fallback. Resolver registration order
does not affect this sequence. A definitive higher-authority answer stops
resolution; resolvers at the same authority must surface disagreement as
`conflicting_evidence`. The host injects resolver identity/version and
revalidates every evidence citation against the packet before producing
`probe-result-v1`.

The built-in resolvers deliberately abstain where structure is not semantics.
A mapped symbol does not prove safe lifecycle ownership, and a raw measurement
without an operator-relative threshold does not prove material impact. Every
mandatory lifecycle probe still receives one typed result, so `unknown`
records complete execution truth without manufacturing a provider request.
Specs 08–09 own execution of model-eligible interpretation; this completed
runtime already validates those results.

## Deliberation Routing And Case State

The v2 router selects one of `deterministic`, `single_reviewer`, or `ensemble`
from case policy, validated mandatory probe results, and typed routing facts.
It never reads provider/model metadata or interprets free-form prose. Ensemble
predicates cite packet evidence, and the combined medium
consequence/uncertainty/scope condition uses named categories with packet
citations rather than a hidden score.

Mode D requires a deterministic not-substantiated, evidence-unavailable, or
fully-determined disposition, no model-derived probe result, and no remaining
higher-order inference. Mode S is limited to a narrow local, reversible
judgment with at most one credible action family. Any configured ensemble
predicate, material probe conflict, multiple credible action families, or the
combined-medium condition selects Mode E.

Availability is evaluated only after selection. Mode E requires all three
independently qualified slots. Missing or incomplete slots close locally as
`execution_failed` with `mode_unavailable` and the more specific
`blocked_by_mode_unavailable` reason; the router never downgrades to Mode S or
clones another slot. Unknown operator intent likewise closes only that case
with `operator_input_required`.

Each case has an append-only execution ledger from `planned` through `closed`.
Transitions are sequence-numbered, conditional skips are explicit and typed,
failure short-circuits record all bypassed stages, and no transition is
accepted after closure.

The case provider projection is typed data rendered through one deterministic
Markdown renderer shared by qualification, review, reconsideration, challenge,
response, repair, and lookup results. It carries the decision kind and
subjects, immutable finding references, probe contracts,
claim/recommendation boundaries, operator constraints, trust-labelled evidence
content, deterministic probe authority, closure, omissions, and stable task
references. Allowed-conclusion labels and polarity-free inference guards are
host policy and do not cross the model boundary. It omits host-only
run/packet/binding IDs, schema labels, byte counters, per-item digests,
extractor and resolver metadata, duplicated identity fields, evidence indexes
that restate the evidence body, and duplicated reviewer/challenge records.
Finding projections retain detector, category, title, message,
recommendation, severity, confidence, locations, typed evidence, and source
role.

Host-authored model-visible messages contain deterministic Markdown or prose,
never serialized JSON. This rule covers initial context, correction messages,
and every EA tool result. Provider wire encoding, provider-native tool
arguments, JSON Schema tool definitions, internal artifacts and transcripts,
and repository evidence whose source content is JSON are not model-authored
JSON text and remain allowed. Repository JSON evidence is preserved verbatim
in a labelled fenced block.

Every projection records a non-egressed audit record containing renderer
version, source digest, rendered Markdown digest and byte count, and the sorted
field paths excluded from the model projection. Input accounting uses the
actual rendered or provider-wire bytes, not the larger internal source value.

## Single-Reviewer Claims And Recommendations

Mode S is a native-tool task ledger rather than one monolithic result. The
initial reviewer turn exposes `answer_case_probe`, `record_case_claim`,
`record_case_recommendation`, and `complete_case_review`.
`request_case_evidence` is present only when the
projected lookup authority contains at least one unresolved target.

Recommendation creation is one complete native
`record_case_recommendation` call. A rejected create does not reserve its local
reference and may be resubmitted correctly. A later intentional change uses
`revise_case_recommendation` with the current revision, a non-empty reason, and
the complete replacement record. There are no incremental recommendation
draft/component tools. Model-facing task schemas encode absence by omission,
not an explicit JSON `null`: the host reconstructs optional persistent fields
before semantic admission. In particular, a recommendation with alternatives
omits `no_alternative_reason`, completion without a preferred recommendation
omits `preferred_recommendation_ref`, and a response without a replacement
omits `replacement_recommendation_ref`.

`expected_benefits` is a provider-native exact tagged union. Direct
measurement requires a numeric value and unit; bounded extrapolation also
requires an extrapolation model; structural, qualitative, and unknown benefits
cannot carry those fields. Reviewer and response create/revision calls validate
all benefit items before ledger admission and return every mode error together
with an indexed Markdown field path.

Semantic records retain a deterministic creation-order index in addition to
keyed lookup. Claim admission uses that order so an “earlier local claim” means
an actually earlier accepted create call, independent of lexical local IDs.
Revisions retain the record's original position, withdrawals filter without
renumbering, and legacy ledgers without the index fall back to deterministic
key order.

Completion is accepted only after every unresolved probe ID has one answer.
The host derives the typed case conclusion from all deterministic and
model-resolved probe answers with an explicit lifecycle precedence policy. A
probe-answer revision recomputes the conclusion; no conclusion create or
revision tool exists. Supporting claim slots and recommendation families are
optional; the host records deterministic omission dispositions for those not
asserted. Free-form claims cannot own case polarity. Completion contains only
the preferred recommendation reference and unresolved gaps. The host derives
case position and semantic closure, binds reviewer-run, case, template,
finding, evidence-packet, and route metadata, and derives all persistent
identities.

Lifecycle conclusion precedence is semantic rather than incidental enum or
branch order. Non-repetition is `impact_immaterial`. Relevant intervening
mutation, non-equivalent inputs, or a consumer that needs a per-execution
result is `repetition_justified`, even when a generic ownership or retention
constraint also exists. Only otherwise-invariant work blocked by wider-owner,
invalidation, or retention constraints is `wider_ownership_infeasible`.
Equivalent repeated inputs with no relevant mutation or consumer distinction,
a suitable owner, feasible invalidation, and acceptable retention derive
`lifecycle_mismatch_substantiated`; unresolved critical facts derive
`insufficient_evidence`.

`engineering-claim-v1` records one typed claim slot, a proposition
classification (`root_cause`, `actionability`, `recommendation_premise`, or
`expected_benefit`), status, supporting and counter-probes, assumptions, and
confidence. Kind is not a second dependency topology. Admission rejects stale
identities, unsupported slots, dangling probes or dependencies, cycles, and
duplicate local references. Normal record tools are create-only: any repeated
local reference is rejected even when its arguments are byte-identical, and
the first admitted record remains authoritative. Blocked, not-substantiated,
and contested states propagate
through dependent claims. A blocked or not-substantiated claim cannot support
an action; a redundant recommendation-premise wrapper is not required.
Root claims require direct probe support. A claim derived from one or more
earlier claims may omit direct probe IDs; its status follows the dependency
fixed point, avoiding duplicated citations without weakening admission.

`contextual-recommendation-v1` binds a template-allowed action family to case
scope, premise claims, affected findings, detector-advice relationship,
preconditions, benefits, risks, costs, alternatives, verification,
materiality, and confidence. Numeric benefit values are canonical decimal
text and require direct measurement or a named bounded extrapolation model.
Lifecycle reownership requires a credible alternative or an explicit
no-alternative reason. It is admissible only when the probe-derived conclusion
is `lifecycle_mismatch_substantiated`; it cannot be used to redesign a
mutation or consumer boundary until a currently invalid hoist becomes safe.
Finalization repeats this compatibility rule as defense in depth. An empty
recommendation list is valid and receives
host-authored family-omission dispositions; the model does not emit decline
calls solely for cardinality.

Following host feedback, a continuation may expose revision tools for records
that already exist. A revision repeats the same local reference, supplies the
expected current revision, gives a non-empty reason, and replaces the complete
record. Patch semantics and implicit numeric/string coercion are forbidden.
A stale expected revision fails without mutation. Explicit withdraw and reopen
operations own transitions between active and declined states. Dependent
references always resolve to the latest admitted active revision; withdrawing
a referenced record makes completion invalid until its dependents are revised
or withdrawn. The audit artifact retains the final semantic record and a typed
revision ledger with task identity, revision, before/after digests, reason,
request ID, and provider tool-call ID. The exact transcript retains full
replacement arguments.

A single-reviewer session permits at most one case-bounded evidence-expansion
round. Admitted records survive a correction continuation. The assistant's
native evidence-request call is retained in conversation history and the
recompiled packet is returned as a native tool result containing complete
Markdown evidence. The final ledger must cover every probe exactly once across
immutable deterministic results and model interpretations; a model cannot
replace deterministic authority or cite outside-packet evidence. After a
rejected duplicate or completion, the model must issue another completion call
to affirm the unchanged ledger or revise it explicitly.

## Challenge, Finalization, And Review V2

Mode E runs three mutually blind Pass A reviewers against the same initial
packet. EA validates and deduplicates their allowed evidence requests, resolves
one provenance-preserving union, and gives every reviewer the byte-identical
expanded packet for one Pass B reconsideration. Structural comparison retains
unanimity, qualified agreement, splits, all-different results, shared unknowns,
evidence conflicts, compatible/scope-variant claims, minority records, and
recommendation-family differences without voting. The resulting exact target
set drives a separate frozen-evidence challenge for each reviewer result.
Incomplete ensembles and challenge failures remain explicit shadow execution
truth and cannot alter the Mode S production portfolio.

Every lifecycle-reownership recommendation enters an independently routed
challenge task ledger. The challenger records only material target objections
and challenge-wide concerns. When `complete_case_challenge` is admitted, the
host deterministically clears every admitted claim and recommendation without
an active objection and publishes a disposition for every target. Objections cover
unsupported assumptions, ignored counterevidence, overclaimed benefits,
unsafe lifecycle changes, omitted alternatives, or another material
objection. Targets must resolve to the admitted reviewer result. Citations must
resolve either to the closed packet or to case probes cited by that admitted
review; this permits a challenger to cite an unknown or conflicting probe
without treating model output as packet evidence. The challenger may use the
same single case-bounded evidence request/recompile round as the reviewer when
unresolved lookup authority exists.

The original reviewer receives one response task ledger and must address every
objection exactly once by conceding, narrowing, maintaining with named
evidence, withdrawing, or marking it unresolved. A replacement recommendation
is recorded separately and the response cites its local reference. Missing
responses remain contested; a missing or blocked mandatory challenge blocks
lifecycle acceptance. The host derives challenge status from target
dispositions and common concerns rather than accepting a model-authored status.

Finalization is deterministic host policy. The probe-derived typed conclusion is
the sole position input: `lifecycle_mismatch_substantiated` maps to
substantiated (or resolved with uncertainty when gaps remain),
`repetition_justified`, `impact_immaterial`, and
`wider_ownership_infeasible` map to not substantiated, and
`insufficient_evidence` maps to evidence blocked. Closure follows exactly from
that position and unresolved gaps. Claim objections update claim
dispositions and rejected, blocked, or contested premises invalidate dependent
recommendations. The host derives accepted variants, the non-mutating finding
disposition ledger, and final case closure; model prose cannot merge or
override those states.

Admission and offline verification reconstruct the complete slot-to-answer
ledger from the bound probe definitions and admitted results, rerun the same
typed conclusion policy, and reject any stored conclusion that differs. Enum-
compatible position or closure values cannot mask a stale or independently
authored conclusion.

Valid native tool calls are authoritative even when an assistant response also
contains prose or provider-visible reasoning; that text is transcript-only.
Prose-only responses and malformed tool arguments receive a bounded readable
Markdown correction and are never interpreted as semantic output. Initial and
evidence continuations allow one evidence round. An accepted-progress
continuation is distinct from correction: it preserves every admitted record,
states that those calls succeeded, requests only unfinished ledger work, uses
a `continuation-N` request suffix, and does not increment repair counts. A host
correction is emitted only after rejected task calls; it names the admission
errors, uses a `host-correction-N` suffix, and increments the semantic repair
count. Three invalid turns or four consecutive incomplete turns with no typed-
ledger progress fail closed; valid turns that add required records may continue
until the finite ledger is complete. When any call makes an assistant task turn
schema-invalid, no call from that turn is recorded; the replay includes one
protocol-native rejection result for every call before the Markdown correction.
If any native argument string is not syntactically valid, EA preserves the exact
response in the immutable transcript but does not replay an upstream-invalid
assistant message. It instead restarts that bounded repair from the complete
original case conversation and supplies readable Markdown naming the rejected
call, parse error, required fields, allowed top-level values, and whole-turn
resubmission rule. The repair request ID records the
`malformed-native-arguments` path.

`engineering-review-v2` is a content-addressed aggregate of the verified input
binding, agenda, cases, packets, probes, reviewer runs, claims,
recommendations, challenge outcomes, finding dispositions, portfolio,
execution truth, qualifications, provider records, and egress manifests.
Empty ensemble comparisons and an empty portfolio are valid. Its verifier
performs no transport calls: it replays admission and finalization and checks
every reference, identity, budget, immutable finding, route, and process count
against the current verified input.

Publication stages the digest-addressed JSON artifact, deterministic v2
Markdown, bound review plan, and reviewed report as siblings in one
transaction. The report is renamed last and rollback restores any prior
outputs. Provider wire responses contain semantic drafts and local references;
all stable semantic and process IDs are host-derived before admission. The
shared v2 service owns production planning, inspection, execution, publication,
and verification; v1 execution and artifact compatibility are removed.

The provider projection separates
`host_resolved_probes` from `model_probe_contracts`, exposes exact per-probe
admissible evidence IDs and item evidence classes, distinguishes decision
targets from lookup authority, and states the host's claim-status,
probe-reference, recommendation-premise, scope, and numeric-benefit rules. It
omits packet identity, raw provenance, and duplicated unavailable omissions;
measured production context fell by more than ten percent from the legacy
projection. Negative probe answers may support claims aligned to their declared
negative boundary. Native tool-schema failures and host semantic admission
each aggregate all errors into one readable Markdown correction and permit at
most three correction turns. Corrections enumerate compatible evidence IDs,
allowed values, and field paths; accepted earlier task calls remain in the
native conversation and intentional changes require the corresponding
optimistic revision tool. Production activation is atomic: the v2 lifecycle
gate must pass before the production owner changes or any v1 surface is
removed.

## Trust And Context

Context blocks retain one of six trust labels:

| Trust label | Meaning |
|---|---|
| `protocol_instruction` | Fixed auditor protocol and output obligations. |
| `operator_intent` | Explicit goals, constraints, risk posture, and document intent. |
| `deterministic_evidence` | Report, coverage, evidence graph, and map facts. |
| `repository_source` | Hash-validated excerpts returned by bounded lookup. |
| `untrusted_repository_documentation` | Allowlisted repository prose, treated as claims only. |
| `untrusted_provider_output` | Model-produced intermediate/output data. |

Each egressed block records kind, provenance, digest, trust, and byte count.
The compiled egress manifest records no credentials. Document paths must be
normalized repo-relative files contained by the repository; traversal and
symlink escape fail closed.

### Context Payload Review Questions

Every new or changed provider projection is reviewed against the following
questions:

1. Does each field help the model make the bounded case decision, cite support,
   recognize a guard, or produce the required output?
2. Is any evidence, review content, instruction, constraint, or identifier
   duplicated elsewhere in the same request?
3. Is host-only metadata such as persistent run IDs, binding IDs, schema labels,
   counters, digests, or resolver internals being sent without a model task that
   uses it?
4. Are trust, provenance, deterministic authority, omissions, and unavailable
   evidence explicit enough to prevent source and inference confusion?
5. Does the payload structure follow the decision flow—decision contract,
   evidence, probe authority, unresolved questions, and output boundary—rather
   than mirror internal storage layout?
6. Are stable references limited to values the response must cite or copy, and
   are their meanings locally discoverable?
7. Is the context sufficient for negative conclusions, abstention, and
   challenge—not merely for producing a positive recommendation?
8. Can repeated prose or metadata be replaced with a typed compact projection
   without weakening admission or offline verification?

Context-size reductions require a regression test that proves decision
completeness, absence of host-only keys, and a material byte reduction against
the prior projection.

### Source Lookup Identifiers

The repository-map outline carries provenance `repository-map-v1:overview`,
which is not a symbol ID, plus a catalog of at most 64 queryable symbols
(`id`, `name`, `file_id`, `file_path`, `kind`), prioritized by files that
findings reference (`context.rs`, `build_queryable_symbols`). A source request
may name a catalog symbol or an `EA-<hex16>` finding ID; a finding ID resolves
to a bounded window at the finding's location, returned as an excerpt with id
`finding-excerpt:<path>:<start>-<end>`. Both paths enforce the same budget
ceilings and digest checks. `audit-artifact:` is a documentation provenance
prefix, not a request input.

## Review Protocol

1. Compile the brief, capability ledger, evidence graph outline, finding
   portfolio, map outline, and allowlisted documentation into case-scoped,
   typed Markdown within byte budgets.
2. Build stable case probe contracts for the selected template. Each probe has
   compatible evidence classes and explicit positive, negative, unknown, and
   conflict boundaries.
3. Qualify every configured model with provider-native tools, a native
   tool-result continuation, and the five bound semantic cases. Both the
   reviewer and challenger roles need at least one qualified target.
4. Answer every model-resolved probe exactly once with native task calls. A
   model may request only case-bound finding or map-symbol targets in one
   evidence round. Successful reads return the complete addressed source or
   fail explicitly; they are never truncated.
5. Record only case-bounded supporting claims and typed recommendations whose
   premises, risks, alternatives, and verification are supported by admitted
   evidence and probes.
6. Run an independently routed challenger. It emits typed objections such as unsupported
   inference, counterevidence, coverage violation, unsafe remediation, citation
   mismatch, or overstated certainty.
7. Treat record calls as create-only. Repeated IDs are rejected, while an
   intentional change uses a full-record revision tool with the current
   revision and a reason. Withdraw and reopen transitions are explicit and
   audited. Addressed objections are named; unresolved objections remain
   attached and set review status to `contested`.
8. Compute deterministic coverage and reliability from evidence coverage,
   evaluator independence, and unresolved objections, then verify the complete
   artifact before publication.

Provider fallback does not silently change this protocol. The artifact records
the proposer, synthesis, evaluator, lookup, and revision topology, including
reduced independence when the evaluator shares a provider or model.
Transcript events that pass provider parsing but fail host admission retain the
complete structured output and are annotated in place with `invalid_output`,
`discarded=true`, and the exact host validation error. This applies to both the
first correction-triggering rejection and a terminal corrected rejection.
Typed tool-boundary decode failures use that same single correction path;
invalid case/probe/target combinations do not bypass correction and collapse
directly into a generic execution failure.

## Verification And Disposition

The verifier rechecks review/report/map/snapshot/brief/prompt bindings,
assessment coverage, evidence and excerpt citations, source budgets, egress,
provider qualification, call budgets, materiality, critic/revision topology,
reliability, and deterministic finding immutability. Verification is offline.

Disposition ledgers are separate operator-owned artifacts. They bind one review
and one current decision per material recommendation: `accepted`, `deferred`,
or `rejected`. Every decision records an operator and timestamp; deferred or
rejected advice requires rationale or counterevidence. Review gates can require
complete disposition coverage, never acceptance.

See `layer6-providers.md`, `layer6-evaluation.md`, `mcp-interface.md`, and
`report-contract.md` for the surrounding contracts.

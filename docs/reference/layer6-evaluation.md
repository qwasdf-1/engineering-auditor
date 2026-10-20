# Layer 6 Evaluation And Prompt Contract

Layer 6 quality is governed by checked-in prompts, a provenance-bearing corpus,
metric-vector hard gates, repeated runs, and a manual offline-improvement
process. Runtime review never modifies its own prompts.

V2 evaluation additionally proves the native conversation contract. Fixtures
cover Chat Completions, Anthropic Messages, and OpenAI Responses request and
response shapes; multiple task calls in one assistant response; completion
after all task classes; assistant-call plus Markdown tool-result continuation;
duplicate create calls; explicit full-record revisions; stale revisions;
withdraw/reopen transitions; and prose accompanying valid calls. A static and
runtime assertion rejects host-authored JSON in model-visible system, user, and
tool-result messages. Repository JSON evidence remains verbatim evidence and
is distinguished from host serialization by its typed source boundary.

Qualification covers positive, negative, unknown, conflicting-evidence, and
forbidden-redesign semantics using the exact native tools production exposes.
It cannot pass through strict structured output or a text fallback. The
offline artifact verifier checks task dispositions, revision records, renderer
projection audits, provider tool-call IDs, recomputes the typed case conclusion
from admitted probe answers, and checks host-derived position/closure/challenge
status without invoking a model.

## Lifecycle Evaluation V2

`config/auditor/review-eval-corpus-v2.json` is the public lifecycle corpus for
the case-oriented review path. Its eight generated repositories cover immutable
equivalent input, intervening mutation, distinct consumer configurations,
test-only repetition, freshness constraints, retention risk, missing evidence, and an
unsupported numeric-benefit claim. Each case binds the public generator and
source-tree digest and records typed expected candidates, probe answers,
case conclusions, closures, optional-claim prohibitions, recommendation
families, gaps, safety constraints, and human review provenance. Each case has
one allowed typed-conclusion set. Runtime derives that conclusion from the
admitted typed probe ledger; the model does not separately reclassify the same
facts. Optional explanatory claims are not recall targets.

The distinct-consumer fixture makes the behavioral boundary observable: each
consumer applies its own replacement before ordering and receives the resulting
snapshot. Hoisting one shared sort would therefore change returned behavior,
even though a syntax-only detector cannot see the mutation hidden behind the
helper call. This is the corpus sentinel for justified repetition rather than a
second immutable-input case.

An explicit source behavioral contract can establish a concrete mutation; a
bare opaque-call invocation cannot. Optional claim uncertainty is retained in
the claim dispositions and closure detail, but it does not replace a settled
host-derived case conclusion or its conclusion-derived closure. A blocked
recommendation premise still blocks that recommendation and therefore the
action path.

For lifecycle probes, a consumer is any use-site that observes the reviewed
result, including an internal loop iteration as well as a downstream caller.
Consumer distinction asks whether one unchanged precomputed result satisfies
all of those observations; it does not ask whether public call signatures are
uniform. Probes are independent rather than mutually exclusive: a destructive
mutation can establish both intervening mutation and a later consumer's need
for reconstructed state. A wider owner is admissible only if it satisfies
explicit retention, destruction, privacy, freshness, and correctness
constraints; lexical enclosure by itself is not an ownership proof.

Source role is affirmative for model judgment only when a selected finding is
classified as test or fixture code. The complete typed role fact remains in the
host evidence packet for closure and auditability, but a production file path
does not prove that a guarded operation executes in production. Coarse
production-role metadata is therefore omitted from the model projection and
operation-level source remains authoritative. The same projection strips
duplicated extractor, origin, span-precision, evidence-key, and source-role
metadata from host-generated evidence-graph nodes while preserving graph
identity, relationships, decision fields, and repository evidence.

`tooling/live_provider/run_lifecycle_v2_eval.py` is the fail-closed gate runner.
It verifies all bound source/configuration digests, rebuilds release binaries by
default, generates a fresh repository, scans it, executes the v2 library
service, verifies the resulting artifact offline, and records binary identities
and typed metrics. The checkpoint identity also binds the evaluator metric
contract version and evaluator source digest, so an evaluator change cannot
silently reuse earlier metrics. Every completed run and the final aggregate
recheck the full live identity, including both executable digests and all bound
inputs, so a concurrent rebuild or configuration edit fails instead of
publishing mixed-identity evidence. The runner also retains subprocess stdout/stderr
received before an outer timeout. `--no-review-timeout` disables only that
outer review-process limit for supervised long runs; provider calls still obey
the route's per-call timeout and bounded retry policy, while build and scan
retain the evaluator timeout. Gate eligibility uses a 16-run risk-weighted
matrix: all eight cases run once, and positive action, mutation-negative
closure, missing-evidence abstention, and unsupported-numeric safety run three
times each.
Schema/binding/citation/finding immutability, negative
closure, typed-conclusion recall, abstention, repeated-run stability, challenge
coverage, lifecycle action completeness, useful novelty, and trace safety are
independent hard dimensions.

Evaluator metric contract v4 names the combined forbidden claim-slot or
recommendation-family count `forbidden_outputs`. Forbidden recommendation
families are also counted by the narrower `unsafe_recommendations` dimension;
neither metric implies that a valid typed case conclusion was forbidden. Every
qualified, executed run independently requires exact admitted probe boundaries
through `probe_answers_valid` and every required accepted recommendation family
through `required_recommendations_valid`. These checks apply to focused
preflights as well as complete matrices. Aggregate `required_outcome_recall`
remains diagnostic and cannot hide a per-run semantic miss.

Required recommendation families are keyed to the host-derived conclusion:
`lifecycle_mismatch_substantiated` requires `lifecycle_reownership`,
`repetition_justified` requires `explicit_retention_avoidance`,
`insufficient_evidence` requires `measurement_before_change`, and
`impact_immaterial` accepts no recommendation. The evaluator scores that exact
pairing per run instead of treating a generally safe recommendation as
interchangeable with the required outcome.

Repeated-run material stability compares the typed case conclusion and final
accepted recommendation families. Optional explanatory claims and differences
between corpus-allowed closure confidence states are not material flips; claim,
closure, and typed-conclusion validity remain independently exact-gated.

Transcript JSON sidecars are the lossless audit source. For provider debugging
or human/agent review, render one without transport-envelope nesting:

```sh
auditor review transcript \
  --input path/to/reviewed-report.transcript.json \
  --output path/to/reviewed-report.transcript.md
```

The adjacent `reviewed-report.transcript.md` preserves the event order and
captured content without truncation. It decodes provider-native messages,
native tool schemas, assistant tool calls and arguments, provider-returned
reasoning fields, adapter fixes, host-parsed output, failures, and retries.
Known request/response wrappers are removed, while their provider-visible
messages and tool contracts are rendered as typed fields. The renderer cannot
reconstruct hidden reasoning that the provider did not return. Omit `--output`
for stdout. Add `--model-input-only` and repeatable `--stage-prefix <prefix>`
filters to audit exactly what a role saw. The JSON sidecar remains
authoritative.

## Causal Attribution Policy

Provider quality is always the last possible attribution. Before using it, an
operator or evaluator must verify, with retained artifacts or controlled
comparisons, every earlier cause:

1. corpus inputs, human expectations, and allowed closures;
2. prompt and context sufficiency, clarity, structure, duplication, and
   irrelevant metadata;
3. native tool schemas, native conversation rendering, adapter normalization,
   and host-admission contract equivalence;
4. route/model discovery, adapter version, and protocol mapping;
5. input, per-request output-token, cumulative request/cost, timeout, and
   multi-pass budgets, including proof that earlier turns do not shrink later
   generation allowances;
6. transport status, HTTP-200 error envelopes, bounded retry, and transient
   availability behavior;
7. native argument parsing, bounded repair, and typed deserialization;
8. host admission, offline verification, publication, and evaluator logic;
9. controlled exact-tool comparisons across supported provider-native
   protocols and adapters; and
10. repeatable live wire evidence across the required run count.

If any item remains unresolved, record the exact EA, configuration, route, or
environment cause, or `unknown`; do not label it provider quality. Availability
and protocol-compatibility failures remain separate from semantic model
quality even when the external endpoint is involved.

## Prompt Bundle And Corpus V2

`config/auditor/review-prompt-bundle-v2.json` contains independently bound
reviewer, reconsideration, challenger, response, and comparison instructions.
Its content-derived identity is recorded by every review.

`config/auditor/review-eval-corpus-v2.json` defines generated lifecycle cases
with expected and forbidden conclusions, closure, recommendation, evidence-gap,
and human-review provenance. Public generators build real repositories under
an operator-chosen destination; generated snapshots are not expected-output
fixtures.

Generators cannot be absolute, traverse parents, or escape the approved corpus
root. Regeneration verifies the script digest, requires a new output directory,
and executes the path directly without shell interpolation. Before/after pairs
share a stable pair ID and state which snapshot is the before or after case.
Human provenance identifies reviewer role, review time, and a rationale digest
rather than private free-form notes.

## Evaluation Gate

An evaluation runner executes each case against a named prompt bundle and
returns per-case conclusion/evidence results. The aggregate metric vector covers
required conclusion recall, forbidden conclusion avoidance, evidence citation
validity, abstention when proof is unavailable, materiality precision, critic
coverage, and reproducibility.

A release gate requires the complete risk-weighted matrix. Thresholds apply to each
metric independently; averaging cannot trade one failed safety/correctness
dimension for another. The report retains every run and its bundle/corpus
identity so variance is visible. Scripted unit runners verify metric and gate
mechanics but are not evidence of live model quality.

Hermetic scripted-provider end-to-end tests
(`crates/auditor-review/tests/case_review_input/`) cover every mode on the
production lifecycle template (single reviewer, challenger, and the shadow
three-slot ensemble with shared reconsideration) and both shadow templates,
`shared_root_cause` and `recommendation_consolidation`, through plan, review,
publication, and offline verification. They prove the mechanics, not live
quality.

The live evaluator writes the identity-bound report atomically before the first
run and checkpoints it after every fully validated run. An interrupted or
failed report retains all completed run records and artifact digests. A later
`--resume-from` invocation rechecks corpus, bindings, binary identities,
artifacts, and recomputed metrics before reuse. A partially written run is
moved to a numbered `.incomplete-N` sibling and retained for diagnosis before
that run index is retried; it is never counted or overwritten.

The current live gate is `tooling/live_provider/run_lifecycle_v2_eval.py`. It
requires explicit egress opt-in, current corpus/template/route/prompt/
adapter/evaluator identities, native role qualification, complete artifacts,
and exact per-case outcome checks. It checkpoints complete runs and can resume
only when every bound identity still matches.

`--route-set` selects the route the matrix qualifies: `anonymous` (the default
OpenCode route, opt-in `AUDITOR_LIVE_ANONYMOUS_EVAL=1`) or a corpus
`keyed_free_routes` entry, a zero-cost route behind an operator key
(`mistral_free`, `openrouter_free`; opt-in `AUDITOR_LIVE_HOSTED_EVAL=1`). Keyed
routes are declared zero-cost with a zero spend budget, and only the
hosted-review consent is passed for them.

## Ensemble And Production Escalation

`contracts/review-escalation-suite-v1.schema.json` and the checked-in suite
define the full read-only escalation decision. The evaluator consumes complete
v2 artifacts and compares production single-reviewer-plus-challenger results
with shadow ensemble-plus-challenger results from the same cases. It measures
pre/post evidence-union disagreement, useful incremental outcomes, failed or
suppressed minority targets, role qualification, and exact per-template probe,
conclusion, closure, and citation matrices.

Promotion is manual and ordered: lifecycle Mode E, shared root,
recommendation consolidation, then the optional comparator. The evaluator
never writes active routes, prompts, or templates. Fewer than the configured
minimum complete qualified runs, any safety regression, or no measured
incremental value produces an explicit `remain_shadow` decision with unmet
predicates. Insufficient-evidence decisions name the missing minimum-run,
minimum-template-case, or alternative-comparison predicate in `failed_gates`;
an empty failure list cannot mask absent evidence. Code completion alone is
never promotion evidence. The current lifecycle evidence satisfies the
read-only eligibility gates for manual Mode E approval, but does not activate
it. Later owners remain independently evidence-gated and ordered.

## Offline Improvement

Prompt and template wording changes only through a manual, evidence-gated
candidate process (Spec 17). The v1 Pareto optimizer was removed with the v2
pipeline; each candidate is now one minimal change to one owned asset,
recorded in a tracked manifest under `eval/review-improvement/<id>/`:

- the owned asset (file, case kind, probe slot, field) and the candidate
  bundle file, both digest-bound; a wording-only change bumps the template
  bundle version, while the template structure version stays a code constant;
- the route file, prompt bundle, and release corpus shared by both arms;
- training cases, drawn from the release corpus, that informed the candidate;
- held-out cases from `tooling/review_eval/generate_lifecycle_heldout_repo.py`,
  with expectations fixed before any candidate run;
- pre-registered decision rules.

`tooling/live_provider/compare_prompt_candidate.py` runs one arm (`baseline`
or `candidate`) on one split with the release evaluator's per-run execution
and metrics. Only runs whose route qualified and whose execution reached
closure are scored; provider, budget, and qualification failures leave the
host's fallback `unknown` answers and are rerun. Held-out cases run only after
the manifest marks the candidate frozen. The reviewer accepts only its
compiled-in template bundle, so the candidate arm uses binaries built from a
clean export of the source with the candidate bundle substituted, under
ignored `.auditor-review-optimization/`, which also holds run output.
Promotion requires operator approval of the held-out expectations and the
results, then a deliberate edit of the tracked bundle and its corpus binding.
Private corpora belong under ignored `eval/private/`.

## Release Evidence

An active-bundle quality claim requires the exact tracked bundle and corpus,
the required qualified live runs, every hard metric passing, held-out results that were
not used to generate candidates, and human approval provenance. Credentials or
network absence is a blocked qualification, not a pass. The environment status
is recorded in the qualification record below rather than replaced by mock
evidence.

## Qualification Record

The dated record of what has and has not qualified, kept so a later run can
be compared with it. As of 1.0.0 no route passes the free-route lifecycle
matrix.

### Ensemble And Portfolio Promotion

- Spec 12 passed its immutable adapter-version-43 16-run anonymous matrix and
  the public CLI/MCP/report/disposition surfaces now accept only review v2. The
  v1 engine, artifacts, schemas, prompts, routes, corpus, and aliases were
  removed.
- Mode E, ensemble comparison, the shared-root and consolidation templates,
  portfolio assembly, and the read-only production-escalation evaluator are
  implemented. Three complete immutable adapter-version-43 ensemble runs made
  Mode E eligible for manual approval with measured incremental value,
  nonincreasing disagreement, and no suppressed minority target. It remains
  shadow until separately approved. Shared root lacks its minimum template-case
  evidence; consolidation lacks its minimum template-case evidence and is
  ordered after shared root; the optional comparator lacks comparison evidence
  and is ordered last. Every unmet predicate is emitted in the decision record,
  and evaluation does not mutate active policy.
- Provider-quality attribution remains the last possible conclusion. Corpus,
  payload, protocol, budgets, transport, parsing/repair, admission/verifier/
  evaluator logic, controlled comparisons, and repeated wire evidence must be
  ruled out first.

### Route Qualification

- Credentialed OpenAI, Anthropic, Mistral, keyed OpenCode, and operator custom
  routes require provisioned environment variables and network access.
- Anonymous OpenCode qualification is exercised by the case-bound v2 lifecycle
  evaluator. Its release-line adapter-version-43 matrix passed all 16 runs. That
  result qualifies only the bound route/corpus/prompt/template/adapter set and
  is not a general provider-quality claim.
- A post-attribution-fix binary-identity refresh is currently blocked before
  semantic output by anonymous-route operations: the bound Mimo route returned
  HTTP 429 `FreeUsageLimitError` on every bounded attempt in the fresh
  identity-8 preflight and retry artifacts (`target/live-eval/
  spec17-adapter43-identity8-preflight.json` and
  `target/live-eval/spec17-adapter43-identity8-mimo-retry.json`), while newly
  discovered Laguna/Ling routes intermittently returned endpoint-unavailable
  responses or transport timeouts. An approved isolated DeepSeek probe also
  returned HTTP 429 `FreeUsageLimitError` for both role qualification and a
  no-repository-content health request; its artifact records
  `model_route_unavailable`, zero reviewer runs, and no semantic output. The
  retained evidence therefore shows an anonymous-route availability gap, not
  provider-quality evidence and not an invalidation of the immutable
  adapter-version-43 semantic artifacts.
- The v1.0 Phase E refresh (2026-10-19) found the anonymous route closed by
  provider policy, not by availability: `mimo-v2.5-free` is no longer listed,
  and role qualification of its successor `mimo-v2.6-flash-free` returned
  HTTP 403 `FreeTierError` ("OpenCode's free tier can only be used from within
  OpenCode") for both the reviewer and the challenger, before any repository
  content was sent (`target/live-eval/phaseE-preflight-2.json` and its
  transcript). Imitating the OpenCode client would circumvent that
  restriction, so EA's anonymous adapter cannot qualify on OpenCode's free
  tier. The free-route matrix needs another zero-cost route (an
  operator-keyed free tier) or an operator decision to qualify on a keyed
  route; qualification is not relaxed to manufacture a pass.
- Keyed free routes (2026-10-19). Mistral's free tier now qualifies at the
  protocol level after three EA-side fixes (strict-mode schema lowering,
  response schema matching admission, actionable premise corrections;
  adapter version 44): both roles pass transport, native-protocol, and
  semantic qualification, and runs reach semantic closure. Semantically it is
  unstable on the immutable-equivalent-input case: Mistral Medium 3.5
  (`mistral-medium-latest` and `magistral-medium-latest` are aliases of
  `mistral-medium-2604`) passed 3 of 6 runs (1 of 3 at the default
  temperature, 2 of 3 in an uncommitted temperature-0 diagnostic), answering
  consumer distinction or intervening mutation `unknown` although the complete
  loop body is visible. The first matrix run stopped
  at that case (`target/live-eval/phaseE-mistral-matrix-1.json`). The probe's
  negative-boundary wording ("failure to establish a concrete mutation is not
  evidence of no mutation") invites the drift, but it is digest-bound template
  text that changes only through the Spec 17 offline-improvement process. The
  OpenRouter free route is capped at about 50 requests a day without credit,
  below one matrix, and its Nemotron Ultra upstream returned HTTP 500.
- Offline improvement `intervening-mutation-negative-boundary-v1`
  (2026-10-19, `eval/review-improvement/intervening-mutation-v1/`) was
  rejected at its pre-registered training gate on Mistral Medium 3.5
  (adapter 45). The candidate answered intervening mutation correctly in 2 of
  2 scored immutable-input runs (baseline 2 of 3) but only 2 of 3
  intervening-mutation runs (baseline 3 of 3): one run reasoned that a
  helper's write after the sort within an iteration does not invalidate that
  iteration's result. Missing evidence stayed `unknown` in every run of both
  arms, and safety counts were zero. No held-out case ran. Two successors
  were also rejected at their training gates: `intervening-mutation-v2`
  (repetition-interval definition) repeated the intervening-mutation
  regression, citing the finding's hard-coded `mutation_count: 0`, and
  `consumer-distinction-v1` could not beat a baseline that answered consumer
  distinction correctly in every adapter-45 immutable-input run. The
  algorithmic evidence now reports `direct_mutation_count` and
  `calls_receiving_collection`. `intervening-mutation-v3`, the v2 wording on
  that corrected evidence, was rejected as well: immutable input improved to
  3 of 3 (baseline 2 of 3), but the intervening-mutation case fell to 1 of 3
  (baseline 2 of 3), because the model stretched the idempotence exemption to
  excuse the helper's write. Three wordings that add an exemption to this
  boundary failed the same way, so the production wording stays, and Medium
  3.5's lifecycle instability blocks a passing free-route matrix. The production
  wording is unchanged, so Medium 3.5's immutable-input instability (about one
  `unknown` in three runs on intervening mutation, plus consumer distinction
  `unknown`) remains. Earlier evidence on adapter 44 (Medium 3.5 partial,
  MiniMax M3.1 Flash, where both arms answered every training case correctly)
  is archived. Mistral Small 4 is unusable on the free Mistral key: its 33,333
  tokens-per-minute limit is below one review run, and EA's bounded retry
  (about 7.5 seconds) cannot wait out a per-minute window.

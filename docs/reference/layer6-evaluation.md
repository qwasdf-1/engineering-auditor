# Layer 6 Evaluation

How a provider route qualifies on the rebuilt engineering review (v1.6
plan, step 6): recorded reviews of real repositories, checked offline by
automatic gates, compared across runs, and scored for usefulness by a
rubric. The v2 protocol's evaluation (the lifecycle corpus as release gate,
the live lifecycle evaluator, the escalation evaluator, and the prompt
candidate runner) was removed with that protocol; its results stay in the
[qualification record](#qualification-record).

## Qualification Corpus

`config/auditor/review-qualification-corpus-v1.json`
(`contracts/review-qualification-corpus-v1.schema.json`) fixes what a route
qualifies on:

- `repositories`: real repositories from the application slice of the
  ecosystem corpus, pinned by commit in `eval/ecosystem/manifest.json`;
- `bindings`: the question templates, prompt bundle, and brief every run
  must use, each by the SHA-256 of its file;
- `route_sets`: the routes that may qualify, by file and digest
  (`mistral_free`, `openrouter_free`; the anonymous OpenCode route is closed
  by provider policy);
- `run_plan`: runs per repository (two), the history window, and the
  patch-check settings;
- `stability`: how many top hotspots and plan steps two runs compare, and
  the share they must have in common;
- `usefulness.threshold`: the useful share of counted items a route must
  reach, `null` until the first measured runs set it;
- `protocol_suite`: the eight generated lifecycle repositories
  (`tooling/review_eval/generate_lifecycle_repo.py`, by generator and
  source digest), kept as a regression of the review mechanics rather than
  as qualification.

## Recording Runs

`tooling/ecosystem/review_inputs.py prepare` fetches each repository at its
pinned commit with a window of history, scans it with a repository map,
writes a history digest, runs the review while the checkout exists, and
deletes the checkout. `--review-runs N` runs the review command N times,
with `{run}` numbering the outputs:

```sh
python3 tooling/ecosystem/review_inputs.py prepare --label L --window-days 365 \
  --review-runs 2 --review-command "target/release/auditor review run \
  --experimental-reviewer --report {report} --history {history} \
  --routes tooling/live_provider/mistral_free_routes_v2.json \
  --brief config/auditor/review-brief-general-v1.json --allow-hosted-review \
  --check-patches --max-patch-checks 8 --repo-root {repo} \
  --transcript {out}/transcript-{run}.json --output {out}/review-{run}.json"
```

What stays per repository is the report, its map, the history digest, the
review records, and their transcripts: enough to qualify offline.

Preparing the same label again continues it. A repository whose inputs and
runs are all recorded is skipped; a prepared one keeps its report and
digest and is fetched again only for its pending runs. A run whose record
says it stopped (an outage outlasted the retries and resends, or the run
was interrupted after the checkpoint `review run --output` writes after
each question) is resumed with `--resume` on that record, so no answered
question is asked, or paid for, twice. `prepare` exits nonzero while any
run is still stopped. Each run's progress lines (questions, retries and
their waits, target changes, slow requests) go to `prepare`'s stderr,
labeled with the run's record path.

## Automatic Gates

`auditor review qualify` checks recorded runs of one repository against the
report, map, and history they read
(`contracts/review-qualification-v1.schema.json`). It reads only files, so
it runs after the checkout is gone. It exits 1 with the result on stdout
when a gate fails. Each run passes six gates:

- **binding**: the record's schema, source digest, template digest, prompt
  bundle, routes digest, brief digest, and orientation pack are the ones the
  inputs give, and its questions are the ones the view asks again;
- **coverage**: every question was answered, so an empty run cannot pass;
- **citations**: each answer passes its question's checks again against the
  evidence the review showed it (the pack, the questions so far, every
  retrieval result, and for the plan the earlier answers it depends on), and
  every ID an answer cites or a retrieval call issued names a finding,
  file, symbol, edge, entry point, module, dependency, cycle, detector, line
  range, or history entry the inputs hold;
- **contradictions**: no claim cites one ID both for and against itself, and
  no claim gives a number for a measured value that none of its cited
  findings measured (`cyclomatic complexity is 12` when the only cited
  finding says 31; a claim about two findings may give each one's value).
  Whether a judgment is right is the rubric's `correct`;
- **patch checks**: every accepted recommendation with a patch carries the
  host's check, with a verdict of verified or refuted;
- **budget**: every call went to a route target; the recorded spend adds up
  from the calls, with costs from the routes; the review's and each
  question's provider budget, the output-token limit, the retrieval budget
  (the brief tightens it), and the patch-check limit all hold.

Across runs, **stability** compares each pair: every hotspot question's top
`top` subjects, and the findings the plan's top `top` steps resolve (step
IDs are content-derived, so findings are what two plans share), must
overlap by at least `min_overlap`. A question one run answered and the
other did not is a disagreement, and one run alone cannot pass.

## Usefulness Rubric

`auditor review rubric --review R` writes an unscored sheet
(`contracts/review-usefulness-v1.schema.json`): one item per answered claim
and recommendation, with its text, cited evidence, and patch verdict. A
scorer marks each item on four criteria: correct, specific, actionable, and
novel (not already stated by a finding). An item is useful when all four
hold.

A second model, blind to the first, may pre-label items. A model's score is
a `pre_label` until a different model agrees (`corroborated`) or a person
scores it (`final`). Scorers who disagree raise a dispute that only a person
settles. No model the review ran on may score it. Only corroborated and
final items count. `review qualify --scores S` checks each sheet against
its review and reports the counts.

## The Bar

`review_inputs.py qualify --label L --route-set NAME` checks the corpus
bindings, runs `auditor review qualify` on every corpus repository with the
corpus route set, brief, and stability policy, and writes a summary with
the gates per repository, the counted usefulness, and a decision. A route
qualifies when the gates pass on every repository and the useful share
meets the threshold. Until the threshold is set, the best decision is
"gates pass; the usefulness bar is not set yet".

Hermetic tests run the same harness on scripted reviews of a fixture scan
(`crates/auditor-review/tests/review_qualification.rs`, over the fake
provider of `review_driver.rs`). They prove the mechanics, not live
quality.

## Transcripts

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

## Offline Improvement

Prompt and template wording changes only by a deliberate, digest-bound edit
of the checked-in bundle, reviewed like any other change, followed by a new
corpus binding. The manifests under `eval/review-improvement/` record
candidates tried on the v2 protocol; the tooling that ran them was removed
with it.

## Qualification Record

The dated record of what has and has not qualified, kept so a later run can
be compared with it. The entries below were measured on the removed v2
protocol and its lifecycle matrix; as of 1.5.0 no route passed that matrix.
No route has yet qualified on the rebuilt review.

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
  (then about 7.5 seconds; now 3.75 minutes of quick retries and then
  resends every five minutes) could not wait out a per-minute window.

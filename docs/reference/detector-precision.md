# Detector Precision

Measured precision of Engineering Auditor detectors on natural open-source
code. This page is the input to precision gating
([`detector-promotion.md`](./detector-promotion.md)): a detector meets the bar
when **the 95% lower confidence bound of its precision is ≥ 85%** over final
or corroborated labels plus accepted oracle-confirmed findings, with the bound
holding even if every unverified pre-label were a false positive. That takes
at least 22 clean labels, or about 40 with one false positive.

## Status

> **Provisional.** The first labeller is a model (`model:claude-opus-5-5`).
> A pre-label counts once an independent check corroborates it: a reference
> tool that confirms or refutes the finding's factual claim, or a second,
> different model that reaches the same verdict blind. A human confirms a
> label with `labels.py confirm`; every disputed pre-label so far (254 in
> all) was reviewed and confirmed as human-final.
>
> On the 40-repository corpus (scan `t22-a`), 18 detectors meet the bar
> (`assert_in_production`, `class_structure_metrics`,
> `cognitive_complexity`, `excessive_nesting_depth`, `function_complexity`,
> `function_local_import`, `go_errcheck`, `halstead_information_density`,
> `kt_non_null_assertion`, `large_file`, `naming_convention`,
> `npath_complexity`, `out_parameter`, `source_duplicate_detection`,
> `todo_unimplemented_in_production`, `too_many_parameters`,
> `too_many_returns`, `too_many_statements`), 9 are below it, and 50 lack a
> sample that can decide the bound. 3,113 oracle-confirmed findings are
> accepted through clean agreement audits. Every detector that meets the
> bar gates. For most of the 50, every corpus finding is already labelled:
> the corpus cannot decide them. Such a detector gates only with no live
> false positive where it gates (operator decision, 2026-10-18; see
> [Precision Gating](./detector-promotion.md#precision-gating)). Cheap
> mechanisms were fixed first (round eight below); six detectors with
> residual false positives moved to `full` and four were scoped by
> language.
>
> The new full-only test-quality detectors (`test_without_assertion`,
> `tautological_assertion`) have every corpus finding pre-labelled (13/1 and
> 11/0). OpenCode's free tier now refuses this network (HTTP 403), so a
> Claude Sonnet reviewer checked them blind: it corroborates 23 and leaves one
> unclear on a truncated excerpt. It ruled the trpc compile-time regression
> test a true positive; a third arbiter (Claude Fable) upheld the false
> positive (its hooks run inside `ignoreErrors`, and CI type-checks the test
> folder), which the operator confirmed as final. Before labelling, a
> review of the first scan fixed the mechanisms behind 133 of the 169
> findings (round nine under [Fixed Noise Mechanisms](#fixed-noise-mechanisms)).
>
> The full-only `skipped_test_without_reason` reports 10 library-corpus
> skips, all pre-labelled true positives (9 JUnit `@Ignore`/`@Disabled`
> with no reason anywhere, 1 Vitest `it.skip` whose title only hints at
> one); the labels await a blind second check and are not yet in the table
> below, whose scans predate the detector. Before labelling, the first scan's
> 30 findings showed that doc comments and class docs often carry the
> reason ("This test fails because..", "All tests in this file will be
> disabled"); reading those removed 20 findings.
>
> The full-only `network_call_without_timeout` has no library-corpus
> finding (fmt's release-script `urlopen` calls now have the tooling role),
> so it is measured on the application slice (scan `a1-b`; see the corpus
> README): 21 findings, 20 true positives (Go, Rust reqwest, and Node and
> browser axios calls without a timeout) and one false positive, a default
> `HttpClient` in Dependency-Track's plugin test-support module. The Sonnet
> reviewer corroborates 20 and calls one unclear (Hoppscotch's CLI fetch
> shim, whose per-request config also sets no timeout). At 95% the sample
> is still too small to decide the bar. The slice's first scan fixed
> `axios(config)` being read as a URL call, at the cost of one true
> positive (Hoppscotch's CLI `axios(requestConfig)`, whose config sets no
> timeout).
>
> Of the full-only Layer 3 additions, `quadratic_front_removal` has four
> corpus findings, all true positives the Sonnet reviewer corroborates (two
> in black bounded by bracket nesting depth, one hono queue bounded by path
> segments: correct, of modest practical impact), and
> `catastrophic_backtracking_regex` has none after its two hono findings were
> fixed (round ten).
>
> Of the dependency and error hygiene rules, `unused_declared_dependency` has
> one corpus finding (fd's `libc`, which no Rust file references), a true
> positive the Sonnet reviewer corroborates, and `discarded_fallible_result`
> has two true positives: hono's unhandled update-hook promise (corroborated)
> and axios's `CancelToken` listener promise. The axios one was pre-labelled a
> false positive (its promise only resolves); the Sonnet reviewer disputed
> it, and the Fable arbiter ruled it a true positive (the derived promise
> rejects when a listener throws, and `subscribe` is public API), confirmed
> as final by operator delegation. A first,
> partial scan reported 41 findings in the repositories it reached; round
> eleven removed the mechanisms behind 38 of them.
>
> No pre-label is disputed. The operator confirmed the `out_parameter`
> labels as final, upholding them against 15 second-model disputes that
> matched earlier final rulings (`reverse`, `swap`, `fill`, Jackson's
> output helpers). The operator also confirmed the third arbiter's rulings as final: those upholding the first labeller against the second
> model, and its rulings on the disputes the strict recount raised. Where
> review found a second model right, the label was corrected;
> where a dispute exposed a detector bug, the bug was fixed (rounds four to
> seven under [Fixed Noise Mechanisms](#fixed-noise-mechanisms)). The second
> model is `opencode/deepseek-v4.1-flash-free` for most labels; OpenCode's
> free tier served it only intermittently to the CLI, so the remaining
> pre-labels were checked by Claude Sonnet subagents, which rechecked 900 of
> them on whole-function excerpts and recounted the 377 they left `unclear`
> under strict metric rules. Recount verdicts that rested on hand NPath counts
> or excluded closure locals were dropped: both departed from the documented
> metric definitions. Review excerpts span a function-level finding's whole
> function (up to 120 lines).

## Method

- **Corpus.** The 40 pinned repositories in
  [`eval/ecosystem/manifest.json`](../../eval/ecosystem/manifest.json), four
  per Tier-1 and Tier-2 language, scanned with the `full` profile. Checkouts
  are depth 1, so history detectors are silent.
- **Sample.** `tooling/ecosystem/labels.py sample` draws up to 30 findings per
  detector and file language, round-robin across repositories in seeded-hash
  order (seed `ea-v1`). Detectors with fewer findings are labelled in full.
  Tier-2 cells were sampled at up to 12 per bucket (`--cap 12`).
  1,284 samples came from the scan at `a159358`; 19 more topped up buckets
  from the post-fix scan.
- **Verdict standard.**
  - `true_positive`: the finding's claim is factually correct, and the
    detector's stated rationale applies at that site within its stated scope.
    Intentional code still counts when the claim holds; an author-suppressed
    empty `catch` is a true positive.
  - `false_positive`: the claim is wrong (miscount, wrong symbol, misparsed
    syntax); the site is outside the detector's scope (test, fixture,
    generated, or tooling code); or the rationale does not apply (an idiom
    the rule should recognise).
  - `unclear`: needs context the sample does not show. Excluded from
    precision.
- **Corroboration.** Each pre-label can carry independent checks.
  - *Oracles* ([`tooling/ecosystem/oracles.py`](../../tooling/ecosystem/oracles.py))
    run a pinned reference tool on the finding's file and confirm or refute
    its factual claim (table below). A confirmed claim agrees with a true
    positive. A refuted claim agrees with a false positive and contradicts a
    true positive. A confirmed claim is neutral toward a false positive,
    because linters do not judge scope or idiom.
  - *A second model* labels the same finding blind, through a free route
    with every tool call rejected
    ([`model_review.py`](../../tooling/ecosystem/model_review.py)). It agrees
    when it reaches the same decided verdict. Where the free route was
    unavailable, Claude Sonnet subagents (`model:claude-sonnet-5-5`) labelled
    the same blind bundles, reading the full source file when the excerpt
    was too short; any model other than the first labeller counts.
  - A pre-label with at least one agreeing check and no contradicting one is
    `corroborated`. A contradicted pre-label is *disputed* and waits for
    review.
- **Precision** is TP / (TP + FP) over live final and corroborated labels.
  Labels whose finding no longer appears are stale and excluded.
- **Accepted findings.** When an oracle's confirmed findings audit clean (at
  least 30 verified labels, no false positive), the detector's unlabelled
  confirmed findings count as true positives
  ([Oracle Agreement Audit](#oracle-agreement-audit)). The claims snapshot is
  [`eval/ecosystem/oracle-claims.json`](../../eval/ecosystem/oracle-claims.json).
- **Standing** is a sound bound over the unverified pre-labels, because
  corroboration comes far more easily for true positives (a linter confirms
  the measurement) than for scope false positives:
  - `meets_bar`: the 95% Wilson lower bound of precision (labels plus
    accepted findings) is ≥ 85% even were every unverified label a false
    positive;
  - `below_bar`: point precision < 90% even were every unverified label a
    true positive (at least 10 decided labels);
  - `unverified`: verifying the unverified labels could reach the bar;
  - `insufficient_sample`: more labels are needed either way.
- **Labels** live in [`eval/ecosystem/labels/`](../../eval/ecosystem/labels/),
  one file per detector, keyed by finding-ID v3, each with a one-line reason
  and its checks.

### Oracles

| Detector | Language | Oracle | Claim checked | Refutes? |
|---|---|---|---|---|
| `function_complexity` | Python, Rust, Java, Go, Kotlin, C#, C, C++ / JS, TS | lizard 1.24.1 / rust-code-analysis | decisions (CCN − 1) summed over nested functions > 10 | ≤ 8 |
| `cognitive_complexity` | Python / Go / JS, TS / Java / Rust | complexipy 8.0.1 / gocognit 1.2.0 / ESLint SonarJS 3.0.5 / PMD 7.16.0 / rust-code-analysis 0.0.25 | Sonar cognitive complexity > 15 | ≤ 12 |
| `npath_complexity` | Java | PMD | NPath > 200 | ≤ 160 |
| `halstead_information_density` | Tier-1 but Go, and C, C++ | rust-code-analysis | volume > 1500 with difficulty ≥ 15, or difficulty > 30 | clear of both by a fifth |
| `too_many_parameters` | Python / others, Tier-2 included | ruff 0.17.0 `PLR0913` / lizard | parameters > 5 | yes |
| `too_many_returns` | Python / Rust, Java, JS, TS, C, C++ | ruff `PLR0911` / rust-code-analysis | returns > 5 | yes |
| `too_many_statements` | Python | ruff `PLR0915` | statements > 50 | yes |
| `too_many_locals` | Python | ruff `PLR0914` | locals > 15 | no; ruff omits loop and `with` targets |
| `large_file` | all, Tier-2 included | nonblank recount | nonblank lines over the threshold | yes |
| `assert_in_production`, `console_output_in_production`, `shell_execution`, `random_for_security`, `empty_catch_block` | Python | ruff `S101`, `T201`, `S602`, `S311`, `S110` | the flagged construct is at that line | yes |
| `function_local_import` | Python | ruff `PLC0415` | the import is at that line | no; ruff omits `__import__` calls |
| `console_output_in_production`, `duplicate_import`, `empty_catch_block`, `no_await_in_loop` | JS, TS | ESLint 9.39.1 | the flagged construct is at that line (ES `import` only) | yes |
| `excessive_nesting_depth` | JS, TS | ESLint `max-depth` | depth > 5 at the deepest line | no |
| `naming_convention` | Java | PMD naming rules | the name is flagged | yes |
| `go_errcheck` | Go | errcheck 1.9.0 | the ignored error is flagged | yes |
| `go_repeated_string_literal` | Go | goconst 1.8.2 | the literal repeats ≥ 3 times | no |
| `source_duplicate_detection` | all | jscpd 4.0.5 | a duplicate pair spans two occurrences | no |

Oracles see only the files holding labelled findings, at the pinned corpus
commits, with inline suppressions ignored. Detectors without an oracle rely
on the second model. lizard is not used for JavaScript or TypeScript: it
ends a TypeScript arrow-function constant at its first nested arrow and
misreads typed signatures, so it refuted correct counts (zod `mergeValues`:
lizard 0, EA 11). `out_parameter` has no oracle since it stopped reporting
reassignment.

## Results

### Before and After the Task 7 Fixes

"Before" is the scan at `a159358`. "After" is the scan with the v1.0 Task 7
fixes. Both columns count every pre-label with its current verdict, verified
or not, so they show the effect of the fixes, not a gating standing.

| Detector | Profile | Findings | TP/FP before | Before | TP/FP after | After |
|---|---|---:|---:|---:|---:|---:|
| `architectural_layering` | gating | 3 → 0 | 0/3 | 0% | 0/0 | — |
| `assert_in_production` | gating | 175 → 175 | 29/31 | 48% | 29/31 | 48% |
| `blocking_io_in_async_context` | full | 7 → 7 | 7/0 | 100% | 7/0 | 100% |
| `class_structure_metrics` | full | 32 → 22 | 20/11 | 65% | 20/2 | 91% |
| `cognitive_complexity` | full | 349 → 349 | 153/1 | 99% | 153/1 | 99% |
| `collection_parameter_mutation` | full | 17 → 4 | 5/12 | 29% | 4/0 | 100% |
| `console_output_in_production` | gating | 55 → 9 | 5/26 | 16% | 5/4 | 56% |
| `constant_condition` | full | 1 → 0 | 0/1 | 0% | 0/0 | — |
| `dead_code` | full | 1 → 0 | 0/1 | 0% | 0/0 | — |
| `duplicate_import` | gating | 122 → 7 | 6/29 | 17% | 6/1 | 86% |
| `empty_catch_block` | gating | 1 → 1 | 1/0 | 100% | 1/0 | 100% |
| `escape_hatch_type_density` | full | 1 → 1 | 1/0 | 100% | 1/0 | 100% |
| `excessive_nesting_depth` | gating | 14 → 14 | 11/1 | 92% | 11/1 | 92% |
| `function_complexity` | gating | 163 → 163 | 120/1 | 99% | 120/1 | 99% |
| `function_local_import` | gating | 54 → 54 | 2/27 | 7% | 2/27 | 7% |
| `go_errcheck` | gating | 57 → 57 | 30/0 | 100% | 30/0 | 100% |
| `go_log_fatal` | gating | 2 → 2 | 2/0 | 100% | 2/0 | 100% |
| `go_repeated_string_literal` | full | 32 → 11 | 11/20 | 35% | 11/0 | 100% |
| `god_module` | full | 47 → 47 | 38/4 | 90% | 38/4 | 90% |
| `halstead_information_density` | full | 87 → 71 | 71/15 | 83% | 70/1 | 99% |
| `identifier_stutter` | full | 63 → 63 | 0/54 | 0% | 0/54 | 0% |
| `identifier_tersity` | gating | 1 → 1 | 0/1 | 0% | 0/1 | 0% |
| `known_size_preallocation` | full | 2 → 2 | 2/0 | 100% | 2/0 | 100% |
| `large_file` | gating | 41 → 41 | 39/2 | 95% | 39/2 | 95% |
| `loop_invariant_regex_compilation` | gating | 1 → 1 | 0/1 | 0% | 0/1 | 0% |
| `magic_literal_cluster` | gating | 66 → 18 | 14/33 | 30% | 14/4 | 78% |
| `naming_convention` | gating | 108 → 104 | 28/5 | 85% | 28/1 | 97% |
| `nested_quadratic_iteration` | gating | 3 → 1 | 0/3 | 0% | 0/1 | 0% |
| `no_await_in_loop` | full | 2 → 2 | 1/1 | 50% | 1/1 | 50% |
| `npath_complexity` | full | 68 → 68 | 67/1 | 99% | 67/1 | 99% |
| `orphan_symbol` | full | 2 → 0 | 0/2 | 0% | 0/0 | — |
| `out_parameter` | gating | 58 → 58 | 30/0 | 100% | 30/0 | 100% |
| `package_coupling_metrics` | gating | 4 → 4 | 0/4 | 0% | 0/4 | 0% |
| `per_item_external_io` | full | 4 → 4 | 0/4 | 0% | 0/4 | 0% |
| `random_for_security` | full | 3 → 3 | 0/3 | 0% | 0/3 | 0% |
| `repeated_collection_traversal` | gating | 1 → 1 | 1/0 | 100% | 1/0 | 100% |
| `repeated_expensive_initialization` | full | 3 → 3 | 1/2 | 33% | 1/2 | 33% |
| `section_divider_comment` | gating | 7 → 1 | 1/6 | 14% | 1/0 | 100% |
| `shell_execution` | gating | 2 → 2 | 2/0 | 100% | 2/0 | 100% |
| `sibling_call_redundancy` | gating | 2 → 2 | 0/2 | 0% | 0/2 | 0% |
| `source_duplicate_detection` | gating | 121 → 121 | 37/23 | 62% | 37/23 | 62% |
| `stale_link_reference` | gating | 105 → 6 | 6/33 | 15% | 6/0 | 100% |
| `stale_section_anchor` | gating | 13 → 3 | 3/10 | 23% | 3/0 | 100% |
| `todo_unimplemented_in_production` | gating | 6 → 6 | 6/0 | 100% | 6/0 | 100% |
| `too_many_locals` | full | 29 → 29 | 29/0 | 100% | 29/0 | 100% |
| `too_many_parameters` | gating | 127 → 125 | 43/2 | 96% | 43/0 | 100% |
| `too_many_returns` | full | 55 → 55 | 48/0 | 100% | 48/0 | 100% |
| `too_many_statements` | full | 8 → 8 | 8/0 | 100% | 8/0 | 100% |
| `unhandled_termination_in_production` | gating | 48 → 48 | 0/30 | 0% | 0/30 | 0% |
| `unreachable_code` | gating | 14 → 0 | 0/14 | 0% | 0/0 | — |
| **All sampled** | | 2187 → 1774 | 878/419 | 68% | 876/207 | 81% |

Detectors absent from the table produced no findings on the corpus. Small
corpus counts (one to four findings) say little in either direction.

### Corroborated Standing

The gating standing counts only final and corroborated labels, as a sound
bound over the unverified rest (see Method). Generated by
`labels.py precision --scan t22-a a1-b --claims eval/ecosystem/oracle-claims.json`
on the 40-repository corpus after the second model's pass and the
small-sample fixes of 2026-10-18; stale labels (findings the fixes removed)
are excluded. The same table is committed as `eval/ecosystem/precision.json`, which the
detector architecture contract reads (see
[Precision Gating](./detector-promotion.md#precision-gating)).
"Uncorroborated" lists the pre-labels that do not count yet, disputed ones
included.

Gating bar: the 95% lower confidence bound of precision >= 85% over final or corroborated TP+FP labels plus accepted oracle-confirmed findings, holding even if every unverified pre-label were a false positive; below the bar when the best-case precision is under 90%. Confirmed findings are accepted only after an agreement audit of at least 30 verified labels with no false positive.

| Detector | TP | FP | Unclear | Precision | Agreement audit | Accepted | Standing | Final / corroborated | Disputed | Uncorroborated | Oracle agrees | Model agrees |
|---|---:|---:|---:|---:|---:|---:|---|---:|---:|---|---:|---:|
| `architectural_layering` | 6 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 6 | 0 | — | — | 6/6 |
| `assert_in_production` | 35 | 1 | 0 | 97% | 33/1 | 0 | meets_bar | 1 / 35 | 0 | — | 33/33 | 69/70 |
| `bind_all_interfaces` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `blocking_io_in_async_context` | 1 | 2 | 0 | 33% | — | 0 | insufficient_sample | 3 / 0 | 0 | — | — | 3/4 |
| `class_structure_metrics` | 133 | 5 | 0 | 96% | — | 0 | meets_bar | 15 / 123 | 0 | true_positive 1 | — | 154/167 |
| `cognitive_complexity` | 266 | 12 | 0 | 96% | 141/0 | 512 | meets_bar | 13 / 265 | 0 | — | 141/143 | 255/265 |
| `collection_parameter_mutation` | 13 | 7 | 0 | 65% | — | 0 | below_bar | 5 / 15 | 0 | — | — | 18/23 |
| `console_output_in_production` | 62 | 22 | 0 | 74% | 46/16 | 0 | below_bar | 20 / 64 | 0 | — | 47/48 | 82/105 |
| `constant_condition` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `cpp_raw_ptr_ownership` | 22 | 8 | 0 | 73% | — | 0 | below_bar | 20 / 10 | 0 | — | — | 10/30 |
| `csharp_linq_enumeration` | 0 | 9 | 0 | 0% | — | 0 | insufficient_sample | 1 / 8 | 0 | — | — | 8/9 |
| `dead_code` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `debugger_in_production` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `discarded_fallible_result` | 2 | 0 | 0 | 100% | — | 0 | insufficient_sample | 1 / 1 | 0 | — | — | 3/3 |
| `duplicate_import` | 13 | 1 | 0 | 93% | 0/1 | 0 | insufficient_sample | 10 / 4 | 0 | — | — | 11/26 |
| `empty_catch_block` | 18 | 0 | 0 | 100% | 9/0 | 0 | insufficient_sample | 1 / 17 | 0 | — | 9/9 | 21/22 |
| `empty_conditional_branch` | 7 | 1 | 0 | 88% | — | 0 | insufficient_sample | 2 / 6 | 0 | — | — | 6/8 |
| `escape_hatch_type_density` | 12 | 0 | 0 | 100% | — | 0 | insufficient_sample | 1 / 11 | 0 | — | — | 11/12 |
| `eval_in_production` | 5 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 5 | 0 | — | — | 5/5 |
| `excessive_nesting_depth` | 41 | 1 | 0 | 98% | 12/0 | 0 | meets_bar | 1 / 41 | 0 | — | 12/12 | 32/33 |
| `exit_in_production` | 4 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 4 | 0 | — | — | 4/4 |
| `function_complexity` | 183 | 2 | 0 | 99% | 116/0 | 654 | meets_bar | 9 / 176 | 0 | — | 116/116 | 108/117 |
| `function_local_import` | 30 | 0 | 0 | 100% | 30/0 | 38 | meets_bar | 2 / 28 | 0 | — | 30/30 | 54/56 |
| `go_errcheck` | 27 | 0 | 0 | 100% | 27/0 | 0 | meets_bar | 10 / 17 | 0 | — | 27/27 | 33/43 |
| `go_log_fatal` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `go_repeated_string_literal` | 12 | 0 | 0 | 100% | 5/0 | 0 | insufficient_sample | 0 / 12 | 0 | — | 5/5 | 17/17 |
| `god_module` | 30 | 20 | 0 | 60% | — | 0 | below_bar | 2 / 48 | 0 | — | — | 51/53 |
| `halstead_information_density` | 138 | 3 | 0 | 98% | 65/0 | 519 | meets_bar | 3 / 138 | 0 | — | 65/65 | 127/130 |
| `hardcoded_credential` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `hot_path_linear_membership` | 1 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 1 | 0 | — | — | 1/1 |
| `identifier_stutter` | 19 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 19 | 0 | — | — | 33/33 |
| `identifier_tersity` | 1 | 1 | 0 | 50% | — | 0 | insufficient_sample | 1 / 1 | 0 | — | — | 2/3 |
| `insecure_cryptography` | 0 | 3 | 0 | 0% | — | 0 | insufficient_sample | 2 / 1 | 0 | — | — | 1/3 |
| `intermediate_collection_materialization` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `java_inefficient_string_buffer` | 0 | 3 | 0 | 0% | — | 0 | insufficient_sample | 0 / 3 | 0 | — | — | 3/3 |
| `known_size_preallocation` | 1 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 1 | 0 | — | — | 2/2 |
| `kt_non_null_assertion` | 30 | 0 | 0 | 100% | — | 0 | meets_bar | 0 / 30 | 0 | — | — | 30/30 |
| `kt_require_check_misuse` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `large_file` | 149 | 7 | 0 | 96% | 149/7 | 0 | meets_bar | 7 / 149 | 0 | — | 149/149 | 141/148 |
| `loop_invariant_regex_compilation` | 1 | 0 | 0 | 100% | — | 0 | insufficient_sample | 1 / 0 | 0 | — | — | 0/1 |
| `magic_literal_cluster` | 57 | 20 | 0 | 74% | — | 0 | below_bar | 18 / 59 | 0 | — | — | 71/89 |
| `map_entry_opportunity` | 0 | 1 | 0 | 0% | — | 0 | insufficient_sample | 0 / 1 | 0 | — | — | 1/1 |
| `naming_convention` | 46 | 1 | 0 | 98% | 30/0 | 720 | meets_bar | 12 / 35 | 0 | — | 30/30 | 74/87 |
| `nested_cross_collection_traversal` | 0 | 3 | 0 | 0% | — | 0 | insufficient_sample | 0 / 3 | 0 | — | — | 3/3 |
| `nested_quadratic_iteration` | 0 | 2 | 0 | 0% | — | 0 | insufficient_sample | 1 / 1 | 0 | — | — | 1/2 |
| `no_await_in_loop` | 8 | 10 | 0 | 44% | 7/8 | 0 | below_bar | 5 / 13 | 0 | — | 7/7 | 14/19 |
| `network_call_without_timeout` | 20 | 1 | 0 | 95% | — | 0 | insufficient_sample | 0 / 21 | 0 | true_positive 1 | — | 21/21 |
| `npath_complexity` | 227 | 9 | 0 | 96% | 30/0 | 45 | meets_bar | 8 / 228 | 0 | true_positive 16 | 30/30 | 227/234 |
| `orphan_symbol` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `out_parameter` | 81 | 1 | 0 | 99% | — | 0 | meets_bar | 82 / 0 | 0 | — | — | 43/67 |
| `package_coupling_metrics` | 2 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 2 | 0 | — | — | 2/2 |
| `per_item_external_io` | 0 | 5 | 0 | 0% | — | 0 | insufficient_sample | 1 / 4 | 0 | — | — | 7/8 |
| `random_for_security` | 1 | 24 | 0 | 4% | 0/3 | 0 | below_bar | 1 / 24 | 0 | — | — | 27/28 |
| `quadratic_front_removal` | 4 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 4 | 0 | — | — | 4/4 |
| `repeated_collection_traversal` | 1 | 0 | 0 | 100% | — | 0 | insufficient_sample | 1 / 0 | 0 | — | — | 0/1 |
| `repeated_expensive_initialization` | 1 | 2 | 0 | 33% | — | 0 | insufficient_sample | 0 / 3 | 0 | — | — | 4/4 |
| `repeated_format_allocation` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `request_no_cert_verify` | 2 | 2 | 0 | 50% | — | 0 | insufficient_sample | 0 / 4 | 0 | — | — | 4/4 |
| `require_atomic_updates` | 0 | 1 | 0 | 0% | — | 0 | insufficient_sample | 0 / 1 | 0 | — | — | 1/1 |
| `resource_not_closed` | 2 | 0 | 0 | 100% | — | 0 | insufficient_sample | 1 / 1 | 0 | — | — | 1/2 |
| `section_divider_comment` | 4 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 4 | 0 | — | — | 4/4 |
| `shell_execution` | 0 | 0 | 0 | — | — | 0 | insufficient_sample | 0 / 0 | 0 | — | — | — |
| `sibling_call_redundancy` | 3 | 3 | 0 | 50% | — | 0 | insufficient_sample | 0 / 6 | 0 | — | — | 6/6 |
| `source_duplicate_detection` | 169 | 9 | 0 | 95% | 32/0 | 36 | meets_bar | 8 / 170 | 0 | — | 32/32 | 268/276 |
| `sql_injection` | 0 | 2 | 0 | 0% | — | 0 | insufficient_sample | 0 / 2 | 0 | — | — | 2/2 |
| `ssl_with_bad_defaults` | 2 | 1 | 0 | 67% | — | 0 | insufficient_sample | 0 / 3 | 0 | — | — | 3/3 |
| `stale_link_reference` | 6 | 0 | 0 | 100% | — | 0 | insufficient_sample | 4 / 2 | 0 | — | — | 3/10 |
| `stale_section_anchor` | 14 | 0 | 0 | 100% | — | 0 | insufficient_sample | 3 / 11 | 0 | — | — | 11/14 |
| `stringly_typed_parameter` | 1 | 3 | 0 | 25% | — | 0 | insufficient_sample | 2 / 2 | 0 | — | — | 2/4 |
| `tautological_assertion` | 11 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 11 | 0 | — | — | 11/11 |
| `test_without_assertion` | 12 | 1 | 0 | 92% | — | 0 | insufficient_sample | 1 / 12 | 0 | true_positive 1 | — | 13/14 |
| `todo_unimplemented_in_production` | 143 | 2 | 0 | 99% | — | 0 | meets_bar | 3 / 142 | 0 | — | — | 237/240 |
| `too_many_locals` | 109 | 12 | 0 | 90% | 18/1 | 0 | insufficient_sample | 2 / 119 | 0 | true_positive 5 | 18/18 | 115/115 |
| `too_many_parameters` | 89 | 0 | 0 | 100% | 38/0 | 188 | meets_bar | 6 / 83 | 0 | — | 38/42 | 123/125 |
| `too_many_returns` | 128 | 0 | 0 | 100% | 63/0 | 399 | meets_bar | 12 / 116 | 0 | — | 63/63 | 81/96 |
| `too_many_statements` | 97 | 2 | 0 | 98% | 11/0 | 0 | meets_bar | 1 / 98 | 0 | — | 11/11 | 87/88 |
| `unhandled_termination_in_production` | 0 | 30 | 0 | 0% | — | 0 | below_bar | 18 / 12 | 0 | — | — | 29/56 |
| `unreachable_code` | 3 | 0 | 0 | 100% | — | 0 | insufficient_sample | 2 / 1 | 0 | — | — | 1/3 |
| `unsafe_deserialization` | 4 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 4 | 0 | — | — | 4/4 |
| `unsafe_type_cast` | 26 | 4 | 0 | 87% | — | 0 | below_bar | 1 / 29 | 0 | — | — | 29/30 |
| `unused_assignment` | 4 | 3 | 0 | 57% | — | 0 | insufficient_sample | 2 / 5 | 0 | — | — | 5/8 |
| `unused_declared_dependency` | 1 | 0 | 0 | 100% | — | 0 | insufficient_sample | 0 / 1 | 0 | — | — | 1/1 |
| `xml_xxe_vulnerable` | 0 | 3 | 0 | 0% | — | 0 | insufficient_sample | 0 / 3 | 0 | — | — | 3/3 |

### Oracle Agreement Audit

`oracles.py overlap --scan t16-p --json eval/ecosystem/oracle-claims.json` runs the oracles over every finding
in the scan, not only labelled ones, and splits each detector's findings into
three strata: the oracle confirms the claim, refutes it, or cannot judge
(including languages it does not cover). The labelled findings an oracle
confirmed form the agreement audit: if they hold no false positive across at
least 30 labels (a 95% lower bound near 89%), every confirmed finding may
count as a true positive, which is the "sample if agreement accepted" column.
Otherwise only the audited labels count. Stratified precision weights each
stratum's labelled precision by its size, so the unlabelled majority of a
detector's findings is represented instead of the sampled cap.

An oracle confirms a factual claim, not scope or idiom. The audit shows where
that matters: agreement holds for the metric detectors and fails for the
judgment detectors, whose confirmed findings include false positives the
oracle cannot see (`random_for_security` 0/3, `console_output_in_production`
38/16, `no_await_in_loop` 7/4, `large_file` 147/3, and `cognitive_complexity`
140/1, whose one false positive is a tooling script). Every refuted finding is
already labelled. The gating standing above counts accepted findings from
the committed claims snapshot.

| Detector | Findings | Confirmed | Agreement audit (TP/FP) | Audit lower bound | Refuted labelled (TP/FP) | Other labelled (TP/FP) | Stratified precision | Sample if agreement accepted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `assert_in_production` | 127 | 125 | 33/1 | 85% | 0/0 | 2/0 | 97% | 36 |
| `cognitive_complexity` | 1283 | 666 | 144/1 | 96% | 2/0 | 70/8 | 95% | 225 |
| `console_output_in_production` | 88 | 66 | 46/16 | 62% | 1/1 | 15/5 | 74% | 84 |
| `duplicate_import` | 14 | 1 | 0/1 | 0% | 0/0 | 13/0 | 93% | 14 |
| `empty_catch_block` | 20 | 11 | 9/2 | 52% | 0/0 | 9/0 | 90% | 20 |
| `excessive_nesting_depth` | 49 | 12 | 11/1 | 65% | 0/0 | 9/3 | 79% | 24 |
| `function_complexity` | 921 | 779 | 117/0 | 97% | 0/0 | 47/2 | — | — |
| `function_local_import` | 71 | 68 | 30/0 | 89% | 0/0 | 0/0 | — | — |
| `go_errcheck` | 30 | 27 | 27/0 | 88% | 0/3 | 0/0 | 90% | 30 |
| `go_repeated_string_literal` | 12 | 5 | 5/0 | 57% | 0/0 | 7/0 | 100% | 12 |
| `halstead_information_density` | 706 | 592 | 68/0 | 95% | 0/0 | 51/4 | 99% | 647 |
| `large_file` | 172 | 172 | 149/7 | 91% | 0/0 | 0/0 | 96% | 156 |
| `naming_convention` | 810 | 750 | 30/0 | 89% | 0/0 | 16/1 | — | — |
| `no_await_in_loop` | 18 | 15 | 7/8 | 25% | 0/0 | 1/2 | 44% | 18 |
| `npath_complexity` | 537 | 75 | 30/0 | 89% | 0/0 | 112/8 | — | — |
| `random_for_security` | 25 | 3 | 0/3 | 0% | 0/0 | 1/21 | 4% | 25 |
| `shell_execution` | 7 | 2 | 2/0 | 34% | 0/0 | 0/5 | 29% | 7 |
| `source_duplicate_detection` | 307 | 68 | 32/0 | 89% | 0/0 | 131/9 | 95% | 208 |
| `too_many_locals` | 131 | 23 | 18/1 | 75% | 0/0 | 9/8 | 60% | 36 |
| `too_many_parameters` | 333 | 226 | 38/0 | 91% | 4/0 | 45/0 | 100% | 275 |
| `too_many_returns` | 534 | 469 | 63/0 | 94% | 0/0 | 29/0 | 100% | 498 |
| `too_many_statements` | 109 | 11 | 11/0 | 74% | 0/0 | 13/3 | 83% | 27 |

## Oracle Findings

The first oracle pass (610 checks) contradicted 51 true-positive pre-labels,
and every contradiction traced back to a detector or oracle defect, not to
the labeller:

- **`cognitive_complexity` departed from the Sonar specification** (40
  disputed). It added +1 per `match`/`switch` arm instead of per switch, gave
  `else if` chains nesting credit, counted every boolean operator instead of
  each run, and emitted at 15 where Sonar flags above 15. It also folded
  JavaScript `function` expression callbacks into the enclosing function.
  Fixed: the scorer follows the specification (see
  [`layer2-structural-engineering.md`](./layer2-structural-engineering.md#cognitive_complexity-full-only)).
- **Java `npath_complexity` multiplied across early exits** (6 of 11
  refuted; EA 7,290 where PMD reports 56): gson's guard-clause chains doubled
  at every `if (..) return`. It also ignored brace-less branches and Java's
  `else if`, and counted `catch` handlers. Fixed: paths that leave the
  function add, as in PMD, and tests pin each construct to PMD 7.16.
- **`too_many_parameters` counted `@typing.overload` stubs.** Fixed.
- **lizard mis-spans TypeScript functions**, so its 4 `function_complexity`
  refutations were wrong; rust-code-analysis replaces it for JavaScript and
  TypeScript and confirms all 33 findings.

The dispute review found four more defects, each pinned to PMD output by
tests:

- **`cognitive_complexity` counted `return name;` as a labeled jump** (+1 per
  returned identifier in Java, JavaScript, and Python), missed Java and
  Python recursion (`method_invocation`, `self.f()`), and added recursion per
  call instead of once per function as the whitepaper specifies.
- **`npath_complexity` treated conditions as one path each.** PMD splits a
  condition into true and false outcomes (`a || b` is true two ways), resolves
  labeled `break` to its labeled statement, counts each case label as a path
  with fall-through into the next group, and counts boolean values outside
  conditions. The four Java functions PMD disagreed with now match it exactly.
- **`magic_literal_cluster` read Python docstrings as code** ("4xx or 5xx").
- **`repeated_expensive_initialization` flagged loop-variant constructors**
  (`new RegExp(pattern)` per pattern).

On the dispute-fix scan the oracles agree with 582 of 585 comparable checks.
The remaining refutations: complexipy and rust-code-analysis score two
functions below Sonar's specification, which EA matches by hand count (rich
`Color.parse` 16, ripgrep `LineBuffer::fill` 19), and errcheck agrees with
one `go_errcheck` false-positive label. PMD's own cognitive scorer nests each
further `else if` in a chain one level deeper than the specification; EA
follows the specification.

## Decided Conventions

The blind second model (`opencode/step-5-preview-free`) agreed with the first
labeller on 370 of 544 decided verdicts (68%). Its disagreements were not
miscounts but open questions in the verdict standard, settled once per
detector from the language communities' own conventions. Each decision is
written into the detector's reference entry, with its sources.

| Detector | Decision | Convention | Detector change |
|---|---|---|---|
| `identifier_stutter` | Stutter is a public name repeating the module callers qualify it with | Go Code Review Comments; clippy `module_name_repetitions` | Retargeted from locals to public items in Go packages and public Rust modules; Python dropped (PEP 8 silent, stdlib stutters) |
| `function_local_import` | A deferred import is a true positive unless guarded or documented | PEP 8; pylint/ruff `PLC0415` | `try/except ImportError`, `TYPE_CHECKING`, and a reason comment beside the import are exempt |
| `assert_in_production` | An assert that validates input is a true positive; a type-narrowing assert is not | `python -O`; Bandit B101; Google Python style guide | Python only (Rust `assert!` always runs); `isinstance`/`is not None` asserts exempt |
| `go_errcheck`, all detectors | `_examples/` is example code | `go help packages`: `_`-prefixed directories are skipped | Role components match with a leading `_`; returned or assigned results are not ignored |
| `out_parameter` | A write through a parameter is an output argument; rebinding is not | Java pass-by-value (JLS §8.4.1); Clean Code "output arguments" | Reports field/element writes; owner-type and enum State handlers exempt |
| `blocking_io_in_async_context` | Blocking I/O in one-shot tooling stalls nothing | — | Tooling files excluded; `scripts/` and `tools/` count as tooling at any depth |
| `source_duplicate_detection` | A clone is a true positive when the code can reasonably be extracted into shared code | Operator decision (2026-10-13) | Parameter lists, same-name forwarding keyword arguments, and literal-only argument lists are not tokenized; sync/async twin clones are not reported |
| complexity detectors | JMH benchmark classes are benchmark code | JMH's `*Benchmark` naming | Files named `*Benchmark` take the Benchmark role |
| `naming_convention` | A `static final` mutable collection, array, builder, atomic, or logger is not a constant | Google Java Style §5.2.4 (over PMD `FieldNamingConventions`) | Exempt by declared type |
| `unhandled_termination_in_production` | A documented panic on a violated API contract, or `unreachable!()` for an impossible state, is not an unhandled termination | The Rust Book ch. 9.3 ("To `panic!` or Not"); Rust API Guidelines C-FAILURE | None yet: demoted to `full` (the 18 second-model disputes apply the literal claim) |
| `source_duplicate_detection` | A copy is not extractable debt when sharing would add public API across packages, when it repeats a specification's states, or when each locale owns its messages | Workspace package boundaries (Cargo, npm, Go, Maven/Gradle, Python manifests); the State pattern; per-locale message files | Pairs only within one build package; skips handlers of different states of one Java enum and clones among locale files; tokenizes only function bodies, without Python docstrings; skips row tables (one row shape repeated at least three times, also across two abutting occurrences) |
| `magic_literal_cluster` | A number named by an object key, a regex quantifier, a lookup-table entry, a hash-mixing constant, or a locale's plural rule is not magic | ESLint `no-magic-numbers` (`detectObjects: false`); Checkstyle MagicNumber (`ignoreHashCodeMethod`) | JS/TS object values, JSX attribute values, and regex literals masked; literal-only lists of three or more numbers skipped; `hashCode`/`__hash__` and locale files skipped; block strings (Rust raw, Java text blocks, JS/Go backticks) masked |
| `class_structure_metrics` | A high NOC on a class built for extension is reuse | Chidamber and Kemerer (1994) | NOC alone does not flag an abstract class, an `ABC`/`Protocol` base, or a mixin |
| `collection_parameter_mutation` | In a typed language a mutation surprises the caller only through a declared collection on a non-private function | The declared parameter type states the contract (a `StringBuilder` or tree builder is an output sink) | Java and TypeScript require a collection-typed parameter, a non-private function, and no recursive call passing it on |
| `duplicate_import` | Repeated CommonJS member requires (`require(x).member`) are not duplicates | ESLint `no-duplicate-imports` and `import/no-duplicates` cover ES imports only; operator decision (2026-10-13) | Member requires excluded |
| `magic_literal_cluster` (Tier-2) | Byte layouts and encoding or hash constants are specification, not magic | The specifications themselves (UTF-8, Base64, SHA, IEEE-754, MS-DOS dates) | Deferred for Kotlin, C#, C, and C++ (Kotlin 0/12, C# 5/12) |
| `source_duplicate_detection` (Tier-2) | Per-type overloads, primitive specializations, mirror pairs, and DSL families repeat by design | The Java deferral (Task 15, part 1) | C qualifies (11/12); Kotlin (6/12), C# (7/12), and C++ (2/9) deferred |
| `collection_parameter_mutation` | A C# extension method's `this` receiver is the value it acts on | C# extension methods; a method's own instance is not a parameter | `this` receivers exempt, like `ref`/`out` |
| `section_divider_comment` | A comment border framed by `+` or a pipe draws a diagram | Byte-layout diagrams (okio's gzip header) | Borders starting with `+` or a pipe are not dividers |
| all detectors | Bison, yacc, and flex output is generated source | The generators' banners, as GitHub Linguist reads them | Banner-stamped C/C++ files take the Generated role |
| `naming_convention` | An underscore in a Kotlin function name violates the convention even as an internal-API marker | detekt `FunctionNaming`, ktlint `function-naming` | None |
| `class_structure_metrics` | A null object whose members are all empty measures its interface, not its design | Null Object pattern | None yet (one Serilog sample) |
| `god_module` | A method belongs to its type, not to the module, however the language attaches it | Operator decision (2026-10-18) | Counts free functions only: Rust `impl`/`trait` functions, C++ qualified definitions (`Block::Compare`), and members of a JavaScript class expression are methods |
| `escape_hatch_type_density` | `any` as a generic default, constraint, or placeholder is not an escape hatch | Operator decision (2026-10-18) | TypeScript `any` inside generic brackets or after `extends` is not counted |
| `architectural_layering` | A cycle that rests only on javadoc-only imports is not a dependency cycle | Operator decision (2026-10-18) | A Java type import named nowhere in code but its declaration creates no edge |
| `magic_literal_cluster` | HTTP status codes, character codes, bit masks, and ANSI codes are domain constants, so magic when bare | Operator decision (2026-10-18) | None |

A repository can also set its own scope: `.eaignore` files (gitignore syntax)
exclude paths such as a documentation site from the audit.

## Fixed Noise Mechanisms

Each fix has a semantic negative capability case, so the mechanism cannot
return silently.

| Detector | Mechanism | Fix |
|---|---|---|
| `unreachable_code` | Python `if/elif` without `else` treated as exhaustive (only the first `elif` was inspected) | Every arm must terminate, and the chain must end in `else` |
| `unreachable_code` | `raise` followed by a generator-marker `yield`; `#[cfg(..)] return` pairs | Neither is a terminator |
| `unreachable_code` | C/C++ `case` labels after `break` (yaml-cpp); C# `#pragma` lines after `return` (Polly) | `case`, `default`, and `goto` labels are entry points; preprocessor directives are trivia |
| `too_many_parameters` | C# extension receivers (`this PolicyBuilder builder`, Polly); typed Python `*args`/`**kwargs` | Receivers and variadics do not count |
| `naming_convention` | Constants with digits (`BASE64_VALUE_INVALID`, jackson-core) | Digits are allowed in UPPER_SNAKE_CASE |
| `too_many_parameters` | Lambda and function-pointer parameters attributed to the enclosing function (Polly, libuv) | Only the function's own signature counts |
| all (C++) | `class YAML_CPP_API Emitter {` parsed as a function definition spanning the class | Export macros are blanked before parsing |
| all | Generated files without a generated path (fastify `config-validator.js`, zlib `crc32.h`); `thirdparty/`, `snippets/`, `fuzz/`, `maven-tests/`, `profiling/` paths | Generated-file notices are read in every language; those paths get vendored, example, test, and benchmark roles |
| `magic_literal_cluster` | Prose lines of multi-line `/* .. */` comments (commons-lang) | Block comments are masked |
| all | Java package `tools.jackson` read as a tooling directory; `single_include/` amalgamations (nlohmann/json) duplicating their sources | Packages below a JVM source root are not tooling directories; `single_include/` and `amalgamation/` are generated |
| `too_many_returns` | Returns inside lambdas and closures credited to the enclosing function | They belong to the closure |
| `naming_convention` | Rust modules under `#![allow(non_camel_case_types)]`; okhttp's `-Name.kt` file prefix | Lint allows in the module or an ancestor are honoured; the `-` prefix is ignored |
| `unsafe_type_cast`, `kt_non_null_assertion` | KDoc and string text; the safe `as?` flagged instead of the hard `as` | Comments and strings masked; only hard casts flagged |
| `stale_link_reference`, `stale_section_anchor` | Escaped brackets, spaced destinations, URI schemes, percent-encoding, directory index pages; anchors from raw heading text | All parsed as CommonMark and GitHub render them |
| `unused_assignment` | Stores in a `try` body read by its handlers | Kept live for the handlers |
| all (C#) | The return type named as the function (`Task`, `ConvertResult`, generic `TResult`) | A declaration's `name` field is preferred |
| all (C++) | `NLOHMANN_JSON_NAMESPACE_BEGIN` making a namespace parse as a function named `namespace` | Namespace macros alone on a line are blanked |
| `duplicate_import`, `magic_literal_cluster` | Round six: tooling scripts and configs (`scripts/`, `docusaurus.config.ts`) reported, because index-backed detectors saw every code file | Both emit in production files only; every detector's declared emission roles are now enforced on its findings |
| all (C, C++) | Round five: an access-specifier macro (`JSON_PRIVATE_UNLESS_TESTED:`), a macro alone before `namespace`/`class` (`FMT_END_EXPORT`, `NLOHMANN_BASIC_JSON_TPL_DECLARATION`), and a block comment inside a `#define` body (libuv `/* ARGSUSED */`) misparsed as functions named `switch`, `m_object`, `namespace`, an include guard, or `elm` | Such macros and macro-body comments are blanked before parsing |
| complexity metrics (TS) | An arrow callback named after its lone parameter (`patches.forEach(patch => ..)`) | The `parameter` field is not a name |
| `cognitive_complexity` | Message breakdown not summing to the total | Nesting points, not nested-increment count, reported |
| `too_many_parameters` | Kotlin default values (`limit: Int = length`); `FnMut(Match)` in a Rust `where` clause; Kotlin `external` and C# `extern` bindings | Defaults and type-level parameters do not count; foreign bindings skipped |
| `magic_literal_cluster` | `0x10` and `16` counted as two literals | Literals keyed by value |
| `class_structure_metrics` | DIT 0 for classes whose header starts with an attribute or annotation, or spans lines (zod `ZodObject<..>`) | The full header is read; `extends` matched outside type parameters |
| `class_structure_metrics` | Children counted by base name across files: okhttp's private `State` credited with other files' `State` implementors; each C# `partial class Policy` file credited with all 24 subclasses | A base resolves to a class in the same file, else to the only class of that name |
| `excessive_nesting_depth` | A nested function's depth including the loops around its definition | Depth is per function |
| `intermediate_collection_materialization` | `filter(..).length` used as a count | Only emptiness tests are flagged |
| `section_divider_comment` | ASCII number-line diagrams (`<--- delta --->`, `--[--+--]--`) | Diagram characters disqualify a divider |
| all | `eslint.config.js`, `docusaurus.config.ts`, `build.gradle.kts`, `Makefile.js`, Gradle `build-logic/`; `test-resolution.ts` | Tooling and test roles |
| `excessive_nesting_depth` | Round seven: a `catch` one level below its `try` (Dapper `QueryMultipleAsync`); Rust's `?` operator, which parses as a `try_expression`, nesting its operand | A handler sits at its `try`'s depth; `?` nests nothing |
| `god_module` | Round seven: methods counted as top-level definitions (Rust `impl` functions, C++ `Type::method` definitions, JavaScript class-expression members and prototype getters) | Free functions only |
| `escape_hatch_type_density` | Round seven: `any` placeholders of variadic generics counted (hono `helper/factory`, trpc `useQueries`, immer `types-internal`) | `any` inside generic brackets or after `extends` is not counted |
| `architectural_layering` | Round seven: module cycles through Java imports kept only for javadoc `{@link}` (gson `FormattingStyle` → `JsonWriter`) | An import named nowhere in code is no edge |
| `too_many_statements` (Kotlin) | Round seven: comments inside a block counted as statements (okhttp `verifyHostname` 57, of which 31 comments; okio `commonWrite`) | Comments are not statements |
| complexity metrics (Kotlin, Go) | Round seven, from the strict recount: Kotlin trailing lambdas (`withLock { .. }`, `use { .. }`) and Go `func` literals folded into the enclosing function's cognitive complexity, cyclomatic count, and nesting depth, though NPath and Halstead already skipped them | Scored as their own functions, as documented |
| `constant_condition` | `for (; true; i++)` infinite-loop idiom | Treated like `while (true)` |
| `duplicate_import` | TS `import type` beside value imports; namespace (`* as`) beside named imports | Grouped by import form; unmergeable forms are not duplicates |
| `duplicate_import` | Python `TYPE_CHECKING` imports beside a runtime `__main__` import | A `TYPE_CHECKING` import is type-only; runtime and typing imports are grouped apart |
| `go_repeated_string_literal` | Quotes inside raw-string shell templates and struct tags; backticks in comments and rune literals | Raw strings, comments, and runes are skipped |
| `stale_link_reference` | Rustdoc intra-doc paths, template placeholders, link titles, links shown as code, extensionless doc-site links, slug-routed pages | Not treated as file paths; `.md`/`.mdx` resolution; fences inside list items |
| `stale_section_anchor` | GitHub slugs (`3.7` → `37`, `/` dropped), `{#id}` heading ids, HTML `id`/`name` anchors | All recognised as anchors |
| `section_divider_comment` | Prose with `--flag` or snake_case; Rust `#[attr]` and `quote!` `#var` lines | A divider is a run of at least three identical rule characters; `#` starts a comment only in hash-comment languages |
| `collection_parameter_mutation` | `options.update(..)` whose result is used (a copying API) | A consumed result of Python's `None`-returning mutators is not a mutation |
| `architectural_layering` | Self-edges from `pub use self::{..}` or a same-named external crate | Self-imports are not graph edges |
| `nested_quadratic_iteration` | `range(..)` taken as a collection named `range` | `range` has no collection identity |
| `orphan_symbol` | Trait-impl associated types; `#[global_allocator]` statics | Implicitly used items are exempt |
| `dead_code` | Functions marked `#[allow(dead_code)]` | Suppression honoured |
| `console_output_in_production` | `bin/`, `scripts/`, `sandbox/`, `examples/`, fixtures, gulpfiles | Treated as tooling paths |
| `too_many_parameters` | JS `function (..)` callbacks and TS function-type parameters credited to the enclosing function | Only real function symbols own parameters |
| `halstead_information_density` | Large, low-difficulty data tables, JSX, and message maps | Volume alone needs difficulty ≥ 15; Julia `operator` leaves are now counted |
| `class_structure_metrics` | Method-less leaves of exception taxonomies flagged on depth alone | Depth alone does not flag an empty leaf |
| `magic_literal_cluster` | Rust `#[test]` functions in `src/`; keyword-parameter defaults | Test functions and signature lines are skipped |
| complexity metrics | Rust `#[test]`/`#[cfg(test)]` functions in `src/` | Skipped |
| `naming_convention` | Java `serialVersionUID` | Exempt (a required name) |
| `cognitive_complexity` | Per-arm switch increments, `else if` nesting, per-operator booleans, callbacks folded into the outer function, emission at 15 | Sonar specification; callbacks scored separately; fires above 15 |
| `npath_complexity` | Guard-clause chains multiplied; brace-less branches and Java `else if` dropped; `catch` counted | Exits add; PMD's treatment of jumps, loops, and `try` |
| `too_many_parameters` | Python `@overload` stubs | Skipped; the implementation is measured |
| `assert_in_production` | Rust `assert!`; Python type-narrowing asserts | Python only; narrowing exempt |
| `function_local_import` | Guarded optional and `TYPE_CHECKING` imports; reason comments in other wording | Exempt |
| `identifier_stutter` | Local variables repeating function words | Public items qualified by their module |
| `out_parameter` | Parameter rebinding reported as an out-parameter | Writes through the parameter only |
| `go_errcheck` | Returned or assigned errors; `_examples/` code | Result use and `_`-prefixed roles recognised |
| `blocking_io_in_async_context` | One-shot tooling scripts | Tooling role excluded |
| `cognitive_complexity` | `return name;` scored as a labeled jump; recursion per call and missed in Java/Python | Only labeled `break`/`continue` count; recursion once per function |
| `npath_complexity` | Conditions as single paths; labeled jumps closed at the innermost loop; case labels counted per group | PMD's outcome, label, and case-label rules |
| `magic_literal_cluster` | Digits in Python docstrings | Triple-quoted strings masked |
| `repeated_expensive_initialization` | Constructors built from the loop variable | Loop-variant arguments are not reusable state |
| `source_duplicate_detection` | Explicit keyword APIs, forwarding calls, data rows, and sync/async twins | Not tokenized or not reported |
| `naming_convention` | Mutable `static final` fields | Google Java Style exemption |
| `duplicate_import` | CommonJS member requires (express `require('./utils').methods`) | Not grouped as duplicates |
| `source_duplicate_detection` | Parallel match arms and attribute-assignment rows (`self.x = x`) that differ in every binding | Clones needing more parameters than the `too_many_parameters` budget are not reported; declared locals do not count |
| `package_coupling_metrics` | Degenerate one-edge package graphs (Ca+Ce = 1), where instability is forced to 0 or 1 | Fewer than two coupling edges fail closed |
| `repeated_format_allocation`, `loop_invariant_computation` | Format calls in constant `switch` cases over a map's keys (cobra), which run once per loop | Calls keyed on a map loop's key are exempt |
| all Rust detectors | A module declared `#[cfg(test)] mod testutil;` classified as production in its own file | The module file and its submodules take the Test role |
| eight gating detectors | Round eight: tooling scripts (`tools/`, `scripts/`, docs scripts) reported by `excessive_nesting_depth`, `empty_catch_block`, `exit_in_production`, `shell_execution`, `hardcoded_credential`, `identifier_tersity`, `resource_not_closed`, and `nested_quadratic_iteration` | They emit in production files only, as the verdict standard scopes them |
| all detectors | A repository-root `build/` (hono release scripts) and `integration/` (fastify integration tests) classified as production | Root-level only: tooling and test roles; nested `build` or `integration` packages stay production |
| all detectors | A repository-root `support/` (fmt's release script, generator, vendored docopt, docs plugin) classified as production | Root-level only: tooling role; a nested `Support` directory (LLVM `lib/Support`) stays code |
| all detectors | A repository-root `extra/` (uptime-kuma release and maintenance scripts) or `hack/` classified as production; `console_output_in_production` reading its own tooling paths instead of the role | Root-level only: tooling role; console output skips every tooling-role file |
| `sql_injection` | SQL formatted from identifiers it cannot parameterize (dependency-track's v4 migrator schema and table names, Dapper.Rainbow `TableName`) and a literal FreeMarker macro prefix | Name-position values and literal-bound locals are not injection: all 17 corpus findings |
| `unsafe_deserialization` | `yaml.load(.., Loader=yaml.SafeLoader)` (apprise) | Safe loaders exempt (Bandit B506) |
| `random_for_security` | Test-data generators (hoppscotch predefined variables, zod and eslint tooling), retry jitter, cache busting, multipart boundaries, request IDs | Reported only when a bound name or the enclosing function's name marks a security purpose: 81 → 9 corpus findings |
| `go_errcheck` | Writes to a `bytes.Buffer` or `strings.Builder` (errcheck's default exclusions); a method the file declares without results (chi `LogEntry.Write`) | A receiver declared with those types, and result-less methods, are skipped |
| `out_parameter` | `@Override` methods whose supertype fixes an output buffer (`Reader.read(char[] cbuf, ..)`) | Overrides exempt |
| `hardcoded_credential` | Property-key names (`"java.net.socks.password"`) and character sets with no letter or digit (okhttp `PASSWORD_ENCODE_SET`) | Not credential values |
| `hardcoded_credential` | Literals restating the target's name (hoppscotch `ACCESS_TOKEN_NOT_FOUND = 'access_token/access_token_not_found'`, `CLIENT_CREDENTIALS: "clientCredentials"`, dependency-track `SECRET_REF = "x-secret-ref"`), Fetch `credentials: "include"`, help text and SQL or regex templates under credential names | Not credential values: 12 of 13 corpus findings; a PostHog project key remains |
| `repeated_format_allocation` | Error messages formatted in a branch before `throw` or a reporting call | Only calls that run on every iteration are reported |
| `loop_invariant_regex_compilation` | A compile already cached behind `if (!re)` (zod) | Lazily cached compiles are exempt |
| `debugger_in_production` | A trap followed by `abort()` (libuv's fatal-error path) | A deliberate fatal breakpoint |
| `test_without_assertion` | Round nine: TypeScript type tests checked by the compiler (hono `type verify = Expect<Equal<..>>`, immer `@ts-expect-error`, `isType.equal<..>()`) | Type-level constructs count as checks; TypeScript tests or files named for types or inference are exempt |
| `test_without_assertion` | Rust turbofish calls (`assert_send::<T>()`, axum, hyper) read with no callee name, a crate's own checking macro (`test_parse_error!`), and `unwrap()` inside a macro's arguments | Generic callees resolve; non-formatting macros and unwrapping macro arguments count |
| `test_without_assertion` | Compile-time checks `let _: T = ..` and `var _ T = ..` (axum, cobra) | Typed discarded bindings count |
| `test_without_assertion` | Formatter input files under `tests/data/cases/` (black) read as pytest tests | Python tests only in files pytest collects (`test_*.py`, `*_test.py`) |
| `test_without_assertion` | A Mocha `done` passed inside an object (`{ end: done }`, express) and a subtest given as a method value (`t.Run(name, tc.test)`, cobra) | Any non-passive use of the test context counts; a passed asserting function counts |
| `test_without_assertion` | Playwright waits (`page.waitForSelector`) that throw when the element never appears (trpc) | `waitFor*`, `getBy*`, and `findBy*` count as checks |
| `test_without_assertion` | Constructor-exists tests (`testConstructor`, commons-lang), placeholders (`testDummy`), and "should not exit" smoke tests (eslint) | Exempt by name |
| `tautological_assertion` | `assertEquals(token, token)` reflexivity tests of a class's own `equals` (commons-lang, jackson) | Self-comparisons count only under an equality the operand cannot redefine |
| `tautological_assertion` | A `vi.spyOn` of a real object asserted through that object (zod), which checks the spy intercepts dispatch | `spyOn` targets are not mock stubs |
| `catastrophic_backtracking_regex` | Round ten: a repeated alternation whose alternative is a delimited group (`(?:{[\d,]+}|[^}])+`, hono routers) read as ambiguous because the group contains a repetition | Only parts that two consecutive matches could also form (an unbounded repetition, or a group ambiguous itself) count |
| `unused_declared_dependency` | Round eleven: private npm packages (trpc's workspace root and `www/` site, immer's and zod's documentation sites) whose dependencies are framework peers, CLIs, and MDX-only imports | A `"private": true` package declares nothing |
| `unused_declared_dependency` | `use {serde_json as json}` inside a `use` group (ripgrep's printer) | A crate named inside a `use` group is used |
| `discarded_fallible_result` | `let _ = writer.write_all(..)` into a `BytesMut` writer (axum SSE) | Writes into a buffer the function binds in memory are infallible |
| `discarded_fallible_result` | `let _ = self.flush()` under a comment saying errors are ignored on exit (fd) | A justifying comment on the line or the line above exempts the discard |
| `discarded_fallible_result` | A floating call to an `async` function whose body is one `try`/`catch` (hono's SSE `run`), and a `Promise.resolve().then` microtask that throws on purpose (hono CSS) | Self-handling `async` functions and microtask scheduling are not floating failures |
| `network_call_without_timeout` | Round twelve (application slice): `axios(config)` read as `axios(url)` with no config (Hoppscotch kernel relay, whose config carries the request's timeout) | A bare `axios(x)` takes `x` as the config unless it is a URL string |
| `network_call_without_timeout` | Push examples under `extra/push-examples/` (Uptime Kuma) classified as production | A `*-examples` directory holds examples |

Two true positives were lost as accepted trade-offs: a `Tree.add` call whose
result is passed on (rich), and one Halstead state handler with difficulty
14.1 (jsoup).

## Open Noise Mechanisms

These are not fixed. They drove the Task 8 profile decisions (see
[Precision Gating](./detector-promotion.md#precision-gating)).

- **`unhandled_termination_in_production` (below the bar; now `full`).**
  Every Rust sample was an idiomatic `unreachable!()` for an impossible state
  or a documented programmer-error panic. Panics on recoverable errors would
  be the real target; the sample holds none.
- **`source_duplicate_detection` (below the bar; now `full`).** Keyword
  `else if` chains whose branches differ, builders over different APIs
  (PCRE2 and Rust regex), and cross-crate copies kept private on purpose.
  Whether extracting them is worth it is a judgment the token matcher cannot
  make. Explicit keyword APIs, sync/async twins, and over-parameterized clones
  are fixed (see above).
- **`magic_literal_cluster` (78%).** Demo and test-utility code. Domain
  constants (ANSI codes, bit masks) remain true positives.
- **`console_output_in_production` (56%).** Residual site-build files
  (`www/`, root `.mjs` scripts). Shipped-library warnings are true positives.
- **Codegen scripts outside tooling paths.** trpc's
  `www/src/components/*/script.ts` are one-shot code generators, but their
  path does not say so; `blocking_io_in_async_context` still reports them.
- **`unsafe_type_cast` (86%, Kotlin).** Casts guarded by a
  `this::class != other::class` check in `equals`, and casts in Gradle build
  scripts.
- **Small samples, all false positives.** `per_item_external_io` (retry, broadcast,
  batch, and echo loops), `random_for_security` (demo use),
  `sibling_call_redundancy` (shared helper callees), `identifier_tersity`
  (domain names such as `r1, g1, b1`).
- **Residual small-sample false positives (2026-10-18).** These moved their
  detector to `full` or scoped out a language: `insecure_cryptography`
  (SHA-1 where collision resistance is not relied on: Flask's HMAC signer,
  httpx's digest-auth nonce), `sql_injection` (Dapper.Rainbow table names
  from type metadata), `nested_quadratic_iteration` (an inner scan bounded
  by its segment; pairwise checks over small lists), `resource_not_closed`
  (fmt's release script under `support/`), `duplicate_import` in TypeScript
  (zod's module-resolution test package), `exit_in_production` in Python
  (vendored docopt's help exit), `unused_assignment` in C++ (yaml-cpp's
  `util/` and `contrib/`), and `stringly_typed_parameter` in Python and
  TypeScript (`mode` mirroring `open()`, a media-type value).
- **Found by the Semgrep comparison (2026-10-20).** `random_for_security`
  reports a cache-busting React state named `nonce` (trpc's og-image page).
  The study's other false positives are fixed (see the `sql_injection`,
  `unsafe_deserialization`, `random_for_security`, and `extra/` rows above).

## Comparison with Semgrep

**Provisional.** One model labelled these findings (`model:claude-opus-5-5`)
without corroboration, so the counts are not part of the precision results
above.

Semgrep 1.181.0 ran with the free registry rulesets `p/default`, `p/secrets`,
and the Python, JavaScript, TypeScript, Java, Go, Rust, C, C#, and Kotlin
packs (metrics off) over the 46 corpus checkouts: the 40 pinned repositories
and six applications (apprise, dependency-track, hoppscotch, lychee,
miniflux, uptime-kuma). It reported 1,768 findings, 1,767 of them security
rules; 699 are GitHub Actions and 173 HTML rules, which no Engineering Auditor
detector covers. Engineering Auditor reported 14,187 findings, of which
Semgrep's free rules have no counterpart for the structural, algorithmic,
systems, optimization, and test-quality detectors.

Six Engineering Auditor detectors overlap with Semgrep rule families.
Findings within three lines in the same file count as agreement. Labels
follow the verdict standard above, applied to each tool's own claim; a
Semgrep finding outside production code (88 of 95 deserialization and 89 of
97 credential findings are in tests and examples) is out of scope and not
counted.

| Detector | Agreed (TP / FP) | Engineering Auditor only (TP / FP) | Semgrep only, production (TP / FP) |
|---|---|---|---|
| `eval_in_production` | 5 / 0 | 2 / 0 (hoppscotch's script sandbox) | none |
| `unsafe_deserialization` | 4 / 0 | 0 / 1 (`SafeLoader`) | 0 / 6 (pickle dumps, own in-memory data) |
| `hardcoded_credential` | none | 1 / 0 (a PostHog key in backend code) | 1 / 5 (a logged access token; a dummy bcrypt hash, a protocol-mandated MD5) |
| `shell_execution` | 1 / 6 (release scripts) | 3 / 10 (fmt's `std::system(format(..))`, two server execs; release scripts) | 0 / 8 (literal commands, an Action's own inputs) |
| `sql_injection` | none | 0 / 17 (identifiers) | 0 / 8, 1 unclear (Dapper internals, a configured-query monitor) |
| `random_for_security` | none | 2 / 17, 1 unclear, of 81 sampled 20 (zlib's PKWARE header, a digest-auth nonce) | none in production |

The table records the scan the labels were made on. Since then, the
`sql_injection` false positives, the `SafeLoader` call, the release-script
`shell_execution` findings, and all but 9 `random_for_security` findings are
no longer reported (see Fixed Noise Mechanisms); the labelled true positives
are all still reported.

On the overlap, neither tool is precise: excluding `random_for_security`,
Engineering Auditor's labelled findings are 16 true positives of 50, and
Semgrep's 11 of 44. Engineering Auditor's unique true positives (8, among
them a format-string shell command in fmt and a weak random source in
zlib's encryption) outnumber Semgrep's (1, an access token written to a
debug log, a check Engineering Auditor lacks). Semgrep's value here is
breadth outside source code (CI configuration, HTML) and fixtures;
Engineering Auditor's is the catalog beyond security. Each checkout was
scanned with:

```sh
semgrep scan --metrics=off --disable-version-check --timeout 30 --json \
  --config p/default --config p/secrets --config p/python \
  --config p/javascript --config p/typescript --config p/java \
  --config p/golang --config p/rust --config p/c --config p/csharp \
  --config p/kotlin --output <repo>.json <checkout>
```

## Known Limitations

- **Finding-ID ordinals.** Same-named functions in one file (Java `process`
  overrides) are disambiguated by ordinal, so removing one finding can shift a
  sibling's ID. `labels.py sample` now resets a label to pending when its
  finding's location changes, so it is labelled again at the new site; a
  shift that keeps the same line goes unnoticed.
- **Preprocessor conditionals inside declarations.** An `#if` splitting a
  C++ constructor's member-initializer list (nlohmann `basic_json` under
  `JSON_DIAGNOSTIC_POSITIONS`) still derails the parse; one finding names a
  function `type` at a `switch` line.
- **Language scope.** Precision is per detector across languages. A detector
  that is noisy in one language and clean in another needs a language-scoped
  decision, as `assert_in_production` (now Python only) did.

## Reproduce

```sh
cargo build --release -p auditor-cli
python3 tooling/ecosystem/run.py scan --label current
python3 tooling/ecosystem/labels.py precision --scan current
# Corroboration (tools install under target/oracles/; the free model runs via opencode):
python3 tooling/ecosystem/oracles.py provision
python3 tooling/ecosystem/oracles.py run --scan current --write
python3 tooling/ecosystem/model_review.py run --scan current \
  --model opencode/step-5-preview-free --workers 3 --out second.jsonl
python3 tooling/ecosystem/labels.py checks second.jsonl --source model:opencode/step-5-preview-free
# Human confirmation, per detector, after reviewing its pre-labels:
python3 tooling/ecosystem/labels.py context --scan current --status pre_label --detector <id>
python3 tooling/ecosystem/labels.py confirm --labeller human:<name> --detector <id>
```

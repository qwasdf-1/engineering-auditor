# Known Gaps

What Engineering Auditor does not do, or does only partly, as of 1.1.0.

## Analysis Depth

- Analysis is syntax-based. There is no type checking, whole-program
  dataflow, alias analysis, or general control-flow graph, so detectors that
  would need them stay silent rather than guess. Your language's own linters
  (Clippy, Ruff, ESLint, golangci-lint, and others) use type information and
  catch defects Engineering Auditor cannot.
- Algorithmic and systems detectors mostly need the expensive call to appear
  directly in the loop or async function. Only external-I/O and blocking-I/O
  effects are followed through calls into repository functions; for sorts,
  queries, regex compilation, and artifact rewrites, a helper that does the
  work hides it. 9 of 29 seeded helper cases are caught
  ([`eval/recall/`](../eval/recall/README.md)). Calls on values whose type
  is unknown are never followed.
- Reflection, plugin loading, and entry points defined outside the scanned
  code are not resolved, so reachability findings such as `dead_code` and
  `orphan_symbol` exclude what they cannot prove.
- Support differs by language.
  [`reference/language-support.md`](reference/language-support.md) is the
  authority for each detector and language. Ruby, Julia, and Shell get
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

- Scans run on a single thread. A repository of a few hundred thousand lines
  takes 10 to 20 seconds.

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

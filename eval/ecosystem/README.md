# Ecosystem Corpus

`manifest.json` pins permissively licensed open-source repositories, at least
two per Tier-1 and Tier-2 language, at exact release-tag commits. The corpus measures
detector precision on natural code. Checkouts are fetched on demand and never
vendored. The schema is `contracts/ecosystem-corpus.schema.json` (internal).

```sh
cargo build --release -p auditor-cli
python3 tooling/ecosystem/run.py fetch                 # fetch pinned commits
python3 tooling/ecosystem/run.py scan --label current  # per-detector summary
python3 tooling/ecosystem/run.py diff <label-a> <label-b>
python3 tooling/ecosystem/run.py compare --rev-a origin/main --rev-b HEAD
```

The runner writes checkouts, builds, and results under `target/ecosystem/`, or
under `$EA_ECOSYSTEM_CACHE` if it is set.

- Each scan writes `results/<label>/summary.{json,md}`, with findings per
  detector, per file language, and per repository, plus each repository's
  report.
- Diffs classify findings as new, resolved, re-keyed, or unchanged. Re-keyed
  means the same detector, path, and line under a different ID. Both runs scan
  identical commits, so a re-key is an identity change, not a finding change.
- Checkouts are depth 1, so history-dependent detectors stay silent and scans
  are reproducible.

## Application slice

The library corpus has almost no outbound network code, so some detectors
cannot be measured on it. `applications` in `manifest.json` pins one
permissively licensed application per Tier-1 language (lychee, apprise,
miniflux, Dependency-Track, Uptime Kuma, Hoppscotch), each naming the
detectors it measures (today `network_call_without_timeout`).

```sh
python3 tooling/ecosystem/run.py scan --slice applications --label apps-current
python3 tooling/ecosystem/labels.py precision --scan current apps-current
```

An application report keeps only its named detectors' findings, so the slice
never moves another detector's standing. Labels from both slices share each
detector's label file; `precision` takes both scan labels so neither slice's
labels read as stale.

## Syntax coverage

A scan's `summary.json` counts, per language, the files whose syntax tree
has errors: `recovered` (structural detectors read them without the
declarations holding the errors) and `unparsed` (they skip them).
`syntax.json` records those counts for each slice, and
`run.py syntax-check <label>` fails when any language's unparsed count rises
above it; the Ecosystem workflow runs it on the new revision. After a change
that parses more files, record the improvement:

```sh
python3 tooling/ecosystem/run.py syntax-check <label> --write
python3 tooling/ecosystem/run.py syntax-check <label> --slice applications --write
```

## Precision labels

`labels/<detector>.json` holds adjudicated samples keyed by finding-ID v3
(schema `contracts/ecosystem-labels.schema.json`, internal). Each label is
`true_positive`, `false_positive`, or `unclear`, with a one-line reason.

```sh
python3 tooling/ecosystem/labels.py sample --scan current     # add pending samples
python3 tooling/ecosystem/labels.py context --scan current > pending.jsonl
python3 tooling/ecosystem/labels.py import verdicts.jsonl --labeller model:<id>
python3 tooling/ecosystem/labels.py context --scan current --status pre_label
python3 tooling/ecosystem/labels.py import reviewed.jsonl --labeller human:<name>
python3 tooling/ecosystem/labels.py confirm --labeller human:<name> --detector <id>
python3 tooling/ecosystem/labels.py precision --scan current
```

- **Sampling.** Each detector and file language holds at most 30 samples,
  drawn round-robin across repositories in seeded-hash order (`--seed`,
  default `ea-v1`). Resampling a newer scan tops buckets up and never redraws
  a labelled finding; stale labels do not count toward the cap.
- **Verdicts.** `import` reads JSON Lines of `{id, verdict, reason}`. A
  `model:` labeller writes `pre_label` verdicts, which never replace a final
  verdict. Only a `human:` labeller writes `final`. `confirm` accepts
  reviewed pre-labels as final human verdicts, for whole detectors or
  selected `--ids`.
- **Corroboration.** A pre-label becomes `corroborated` when an independent
  check agrees with it and none contradicts it; a contradicted pre-label is
  disputed and waits for review. `checks` records checks from JSON Lines:
  `{id, claim, detail}` from an oracle (`--source oracle:<tool>@<version>`),
  or `{id, verdict, reason}` from a second model (`--source model:<id>`,
  which must differ from the labeller). A run replaces that source's earlier
  checks; `--detector` limits the run, and the replacement, to those
  detectors' labels, so a check of one detector keeps the source's checks
  of the others. An oracle that confirms the claim
  agrees with a true positive; one that refutes it agrees with a false
  positive. A confirmed claim is neutral toward a false positive, because
  linters do not judge scope or idiom.
- **Precision.** Precision is TP / (TP + FP) over final and corroborated
  labels; `unclear` is reported separately. The standing is a sound bound
  over the remaining unverified pre-labels: `meets_bar` only if precision
  stays at 90% or more even were every unverified label a false positive
  (with at least 10 verified labels), `below_bar` only if it stays under 90%
  even were every one a true positive, `unverified` in between, and
  `insufficient_sample` under 10 labels. With `--scan`, labels whose finding
  is gone from that scan are counted as stale and excluded.

## Oracles and second-model review

```sh
python3 tooling/ecosystem/oracles.py provision                 # once; needs network
python3 tooling/ecosystem/oracles.py run --scan current --write
python3 tooling/ecosystem/model_review.py run --scan current \
    --model opencode/<free-model> --out second.jsonl
python3 tooling/ecosystem/labels.py checks second.jsonl --source model:<free-model>
```

- **Oracles.** `oracles.py` runs pinned reference tools (lizard, ruff,
  complexipy, gocognit, errcheck, goconst, ESLint with SonarJS, jscpd,
  rust-code-analysis, PMD, and a nonblank-line recount) on the files that hold
  labelled findings, with inline suppressions ignored. `provision` installs
  them under `target/oracles/` (or `$EA_ORACLE_ROOT`). Each rule maps one
  detector and language to one tool and confirms or refutes the finding's
  factual claim; a tool whose silence is not evidence (goconst, jscpd, ESLint
  `max-depth`) only confirms.
- **Second model.** `model_review.py` sends blind bundles (finding, rationale,
  excerpt; never the first verdict or the checks) through `opencode run` in an
  empty directory where every tool call is auto-rejected. Answers are cached
  per batch under `target/ecosystem/model-review/`, so an interrupted run
  resumes.

Current results: [`docs/reference/detector-precision.md`](../../docs/reference/detector-precision.md).

The opt-in `Ecosystem` GitHub workflow runs `compare` against a chosen base
revision and uploads the summaries and diff.

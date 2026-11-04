# User Guide

Engineering Auditor (`auditor`) scans a repository and reports engineering
quality problems: correctness and security defects, structural complexity,
algorithmic cost, systems inefficiencies, and optimization opportunities.
Every finding carries typed evidence and a stable ID, so you can accept
today's findings in a baseline and fail CI only on new ones.

This guide covers installing the tool, running a first scan, adopting a
baseline and gate, wiring it into CI, uploading SARIF, configuring
detectors, and the status of the experimental AI review layer.

## Install

### From a release

Each release publishes archives for Linux x86_64 and macOS (Apple silicon
and Intel), plus a `SHA256SUMS` file. Download the archive for your platform
and the checksum file from the release page, then:

```sh
sha256sum -c SHA256SUMS --ignore-missing   # macOS: shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf auditor-1.5.0-x86_64-unknown-linux-gnu.tar.gz
sudo install auditor-1.5.0-x86_64-unknown-linux-gnu/auditor /usr/local/bin/
auditor version
```

The archive also contains `review/routes-example.json`, a starting point for
the provider routes of the AI review (see [AI review](#ai-review)).

### From source

You need Rust 1.86 or newer.

```sh
git clone <repository-url> engineering-auditor
cd engineering-auditor
cargo install --locked --path crates/auditor-cli
```

This installs the `auditor` binary into `~/.cargo/bin`.

## Your first scan

From the root of the repository you want to audit:

```sh
auditor scan --repo-root . --format human
```

The human report lists each finding with its location, severity, evidence,
and a recommendation. A scan never modifies your files.

Two profiles control which detectors run:

- `--profile gating` (the default) runs the detectors precise enough to fail a
  build on.
- `--profile full` adds detectors that are useful for review but noisier, such
  as broader optimization opportunities.

For machine-readable output, write report v5 JSON to a file. Add `--quiet` so
the report goes only to the file, with a one-line summary on stderr:

```sh
auditor scan --repo-root . --profile gating --format json --output report.json --quiet
```

Other useful options:

- `--path <relative>` limits the scan to part of the repository.
- `--summary-only` prints only the summary of the human report.
- `auditor digest --report report.json` renders a saved JSON report as the
  human report.
- `auditor catalog --profile full --format json` lists every detector, its
  layer, and its language support.

To keep paths out of the audit entirely (vendored code, a generated
documentation site), add a `.eaignore` file with gitignore syntax. It can live
in any directory and applies below it.

### Supported languages

| Tier | Languages | What runs |
|---|---|---|
| 1 | Rust, Python, JavaScript, TypeScript, Go, Java | The full catalog wherever a detector applies to the language. |
| 2 | Kotlin, C#, C, C++ | Correctness and structural detectors at the Tier-1 standard; algorithmic and systems detectors where documented. |
| 3 | Ruby, Julia, Shell | Structural metrics only: complexity, Halstead density, and file size. |

[`docs/reference/language-support.md`](reference/language-support.md) lists
the support mode of every detector for every language.

## Baselines and the gate

Most repositories have findings on day one. A baseline records the findings
you accept so the gate fails only on new ones.

1. Scan with the gating profile and create a baseline. The reason is required
   and is stored with every entry, so reviewers know why the debt was
   accepted:

   ```sh
   auditor scan --repo-root . --profile gating --format json --output report.json --quiet
   auditor baseline create --report report.json \
     --reason "Existing findings accepted when the gate was adopted." > auditor-baseline.json
   ```

2. Commit `auditor-baseline.json`.

3. Run the gate. It scans the repository itself and compares the result with
   the baseline:

   ```sh
   auditor gate --repo-root . --baseline auditor-baseline.json
   ```

The gate prints a JSON result and exits with:

| Exit code | Meaning |
|---|---|
| `0` | Passed: no new findings. |
| `1` | Failed: at least one new finding, listed in `new_findings` (with `reason: new_findings_introduced`). |
| `2` | Usage, input, or runtime error (message on stderr). |

Findings keep their IDs across unrelated edits: IDs hash the enclosing
declaration rather than line numbers. The gate also re-pairs a baselined finding
whose symbol was renamed or whose file moved, as long as the match is
unambiguous.

When findings are fixed or code is refactored, refresh the baseline without
accepting anything new:

```sh
auditor scan --repo-root . --profile gating --format json --output report.json --quiet
auditor baseline rekey --baseline auditor-baseline.json --report report.json \
  --prune-resolved > auditor-baseline.next.json
mv auditor-baseline.next.json auditor-baseline.json
```

`rekey` keeps every review reason, drops resolved entries with
`--prune-resolved`, and never adds a new finding. To accept a new finding,
create a fresh baseline from a report and review the diff.

Instead of a baseline, `auditor gate --repo-root . --max-findings <n>` enforces
an absolute budget.

## CI integration

A GitHub Actions job that installs a release, gates on the committed
baseline, and uploads SARIF so findings appear in code scanning:

```yaml
name: Engineering Auditor

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read
  security-events: write

jobs:
  audit:
    runs-on: ubuntu-latest
    env:
      AUDITOR_VERSION: 1.5.0
      AUDITOR_TARGET: x86_64-unknown-linux-gnu
    steps:
      - uses: actions/checkout@v4

      - name: Install auditor
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release download "v$AUDITOR_VERSION" --repo <owner>/engineering-auditor \
            --pattern "auditor-$AUDITOR_VERSION-$AUDITOR_TARGET.tar.gz" --pattern SHA256SUMS
          sha256sum -c SHA256SUMS --ignore-missing
          tar -xzf "auditor-$AUDITOR_VERSION-$AUDITOR_TARGET.tar.gz"
          echo "$PWD/auditor-$AUDITOR_VERSION-$AUDITOR_TARGET" >> "$GITHUB_PATH"

      - name: Gate on new findings
        run: auditor gate --repo-root . --baseline auditor-baseline.json

      - name: SARIF report
        if: always()
        run: auditor scan --repo-root . --format sarif --output auditor.sarif --quiet

      - uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: auditor.sarif
          category: engineering-auditor
```

Pin `AUDITOR_VERSION`. A newer release can find issues an older one missed,
so upgrade deliberately and refresh the baseline in the same change (see
[Stability](#stability-and-upgrades)).

## SARIF

```sh
auditor scan --repo-root . --format sarif --output auditor.sarif
```

The output is SARIF 2.1.0. Each result's
`partialFingerprints.engineeringAuditor/v3` equals the finding ID, so code
scanning tracks a finding across commits the same way the baseline does. Any
SARIF consumer works; the CI example above uses GitHub's
`upload-sarif` action.

## Configuration

Put an `auditor.toml` at the repository root. Each `[detectors.<id>]` section
tunes one detector:

```toml
# Turn a detector off.
[detectors.repeated_sort_in_loop]
enabled = false

# Change a reported severity: advisory, low, medium, high, or critical.
[detectors.hardcoded_credential]
severity = critical

# Raise a size or complexity threshold.
[detectors.large_file]
threshold = 800
```

The stable keys are `enabled`, `threshold`, `severity`, `entropy_threshold`,
`min_length`, `max_length`, `ratio_threshold`, and `min_count`; which ones a
detector reads is documented in its layer reference under
[`docs/reference/`](reference/).

Architectural layering rules map files to named layers and deny dependencies
between them:

```toml
[layering.layer.domain]
glob = src/domain/**/*.rs

[layering.layer.app]
glob = src/app/**/*.rs

# Domain code must not depend on the application layer.
[layering.rule.domain]
deny = app
```

Path-scoped exclusions (experimental) drop the listed detectors' findings
under the listed path prefixes:

```toml
[exclusions.scripts]
paths = tooling/
detectors = console_output_in_production, exit_in_production
```

Prefer a baseline entry with a reason over an exclusion when the finding is
real but accepted; use an exclusion when a detector does not fit a directory
at all.

## Inline suppressions

To accept one finding where it stands, put a comment on its line, or alone on
the line above it, naming the detector and saying why:

```python
# auditor-ignore[shell_execution]: admin tool; cmd comes from a fixed table
subprocess.call(cmd, shell=True)
```

```go
defer f.Close() // auditor-ignore[go_errcheck]: read-only file
```

Several detectors can share one comment
(`auditor-ignore[shell_execution, eval_in_production]: ..`). The report lists
every suppressed finding with its reason under `suppressed_findings`, and the
human report counts them. A suppression keeps working when the code around
it moves, but unlike a baseline entry it lives in the source, next to the
code it excuses.

`invalid_suppression` (gating) reports a suppression with no reason, which
suppresses nothing; one naming an unknown detector; and one that no longer
matches a finding, so a stale suppression fails the gate instead of lingering.
Markers inside string literals and in Markdown or data files are ignored.

## AI review

Layer 6, the optional AI review, is being rebuilt as a reviewer that reads a
scanned repository through typed tools and cites every claim (see
[`docs/reference/layer6-llm-review.md`](reference/layer6-llm-review.md)).
While it is rebuilt, the review commands of earlier releases (`scan
--review`, `review run`, `review plan`, `review verify`, and the
disposition commands) are gone. These remain or are new:

- `auditor review provider discover --routes routes.json
  --allow-hosted-review` checks that the providers in a routes file answer.
  `review/routes-example.json` is a starting point for Mistral's API; the
  schema is `contracts/review-routes-v2.schema.json`, described in
  [`docs/reference/layer6-providers.md`](reference/layer6-providers.md).
- `auditor review transcript --input transcript.json` renders a recorded
  provider transcript as Markdown.
- `auditor review history --repo-root . --output history.json` summarizes
  the last year (`--window-days`) of the repository's history: commits,
  growth, and the files changed together.
- `auditor review check-patch --repo-root . --patch fix.diff --resolves
  EA-…` applies a patch to a scratch copy, re-scans it, and prints whether
  the cited findings are gone without new ones. With `--brief`, the brief's
  `test_command` must also pass. Your tree is never written.

Nothing is sent to a provider without the explicit consent flag.

## Stability and upgrades

The CLI commands and exit codes, report v5, finding-ID v3, baseline v2, the
gate result, SARIF output, and the `auditor.toml` keys above are stable: they
change incompatibly only in a major version. Detectors improve in every
release, so an upgrade can add findings; review them and refresh your
baseline in the same change. When a finding-ID version changes,
`auditor baseline migrate` carries an existing baseline across.
[`docs/reference/stability.md`](reference/stability.md) is the full policy.

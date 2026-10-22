# Engineering Auditor

Engineering Auditor (`auditor`) is a static analysis tool that scans a
repository in 13 languages and reports engineering-quality problems:
correctness and security defects, complexity and duplication, algorithmic
cost, and systems inefficiencies. It is one binary, needs no build of the
code it scans, and gives every finding a stable ID, so a team can accept
today's findings in a baseline and fail CI only on new ones.

New here? The [user guide](docs/guide.md) covers installing, a first scan,
baselines and the gate, CI and SARIF, configuration, and the optional AI
review.

## Why use it

- **A ratchet for code quality.** Finding IDs are tied to the enclosing
  declaration rather than to line numbers, so unrelated edits do not re-key
  them, and the baseline follows a finding across a renamed symbol or a
  moved file. `auditor gate` passes when nothing new appears.
- **One tool across languages.** Rust, Python, JavaScript, TypeScript, Go,
  Java, Kotlin, C#, C, C++, and structural metrics for Ruby, Julia, and
  Shell, with one report format and one baseline.
- **No build required.** Analysis is syntax-based, so it runs on code that
  does not compile, on a partial checkout, or on a repository whose
  toolchain you do not have installed.
- **Measured precision.** Detectors that fail a build are measured against a
  labeled corpus of 40 open-source repositories
  ([detector precision](docs/reference/detector-precision.md)); noisier
  detectors run only in the `full` profile.
- **Evidence for people and tools.** Each finding carries typed evidence and
  a recommendation. Output is JSON (report v5), SARIF 2.1.0, or a human
  report, and an MCP server exposes scans to coding agents.

## What it is not

Engineering Auditor complements your language's own linters; it does not
replace them. Clippy, Ruff, ESLint, golangci-lint, and their peers have far
more rules, use type information, and are faster. Most of what Engineering
Auditor reports on typical code is structural: complexity, size,
duplication, and naming. Its value is in applying those checks, plus a set
of correctness, algorithmic, and systems detectors, uniformly across
languages behind one stable baseline.

## Quick start

Download an archive for Linux x86_64 or macOS from the releases page, or
build from source with Rust 1.86 or newer:

```sh
cargo install --locked --path crates/auditor-cli
```

Then, from the repository you want to audit:

```sh
auditor scan --repo-root . --format human

# Accept today's findings, then fail only on new ones.
auditor scan --repo-root . --format json --output report.json --quiet
auditor baseline create --report report.json \
  --reason "Existing findings accepted when the gate was adopted." > auditor-baseline.json
auditor gate --repo-root . --baseline auditor-baseline.json
```

## What it checks

| Layer | Focus | Detectors (gating / full-only) |
|---|---|---|
| 1. Correctness | Insecure APIs, injection, swallowed errors, unsafe termination, dead code | 16 / 25 |
| 2. Structure | Complexity, size, duplication, naming, stale references, test quality | 22 / 16 |
| 3. Algorithms | Quadratic loops, repeated sorts and lookups in loops, N+1 queries | 4 / 9 |
| 4. Systems | Layering rules, coupling, blocking I/O in async code, missing timeouts and backoff | 6 / 7 |
| 5. Optimization | Preallocation, bulk operations, loop-invariant work, eager log formatting | 4 / 10 |

The default `gating` profile runs the 52 detectors precise enough to fail a
build; `--profile full` runs all 119.
`auditor catalog --profile full --format json` lists every detector, and
[language support](docs/reference/language-support.md) shows which detectors
run for each language.

## Performance

Scans run on a single thread. On a 16-core Linux machine, a 50,000-line
repository scans in about 1 to 3 seconds and a 540,000-line JavaScript
repository in about 14 seconds with the gating profile, using up to about
550 MB of memory.

## Experimental features

These are off by default and not covered by the stability policy.

- **AI review (Layer 6).** `auditor review run` sends a bounded,
  evidence-only package about selected findings to a language model you
  configure, and records its assessment and recommendations next to the
  deterministic findings, which it can never change. Nothing is sent without
  the explicit `--allow-hosted-review` flag. It has not yet passed its own
  qualification bar on the models tested so far
  ([known gaps](docs/known-gaps.md)); treat its output as advisory.
- **Graph analysis.** A build with `--features graph-analysis` adds seven
  detectors that consume an externally produced repository graph and
  co-edit journal (`--repo-graph`, `--co-edit-journal`).

## Documentation

- [User guide](docs/guide.md)
- [Stability policy](docs/reference/stability.md): what a version number
  promises.
- [Report contract](docs/reference/report-contract.md): report, baseline,
  gate, and SARIF formats.
- [Detector references](docs/reference/): one page per layer.
- [Architecture](docs/architecture.md) and the
  [documentation map](docs/README.md) for contributors.

## Development

```sh
cargo test
cargo run -p auditor-cli -- scan --repo-root . --format json
cargo run -p auditor-cli -- gate --repo-root . \
  --baseline config/auditor/gating-baseline-v1.json
cargo run -p auditor-cli -- donor check
```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full checklist. The
repository gates itself: CI fails on any new finding against
`config/auditor/gating-baseline-v1.json`. External tools used to measure
parity and precision ("donors") are described in
[`docs/donor-policy.md`](docs/donor-policy.md); none of them is a runtime
dependency.

## License

Apache License 2.0. See [`LICENSE`](LICENSE).

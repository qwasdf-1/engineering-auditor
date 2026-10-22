# Project Reference

A one-page summary of what Engineering Auditor is and where its parts live.
For using the tool, start with the [user guide](guide.md).

## What It Is

A Rust workspace that builds two binaries:

- `auditor`, the command-line scanner.
- `auditor-mcp`, an MCP server that exposes the same scan and review services
  to coding agents.

Scans are deterministic: the same repository and inputs give byte-identical
reports. An optional, experimental AI review (Layer 6) runs only on request
and can never change deterministic findings.

## Detectors

The default catalog has 124 detectors: 54 in the `gating` profile, precise
enough to fail a build, and 70 more in `full`. A build with
`--features graph-analysis` adds seven detectors that read an externally
produced repository graph and co-edit journal.

| Layer | Concern | Reference |
|---|---|---|
| L1 | Correctness and security | [`layer1-correctness.md`](reference/layer1-correctness.md) |
| L2 | Structure: complexity, size, duplication, naming | [`layer2-structural-engineering.md`](reference/layer2-structural-engineering.md) |
| L3 | Algorithmic cost | [`layer3-algorithmic-engineering.md`](reference/layer3-algorithmic-engineering.md) |
| L4 | Systems: I/O, async, caching, architecture | [`layer4-systems-engineering.md`](reference/layer4-systems-engineering.md) |
| L5 | Optimization opportunities | [`layer5-optimization-opportunities.md`](reference/layer5-optimization-opportunities.md) |
| L6 | Experimental AI review | [`layer6-llm-review.md`](reference/layer6-llm-review.md) |

Every detector declares its profile, the languages it supports and how
(semantic, lexical, partial, unsupported, or not applicable), and which file
roles it may report on. Support is set per detector, not implied by having a
parser for the language;
[`language-support.md`](reference/language-support.md) is generated from the
catalog. Grammar-backed languages: Rust, Python, JavaScript, TypeScript, Go,
Java, Kotlin, C#, C, C++, Ruby, Julia, and Shell. Markdown and structured data
are read by a few documentation detectors.

## Contracts

Machine-readable schemas live under `contracts/`;
[`report-contract.md`](reference/report-contract.md) describes them.

| Contract | Version | Stability |
|---|---|---|
| Report | v5 | Stable |
| Finding ID | v3 | Stable |
| Baseline | v2 | Stable |
| Gate result | 2 | Stable |
| SARIF | 2.1.0 | Stable |
| Evidence graph (inside the report) | v2 | Stable as part of the report |
| Repository map | v1 | Experimental |
| Engineering review | v2 | Experimental |

[`stability.md`](reference/stability.md) defines what each stability class
promises.

## Commands

| Command | Purpose |
|---|---|
| `scan` | Scan a repository; JSON, SARIF, or human output. `--review` adds an AI review. |
| `digest` | Render a saved JSON report as the human report. |
| `gate` | Compare a scan with a baseline, or with an absolute finding budget. |
| `baseline create`, `rekey`, `migrate` | Create and maintain baselines. |
| `catalog` | List detectors with their profiles and language support. |
| `review plan`, `inspect-case`, `run`, `verify`, `transcript`, `provider`, `disposition`, `gate` | Experimental AI review operations. |
| `donor check`, `donor sync` | Check the reference-tool manifest (contributors only). |
| `version` | Print the version. |

## Where To Look Next

- [`architecture.md`](architecture.md): crates, the scan pipeline, and the
  review pipeline.
- [`current-state.md`](current-state.md): what is implemented, by area.
- [`known-gaps.md`](known-gaps.md): what is not.
- [`donor-policy.md`](donor-policy.md): how external tools are used for
  parity and precision, and their licenses.

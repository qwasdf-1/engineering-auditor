# Engineering Auditor Documentation

## Using Engineering Auditor

| Page | Use |
|---|---|
| [`guide.md`](guide.md) | Install, scan, baselines and the gate, CI, SARIF, configuration, AI review. |
| [`reference/stability.md`](reference/stability.md) | Stable and experimental surfaces, and what a version number promises. |
| [`reference/report-contract.md`](reference/report-contract.md) | Report, baseline, gate, and SARIF formats. |
| [`reference/language-support.md`](reference/language-support.md) | Which detectors run for each language (generated). |
| [`reference/layer1-correctness.md`](reference/layer1-correctness.md) through [`layer5-optimization-opportunities.md`](reference/layer5-optimization-opportunities.md) | What each detector reports and how to tune it. |
| [`reference/detector-precision.md`](reference/detector-precision.md) | Measured precision on a 40-repository corpus. |
| [`known-gaps.md`](known-gaps.md) | Known limitations. |

## Experimental: AI review

- [`reference/layer6-llm-review.md`](reference/layer6-llm-review.md): how the
  review works and what it may and may not do.
- [`reference/layer6-providers.md`](reference/layer6-providers.md): routes,
  providers, consent, privacy, and cost limits.
- [`reference/layer6-evaluation.md`](reference/layer6-evaluation.md): how
  review quality is measured and how prompts change.
- [`reference/mcp-interface.md`](reference/mcp-interface.md): the MCP scan
  and review tools.

## Contributing

| Page | Use |
|---|---|
| [`architecture.md`](architecture.md) | Pipeline, crates, and resource boundaries. |
| [`project-reference.md`](project-reference.md) | Compact product and repository summary. |
| [`current-state.md`](current-state.md) | Detailed implemented behavior. |
| [`reference/detector-architecture.md`](reference/detector-architecture.md) | How a detector is defined, registered, and tested. |
| [`reference/detector-promotion.md`](reference/detector-promotion.md) | How a detector moves between the `full` and `gating` profiles. |
| [`donor-policy.md`](donor-policy.md), [`reference/donor-parity-spec.md`](reference/donor-parity-spec.md), [`reference/donor-gaps.md`](reference/donor-gaps.md) | External reference tools, their licenses, and parity tests. |
| [`distribution.md`](distribution.md) | What is published, releases, and licensing. |
| [`documentation-layout.md`](documentation-layout.md) | Where each kind of document belongs. |

Code, schemas under `contracts/`, and tests are the authority when prose
disagrees with them.

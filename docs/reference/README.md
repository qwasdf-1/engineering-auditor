# Reference Docs

`docs/reference/` is the curated current implementation reference layer.

- `detector-architecture.md` and `language-support.md`: enforceable catalog,
  owner, role, capability, and generated coverage truth.
- `layer1-correctness.md` through `layer5-optimization-opportunities.md`:
  deterministic detector behavior and graph-input boundaries.
- `layer6-llm-review.md`: review semantics and epistemic boundary.
- `layer6-providers.md`: routes, adapters, consent, qualification, privacy, and
  retention.
- `layer6-evaluation.md`: prompt/corpus/evaluation/optimizer contract.
- `mcp-interface.md`: typed asynchronous scan/review operations.
- `stability.md`: stable, experimental, and internal surface classes and the
  versioning policy.
- `report-contract.md`: report v5, map v1, review v2, graph v2, baseline v2,
  compatibility, provenance, and gates.
- `detector-precision.md`: measured per-detector precision on the ecosystem
  corpus and the open noise mechanisms.
- `donor-parity-spec.md`, `donor-gaps.md`, and `detector-promotion.md`: donor
  agreement and release-profile authority.
- `original-vision.md`: non-normative historical design motivation; current
  contracts and reference docs take precedence.

Reference docs must cite executable behavior or schemas, label partial or
unsupported behavior plainly, and avoid presenting local plans/run logs as
product authority.

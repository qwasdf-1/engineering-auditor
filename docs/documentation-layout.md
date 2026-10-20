# Documentation Layout

This is the canonical documentation taxonomy. Code, schemas, contracts, tests,
and the checked-in donor manifest remain implementation truth when prose
disagrees.

## Authority Order

1. Runtime truth: code, schemas, contracts, tests, and the checked-in donor
   manifest.
2. Current reference: `docs/project-reference.md`, `docs/architecture.md`,
   `docs/current-state.md`, `docs/donor-policy.md`, `docs/reference/`.
3. Implementation plans (the docs/v1/plans folder), where a checkout has them,
   are design history excluded from the published repository. They explain
   intent and sequencing but are not repository authority;
   current behavior lives in the reference layer above.
4. Local, ignored operator material may record issues, research, and
   historical context, but it is not part of repository authority.

## Canonical Structure

```text
docs/
  README.md                       documentation map
  documentation-layout.md         taxonomy and layout rules
  project-reference.md            product and repository authority
  architecture.md                 system design and crate responsibilities
  current-state.md                compact present-tense repo reality
  known-gaps.md                   explicit residual and unsupported behavior
  donor-policy.md                 donor license tiers and runtime rules
  distribution.md                 public/local repository boundary
  reference/                      current reference material
    detector-architecture.md      canonical detector organization + dual-path form
    language-support.md            auto-generated detector × language matrix
    donor-gaps.md                  per-donor gap analysis
    donor-parity-spec.md           donor parity spec and oracle contracts
    detector-promotion.md          full-only risk and promotion dispositions
    detector-precision.md          measured precision on the ecosystem corpus
    report-contract.md              report/map/review/graph/baseline/gate contract
    layer1-correctness.md .. layer5-optimization-opportunities.md
                                      deterministic layer references
    layer6-llm-review.md             evidence-constrained review semantics
    layer6-providers.md              provider, route, privacy, and consent contract
    layer6-evaluation.md             prompt, corpus, gate, and improvement contract
    mcp-interface.md                 typed scan/review MCP lifecycle
    stability.md                     stable/experimental surface policy
    original-vision.md               non-normative historical motivation
```

## Placement Rules

- Feature truth goes in `docs/reference/`.
- Product summary goes in `docs/project-reference.md`.
- Present status goes in `docs/current-state.md`.
- Known limitations and environment-blocked qualification go in
  `docs/known-gaps.md`.
- Implementation plans go in the docs/v1/plans folder; they are excluded from the
  published repository, so maintained docs must not depend on them.
- Issue ledgers, work registers, raw research, and historical notes are local
  operator material and remain ignored.

## Lifecycle Rules

1. Create or update the narrowest current authority document first.
2. Before archiving, extract still-current behavior into the active reference
   layer.
3. Keep tracked docs present-tense and implementation-focused.
4. Distill completed local plans into reference docs before release.

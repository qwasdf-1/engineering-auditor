# Donor Gap Analysis

Updated: 2026-07-18

This is the current donor gap reference. Runtime status and parity claims are
governed by `auditor-donors.lock.json`,
`docs/donor-policy.md`, and `docs/reference/donor-parity-spec.md`.

## Execution Status

| Donor ID | State |
|---|---|
| `agent-slop-lint` | Live atomic parity |
| `rust-clippy` | Live atomic parity |
| `ruff` | Live atomic parity |
| `eslint` | Live atomic parity |
| `pmd` | Live atomic parity |
| `detekt` | Live atomic parity |
| `golangci-lint` | Live atomic parity |
| `pylint` | Live atomic parity |
| `jscpd` | Live atomic parity |
| `bandit` | Live atomic parity |
| `import-linter` | Live atomic parity |
| `dependency-cruiser` | Live atomic parity |
| `semgrep` | Non-parity reference |
| `deptrac` | Non-parity reference |

If a mapped donor becomes unavailable, its typed status must carry a blocking
reason and expected native command. Live tests never silently skip it.

## Remaining High-Value Gaps

### Clone Analysis

Engineering Auditor now uses one function-bounded rolling-token engine under
`source_duplicate_detection`, including consistent identifier/literal
renaming, repeated semantic-anchor requirements, maximal spans, and overlap
suppression. Homogeneous unique-call/data lists remain silent. PMD CPD and
JSCPD still provide broader language-specific and top-level clone coverage. EA
findings remain review candidates rather than proof that a clone should be
removed.

### Security And Dataflow

EA has live Bandit parity for mapped atomic capabilities and selected Python
security rules. It does not cover Bandit's or Ruff's complete security
inventories and has no Semgrep-style interprocedural taint engine. Semgrep is
study-only under the current policy, not a parity claim.

### JavaScript Semantics

EA implements selected ESLint capabilities including empty handlers,
`no-await-in-loop`, `require-atomic-updates`, and live exact
`no-duplicate-imports` parity. Many JS-specific semantic/style rules remain
outside EA's engineering-audit scope.

### Architecture Resolution

EA supports configurable layering, baselines, package coupling, resolved local
imports and imported symbols, inheritance, re-exports, deterministic cycles,
and completeness/provenance reporting. Dynamic imports, reflection,
build-generated modules, and ecosystem-specific package resolution remain
explicitly unresolved. Deptrac remains a PHP-specific non-parity reference.

### Language Breadth

GolangCI-Lint, Detekt, Ruff, PMD, and Pylint each expose substantially larger
language-specific rule inventories. EA promotes only capabilities that fit its
typed evidence contract and receive positive, semantic-negative, integration,
and live parity coverage where an executable donor exists.

## Completed Promotions

The v0.8.9 profile audit promoted `duplicate_import`, `out_parameter`,
`excessive_nesting_depth`, and `unreachable_code` to gating after executable
semantic negatives, exact mapped live parity, and repository review. All
remaining full-only owners have explicit risk dispositions in
[`detector-promotion.md`](./detector-promotion.md).

- AST-aware cyclomatic, cognitive, NPath, nesting, and Halstead metrics
- package coupling and churn/complexity hotspot analysis
- dead-code and orphan-symbol source-reference analysis
- stringly typed parameters and collection-parameter mutation
- duplicate imports, out parameters, and excessive nesting depth
- live native PMD parameter-reassignment parity
- live native mappings for ESLint duplicate imports and JSCPD clone detection
- live native mappings for Ruff complexity, Pylint nesting, Import Linter
  forbidden layers, and Dependency Cruiser cycles
- configurable architectural layering with baselines
- selected Bandit, ESLint, Detekt, GolangCI-Lint, Clippy, and
  agent-slop capability parity

Exact mapped cases and intentional semantic differences are documented in
`docs/reference/donor-parity-spec.md`.

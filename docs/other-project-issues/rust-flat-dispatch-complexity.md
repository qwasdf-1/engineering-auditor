# Rust flat dispatch receives algorithmic-complexity refactoring advice

**Status:** Open; recommendation precision, not a counting defect. The
branch counts are accurate. **Observed:** 2026-10-30 with Engineering
Auditor 1.5.0 (release commit `d014177`), default `gating` profile, in a
Rust application with native adapters; the fixture below reproduced on
2026-11-04 with 1.5.0 and on 2026-11-18 at 5513420.

## Downstream Shapes

The downstream review found four low-severity `function_complexity` findings
(11 to 14 decision points), each with this recommendation:

> `Reduce the number of decision points by splitting or simplifying logic.`

| Shape | Reviewed as |
|---|---|
| Exhaustive enum-to-static-label mapping; each arm returns one literal. | A wrapper or lookup table moves the same mapping and can lose the compiler's exhaustiveness check. |
| Flat translation of external button codes, with an unsupported-code fallback. | The same: a flat mapping. |
| Twelve sample-format arms, each binding a Rust type and calling one shared generic stream builder. | The common algorithm is already shared; the arms select distinct compile-time types. |
| Event-type dispatch to separate handlers, after ordered sequence checks. | Real ordering responsibility; still worth review, not an exemption. |

The matches and counts are real. What is disputed is the inference that
each function contains an algorithm that splitting would improve.

## Reproduction

A dependency-free fixture reproduces the label case. Save it as
`fixture/src/lib.rs`:

```rust
pub enum Signal { S0, S1, S2, S3, S4, S5, S6, S7, S8, S9, S10, S11 }
pub fn signal_label(signal: Signal) -> &'static str {
    match signal {
        Signal::S0 => "S0", Signal::S1 => "S1", Signal::S2 => "S2",
        Signal::S3 => "S3", Signal::S4 => "S4", Signal::S5 => "S5",
        Signal::S6 => "S6", Signal::S7 => "S7", Signal::S8 => "S8",
        Signal::S9 => "S9", Signal::S10 => "S10", Signal::S11 => "S11",
    }
}
```

From the fixture's parent directory, so the report is not a later scan's
input:

```sh
auditor scan --repo-root fixture --profile gating --format json --output report.json
```

No Cargo manifest, baseline, or suppressions are needed. The scan exits zero
on completion; read the finding from the report:

```json
{
  "id": "EA-3baa417cfe46d52a",
  "detector": "function_complexity",
  "severity": "low",
  "confidence": "high",
  "evidence": {
    "branch_count": 12,
    "function": "signal_label",
    "source": "ast"
  },
  "recommendation": "Reduce the number of decision points by splitting or simplifying logic."
}
```

The ID identifies this fixture at `src/lib.rs`; another path or symbol
changes it.

## Suggested Follow-Up

- Keep the count, but tell flat literal mappings, typed adapter dispatch,
  handler dispatch, and nested or stateful branching apart in the evidence or
  advice: nesting depth, guards, and whether arms hold further decisions or
  several effects.
- For a pure mapping, advise checking completeness and representation drift
  rather than extraction. High confidence in the count should not imply high
  confidence in a refactor.
- Test the fixture next to equally wide matches with guards, nested
  decisions, or side effects, which should stay reportable. A blanket
  exemption for `match`, enums, adapters, or game code would hide real risk.

See [the wider actionability report](structural-findings-actionability.md).

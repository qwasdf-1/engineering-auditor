# Structural findings need clearer actionability evidence

**Status:** Feedback on recommendation precision; no incorrect detector
result was established. **Observed:** 2026-10-29 and 2026-10-30 with
Engineering Auditor 1.5.0 (release commit `d014177`) in a Rust application
with native adapters.

## Evidence

The downstream review kept a record for each finding it retained: the
source span and hash, the rationale, the risks, the tests that support it,
and when to look again. Changed source voids the record. Retained findings
stay visible in the scan, and an unreviewed or stale one stays open.

Representative cases, described by shape:

| Detector | Observation | Downstream assessment |
|---|---|---|
| `too_many_returns` | Six exits in a session's command handler separate close, admission rejection, duplicate handling, stop, and errors found during reset or import. | The count is accurate. Ordered guard clauses were kept: nesting or dispatch would hide which check wins. |
| `too_many_parameters` | Six arguments to a recovery entry point name a store, a runtime, two resolvers, a grouped launch identity, and a callback. | The count is accurate. These are independent owners at a trusted boundary, and a parameter object would only rename them. |
| `source_duplicate_detection` | A 76-token match across two authorization paths that differ in their transaction-ID checks, policy, and error bindings. | The overlap is real. With the shared algorithms already extracted, merging the remaining wiring would couple distinct flows. |
| `source_duplicate_detection` | A 53-token match across two typed entry points that both delegate validation to one admission owner. | A true structural match, kept: the recommendation itself says to consolidate only when a shared owner reduces coupling. |
| `churn_complexity_hotspot` | An unchanged image-validation file, reported from a fresh history window at four commits, +139 lines, and CCX 118. | The history is accurate, but it shows no new regression and names no useful extraction. |

These are true structural signals with a disputed recommendation, not
correctness bugs.

## Review Outcomes

| Outcome | Meaning |
|---|---|
| Detector false positive | Evidence contradicts what the detector claims. None was established here. |
| Accurate, accepted after review | The count or match is real, and a concrete ownership or safety argument supports keeping the code. All the cases above. |
| Open or stale | No current justification, or its source changed. An earlier acceptance does not carry forward. |

Acceptance holds for the reviewed source and trade-off only. It does not
make the pattern right elsewhere.

## Suggested Follow-Up

- Separate the measured fact from the proposed action in finding output.
- Give the evidence that tells a useful change from a metric-only one where
  it is available: the role of each return site, the matched clone spans and
  their control-flow boundaries, and the history window and change
  attribution behind a hotspot.
- Judge recommendations on fixtures by whether they improve the code, not by
  whether they lower a count or reset file history.
- Do not blanket-exempt guards, FFI, orchestration, or typed wiring.

[Rust flat dispatch](rust-flat-dispatch-complexity.md) gives a
dependency-free reproduction for one of these shapes.

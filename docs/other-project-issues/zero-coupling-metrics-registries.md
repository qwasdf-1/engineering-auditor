# Zero-coupling metric registries reported as class structure defects

**Status:** Open. **Observed:** 2026-10-30 with Engineering Auditor 1.5.0
in a Java Minecraft (Fabric) mod; reproduced on 2026-11-18 at 5513420.

## Evidence

A diagnostic class holds a flat set of static atomic counters, each with a
one-line increment and a one-line getter. This reconstruction has the same
shape, with 26 counters:

```java
import java.util.concurrent.atomic.AtomicInteger;

public final class Counters {
    private static final AtomicInteger EVENT0 = new AtomicInteger();
    // ... EVENT1 through EVENT25 ...

    public static void event0Seen() {
        EVENT0.incrementAndGet();
    }

    public static int event0Count() {
        return EVENT0.get();
    }
    // ... the same pair for each counter ...
}
```

`class_structure_metrics` reports it at medium severity:

> ``Class `Counters` exceeds a CK/WMC budget (WMC=52, CBO=0, DIT=0, NOC=0).``
>
> Recommendation: `Reduce class responsibility, inheritance depth, or coupling when the class owns unrelated behavior.`

The downstream class had WMC 51 with the same zero coupling and inheritance.

## Why It Is Not Actionable

- The class couples to nothing but `AtomicInteger` (CBO 0), and has no base
  class or subclasses (DIT 0, NOC 0).
- Every method has cyclomatic complexity 1, so WMC here counts methods, not
  decisions.
- Splitting the registry into several smaller ones would not simplify it:
  the formatter that prints every counter would then depend on all of them.

## Root Cause

`crates/auditor-detectors/src/structural_classes.rs` keeps a class only when
every metric is within its threshold:

```rust
if wmc <= CLASS_WMC_THRESHOLD
    && cbo <= CLASS_CBO_THRESHOLD
    && (dit <= CLASS_DIT_THRESHOLD || wmc == 0)
    && (noc <= CLASS_NOC_THRESHOLD || is_extension_point(class))
{
    continue;
}
```

`CLASS_WMC_THRESHOLD` is 40 (`structural_semantic.rs`), so WMC above 40
alone reports the class, whatever its coupling, depth, or decisions.

## Suggested Follow-Up

- When WMC is the only metric over its threshold and every method is
  branch-free, say so in the evidence, and lower the severity or raise the
  threshold for that case.
- Recognize registries whose fields are all static atomics or maps and whose
  methods are all trivial increments or accessors.
- Keep reporting a large class with real decisions or real coupling.

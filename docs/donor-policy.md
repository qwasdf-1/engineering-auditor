# Donor Policy

This file defines the donor license tiers, metadata requirements, and runtime
rules for using external donor source in Engineering Auditor.

Engineering Auditor takes an aggressive donor posture, but the runtime code must
remain license-clean and provenance-traceable.

## License Tiers

- `permissive_compatible`: direct algorithm ports are allowed when attribution
  and notice requirements are satisfied.
- `weak_copyleft_reference_only`: use as a black-box oracle or clean-room
  reference unless a legal review approves direct use.
- `strong_copyleft_reference_only`: reference and parity behavior only; do not
  copy implementation into first-party crates.
- `unknown_reference_only`: no direct porting until the license is classified.
- `internal_reference`: local project material copied for migration/parity.

## Required Donor Metadata

Every donor in `references/` needs an entry in `auditor-donors.lock.json` with:

- source and snapshot
- license and license tier
- use mode
- runtime dependency status
- imported tests or parity artifacts, with a specific justification
- known restrictions

## Runtime Rule

Production crates must not import code by path from `references/`. Donor source
can drive live oracle tests, atomic capability design, algorithm notes, or
clean-room implementation work.

## Parity Rule

Donor parity requires Engineering Auditor and the locked donor to execute
against the same input and compare their native findings. A hand-written EA
expectation, copied donor output, or local snapshot is an integration oracle,
not donor parity.

Live adapters must fail on missing required tools, missing checkouts, snapshot
drift, invalid output, or empty positive output. Unavailable donors remain
explicitly unavailable in the typed status contract; they do not silently
skip.

Provisioned live cases are explicit ignored integration tests so a public
all-feature build does not require untracked donor installations. This is a
test-lane boundary, not a soft skip: release qualification must invoke every
ignored live case, where any missing prerequisite remains a hard failure.

Static fixtures are retained only when a stable external representation is
intrinsic to the test, such as multi-language parser integration, and the
fixture directory documents that justification.

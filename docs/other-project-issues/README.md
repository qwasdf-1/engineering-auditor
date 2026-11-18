# Other Project Issues

Concise, reviewed reports from downstream projects that identify actionable
Engineering Auditor issues. This is not a general issue tracker. Each note
gives the auditor version, concrete evidence, the observed behavior, and a
bounded recommendation. Notes describe downstream code by its shape or by a
reconstruction; they hold no downstream source, private paths, names, or
secrets.

## Reports

| Report | Status |
|---|---|
| [PyYAML safe-loader subclass reported as unsafe deserialization](pyyaml-safe-loader-subclass.md) | Fixed after 1.5.0 |
| [Platform overrides and domain mutations reported as parameter or out-parameter defects](platform-constrained-parameters-and-mutations.md) | Overrides fixed after 1.5.0; `out_parameter` open |
| [Zero-coupling metric registries reported as class structure defects](zero-coupling-metrics-registries.md) | Open |
| [Rust flat dispatch receives algorithmic-complexity refactoring advice](rust-flat-dispatch-complexity.md) | Open; recommendation precision |
| [Structural findings need clearer actionability evidence](structural-findings-actionability.md) | Feedback |

A finding a downstream review accepted is not a false positive. Acceptance
is that project's outcome for the reviewed source; it is not a suppression,
and it does not exempt similar code.

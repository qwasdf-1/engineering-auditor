# Contributing

Thanks for helping improve Engineering Auditor. Start with
[`docs/architecture.md`](docs/architecture.md) for how the code is organized
and [`docs/reference/detector-architecture.md`](docs/reference/detector-architecture.md)
before adding or changing a detector.

## Build And Test

You need Rust 1.86 or newer.

```sh
cargo build
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --check
python3 tooling/detector_gate_lint.py
python3 tooling/lint/detector_arch.py
python3 -m unittest tooling.lint.test_detector_arch tooling.test_detector_gate_contract
```

`.github/workflows/ci.yml` lists every check CI runs, including the remaining
Python test modules.

The repository gates itself. Before opening a pull request, check that your
change adds no findings:

```sh
cargo run -p auditor-cli -- gate --repo-root . \
  --baseline config/auditor/gating-baseline-v1.json
```

Fix new findings rather than adding them to the baseline. A baseline entry
needs a reviewer-approved reason.

## Detector Changes

- Every detector has a catalog entry in `crates/auditor-detectors/src/registry.rs`
  and a capability case with one positive and one negative repository.
- Declare only the languages the detector actually supports, and keep
  unsupported languages silent.
- After adding, removing, or renaming a detector, regenerate and verify the
  docs:

  ```sh
  python3 tooling/doc_hygiene/generate_language_support.py
  python3 tooling/doc_hygiene/verify_spec_claims.py
  ```

- A detector joins the `gating` profile only after its precision is measured
  ([`docs/reference/detector-promotion.md`](docs/reference/detector-promotion.md)).

## AI Review Changes

Prompt and case-template wording changes only through the evaluated
offline-improvement process in
[`docs/reference/layer6-evaluation.md`](docs/reference/layer6-evaluation.md).

## Licensing

Contributions are licensed under the Apache License 2.0. Do not copy code from
the external reference tools; [`docs/donor-policy.md`](docs/donor-policy.md)
explains how they may be used.

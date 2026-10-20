# Distribution Boundary

The tracked tree is the complete distributable Engineering Auditor product.
It contains runtime crates, schemas, tests, CI, donor provenance for public
donors, and maintained feature and implementation documentation.

Implementation plans (the docs/v1/plans folder), where a checkout has them, are
design history and are not runtime or build inputs; `.gitattributes` marks
them `export-ignore`, so the published repository does not contain them.
The same applies to `AGENTS.md` and its `CLAUDE.md` symlink, which hold
working rules for agents in a private checkout; outside contributors use
`CONTRIBUTING.md`. Issue ledgers, work registers, raw research,
historical notes,
donor checkouts, private integration adapters, private evaluation corpora, and
optimizer/run output are intentionally ignored. They may exist in a developer
worktree but must not be required to build, test, or operate the tracked
product.

The public Layer 6 boundary deliberately tracks reviewed prompt bundles,
machine-readable public corpus manifests, and reproducible corpus generators.
Private cases belong under `eval/private/`; mutable optimization state belongs
under `.auditor-review-optimization/`. Neither location may be a runtime input
required by the default product.

## Publishing

A public remote must be initialized from a clean export of the tracked tree or
an equivalent squashed root commit. Adding ignore rules or deleting files in a
later commit does not remove private material from existing Git history.

Before publishing, verify:

```sh
git ls-files -ci --exclude-standard
git grep -i '<private-project-name>'
cargo test --workspace --all-features
```

The first command must print nothing, the name scan must print nothing, and the
workspace must build without any ignored overlay.

The all-feature command compiles the live donor harness but does not execute its
21 explicitly provisioned integration groups. Those tests are marked ignored
because their locked source checkouts and runtimes are intentionally outside
the tracked product, not because missing prerequisites are acceptable. Release
qualification separately runs every such group on the provisioned runner:

```sh
cargo test --locked -p auditor-detectors \
  --features donor-oracle,graph-analysis \
  -- --ignored --test-threads=1
```

That explicit lane has no soft skips. See
`docs/reference/donor-parity-spec.md` for its environment contract.

## Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`:

1. The tag must equal the workspace version in `Cargo.toml` (`v1.0.0` for
   `1.0.0`), and `CHANGELOG.md` must have a non-empty `## 1.0.0` section.
2. `auditor` is built with `--locked` for `x86_64-unknown-linux-gnu`,
   `aarch64-apple-darwin`, and `x86_64-apple-darwin`, and each binary is
   packaged with `LICENSE`, `README.md`, `THIRD-PARTY-LICENSES.txt`, and a
   `review/` directory (the
   case templates, prompt bundle, and review brief from `config/auditor/`,
   plus `routes-example.json`) as `auditor-<version>-<target>.tar.gz`.
3. A GitHub release is published with the archives, a `SHA256SUMS` file, and
   notes taken from that CHANGELOG section
   (`tooling/release/changelog_section.py`).

Verify a download with `sha256sum -c SHA256SUMS --ignore-missing`.

## Licensing

First-party code is licensed under Apache-2.0 (`LICENSE`).

Donor provenance in `auditor-donors.lock.json` was audited for 1.0.0:

- No donor source is vendored into, linked into, or imported by a
  production crate; the lock file declares no imported files. Detector code
  cites donor rule names (for example PMD `NPathComplexity`, Bandit B113,
  Sonar's cognitive-complexity specification) for semantics, and is written
  in Rust over tree-sitter rather than ported from the donors' Java, Kotlin,
  Python, Go, or OCaml sources.
- The copyleft donors (Semgrep, LGPL-2.1; golangci-lint, GPL-3.0; Pylint,
  GPL-2.0) are `parity_reference` only. They run as separate processes in
  ignored, operator-provisioned tests and are never distributed.
- Permissive donors used as algorithm references (Ruff and JSCPD, MIT; PMD,
  Detekt, and Bandit, Apache-2.0; Deptrac, MIT) are compatible with
  Apache-2.0. A future direct port must keep the donor's copyright and
  license notice, as `docs/donor-policy.md` requires.
- Every Rust crate compiled into `auditor` is under a permissive license
  (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CDLA-Permissive-2.0, or
  Unlicense); `r-efi` is offered under MIT, Apache-2.0, or LGPL and is used
  under MIT.

Each release archive carries `THIRD-PARTY-LICENSES.txt`, generated for its
target by `tooling/release/third_party_licenses.py` from `cargo metadata`:
every crate compiled into `auditor`, grouped under each distinct license
text found in the crate. Five tree-sitter grammar crates (C++, Java, Julia,
Ruby, TypeScript) publish no license file; they are listed with their MIT
expression, and their upstream repositories carry the full notice.

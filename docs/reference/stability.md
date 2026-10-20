# Stability Policy

This document classifies every public Engineering Auditor surface and states
what a version number promises about it. The contract test
`stability_policy_classifies_every_contract_schema` in `auditor-report` fails
when a schema under `contracts/` is missing from the schema table below.

## Classes

| Class | Promise |
|---|---|
| **stable** | An incompatible change ships only in a major release. Minor releases may add optional fields, enum members that consumers must already tolerate, commands, and flags. Every identity or baseline format change ships an upgrade path. |
| **experimental** | Any release may change or remove it. Changes are listed in the CHANGELOG. Experimental surfaces are labelled in CLI help and MCP tool descriptions. |
| **internal** | This is not a product surface: development provenance or tooling. There is no compatibility promise. |

The project follows semantic versioning:
- **Major:** breaks a stable surface.
- **Minor:** adds to a stable surface, adds detectors, or changes profile
  membership.
- **Patch:** fixes defects without adding detectors or gating findings.

Since 1.0.0, a stable surface changes incompatibly only in a major release.
Before 1.0.0, minor releases could break stable surfaces; those breaks are
listed in the CHANGELOG.

## Language Tiers

The language a file is written in decides how much of the catalog applies to
it. Tier 1 (Rust, Python, JavaScript, TypeScript, Go, Java) carries the full
promise. Tier 2 (Kotlin, C#, C, C++) is held to the Tier-1 standard for
Layers 1 and 2. Tier 3 (Ruby, Julia, Shell) is structural-only: complexity,
Halstead density, file size, and Ruby's `!!` lint. A release may add
capabilities to a lower tier; removing a claimed capability from a tier is an
incompatible change. `language-support.md` lists every detector-language mode.

## What Stability Does Not Cover

- **The finding set.** Detectors improve, so a newer release may report
  findings that an older one missed, or stop reporting false positives. What
  is stable is the identity of an unchanged finding:
  - its finding-ID v3 hash;
  - the baseline and gate formats that compare it.

  Consumers absorb an upgrade by reviewing new findings against their
  baseline.
- **Human-oriented text.** The finding titles, messages, and recommendations
  in human digests and in prose fields are for people. Their wording may
  change in any release.
  - Titles are static and participate in finding identity, so changing a title
    is an identity change and follows the stable rule.
- **Rust library APIs.** The workspace crates (`auditor-core`,
  `auditor-detectors`, and the others) are implementation crates. Only the CLI,
  the JSON/SARIF artifacts, and the config file are product surfaces.

## CLI

| Surface | Class | Notes |
|---|---|---|
| `scan` (without `--review`) and its flags | stable | `--format json\|sarif\|human`. The structure of the `human` output is not stable; its existence and flags are. |
| `gate` | stable | Gate result schema 2. |
| `baseline create`, `baseline rekey`, `baseline migrate` | stable | |
| `digest` | stable | Flags only; the digest text is human-oriented. |
| `catalog` | stable | Output follows `detector-manifest.schema.json`. |
| `version` | stable | |
| `scan --review`, all `review` subcommands | experimental | Layer 6 is opt-in and experimental. |
| `donor check`, `donor sync` | internal | Development provenance tooling. |
| Exit codes | stable | `0` success; `1` policy failure (`gate`, `review verify`, or `review gate` failed, with the result on stdout); `2` usage, input, or runtime error (message on stderr). |

## Artifacts And Identity

| Surface | Class | Version |
|---|---|---:|
| Audit report JSON | stable | 5 |
| Finding identity | stable | 3 (frozen by a golden-ID test; see `report-contract.md`) |
| Detector IDs | stable | A rename or removal is a major change. New detectors are minor changes. |
| Detector profile membership (`gating`/`full`) | stable format, changing content | Promotions and demotions are minor changes and are listed in the CHANGELOG. |
| Standalone baseline | stable | 2 |
| Gate result | stable | 2 |
| SARIF output | stable | SARIF 2.1.0; `partialFingerprints.engineeringAuditor/v3` equals the finding ID. |
| Evidence graph (embedded in the report) | stable | 2 |
| Repository-map sidecar | experimental | 1 |
| Engineering review artifacts | experimental | 2 |

## Configuration

| Surface | Class | Notes |
|---|---|---|
| `auditor.toml` `[detectors.<id>]` keys `enabled`, `threshold`, `severity`, `entropy_threshold`, `min_length`, `max_length`, `ratio_threshold`, `min_count` | stable | |
| `auditor.toml` `[layering]`, `[layering.layer.<name>]`, `[layering.rule.<name>]` | stable | |
| `.slop.toml` compatibility mapping | experimental | Read only when `auditor.toml` is absent. |
| `.eaignore` audit-scope files (gitignore syntax, any directory) | experimental | Matched paths are not discovered. |
| `auditor.toml` `[exclusions.<name>]` keys `paths`, `detectors` (comma-separated) | experimental | Drops the listed detectors' findings under the listed path prefixes. |
| Layer 6 route, prompt-bundle, case-template, brief, and evidence files | experimental | |

## MCP

Every `auditor-mcp` tool is **experimental**. Tool descriptions carry an
`Experimental:` prefix. Integrations that need stability should invoke the
stable CLI.

## Contract Schemas

| Schema | Class |
|---|---|
| `audit-report.schema.json` | stable |
| `audit-gate.schema.json` | stable |
| `baseline.schema.json` | stable |
| `evidence-graph.schema.json` | stable |
| `detector-manifest.schema.json` | stable |
| `repository-graph.schema.json` | stable |
| `co-edit-journal.schema.json` | stable |
| `repository-map.schema.json` | experimental |
| `review-brief.schema.json` | experimental |
| `review-dispositions.schema.json` | experimental |
| `case-template-bundle-v1.schema.json` | experimental |
| `case-template-v1.schema.json` | experimental |
| `challenge-record-v1.schema.json` | experimental |
| `challenge-response-v1.schema.json` | experimental |
| `contextual-recommendation-v1.schema.json` | experimental |
| `engineering-case-v1.schema.json` | experimental |
| `engineering-claim-v1.schema.json` | experimental |
| `engineering-review-v2.schema.json` | experimental |
| `ensemble-comparison-v1.schema.json` | experimental |
| `evidence-packet-v1.schema.json` | experimental |
| `probe-definition-v1.schema.json` | experimental |
| `probe-result-v1.schema.json` | experimental |
| `review-agenda-v1.schema.json` | experimental |
| `review-candidate-v1.schema.json` | experimental |
| `review-escalation-suite-v1.schema.json` | experimental |
| `review-eval-corpus-v2.schema.json` | experimental |
| `review-prompt-bundle-v2.schema.json` | experimental |
| `review-qualification-v2.schema.json` | experimental |
| `review-routes-v2.schema.json` | experimental |
| `reviewer-case-review-v1.schema.json` | experimental |
| `donor-manifest.schema.json` | internal |
| `ecosystem-corpus.schema.json` | internal |
| `ecosystem-labels.schema.json` | internal |

## Upgrading

- **A new finding-ID version.** Run `baseline migrate --baseline <old>
  --from-report <old-report> --report <new-report>`. Review the unmatched
  entries.
- **A new release with new or promoted detectors.** Run `gate` against the
  existing baseline. Review the new findings, then run `baseline create` (or
  `baseline rekey --prune-resolved`) once they are accepted.

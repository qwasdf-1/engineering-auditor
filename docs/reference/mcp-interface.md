# MCP Interface Contract

`auditor-mcp` exposes typed JSON-RPC tools for deterministic scans and the
shared Layer 6 review service. Every tool is experimental and its description
carries an `Experimental:` prefix (`stability.md`). Tool argument structs reject unknown fields.
Malformed requests, unknown jobs, policy failures, and busy state are explicit
errors rather than silent defaults.

## Tools

| Tool | Purpose |
|---|---|
| `audit_scan_start` | Validate scan arguments and start an asynchronous deterministic scan. |
| `audit_job_get` | Poll a scan job and retrieve its sanitized result/error state. |
| `audit_catalog` | Return the detector catalog for a selected profile. |
| `audit_review_validate` | Verify a reviewed report, its v2 artifact, bound plan, live inputs, and optional disposition coverage. |
| `audit_review_plan` | Build and publish a deterministic provider-free v2 case plan. |
| `audit_review_case` | Inspect one typed case from a bound v2 plan. |
| `audit_review_start` | Start the shared report/map/source review workflow. |
| `audit_review_status` | Poll review status and current stage without returning large inputs. |
| `audit_review_result` | Return completed artifact/report metadata and optionally load the canonical reviewed report. |
| `audit_review_cancel` | Request cancellation of a queued or running review. |

The server advertises exact JSON Schemas through MCP `tools/list`. Review
inputs are paths to the same report, case-template, brief, route, prompt, and
evidence contracts as the CLI; inline v1 objects are rejected. MCP text content
uses the shared typed-Markdown renderer while `structuredContent` remains the
native MCP structured result. MCP does not implement an alternate review
engine or protocol selector.

## Job Lifecycle

Scan and review jobs use `queued`, `running`, `complete`, `failed`, or
`cancelled`. Review status additionally reports the stage:
`preparing`, `planning`, `compiling_evidence`, `resolving_probes`,
`qualifying`, `reviewing`, `reconsidering`, `comparing`, `challenging`,
`responding`, `finalizing`, or `publishing`.

Only one review may be active. A concurrent start returns the dedicated busy
error code `-32001`. The bounded store retains at most 32 recent job records.
Terminal review jobs release cancellation handles and all large request-owned
route/map/context state; result records contain paths, digests, counts, status,
and sanitized failures, not full prompts or failed bodies.

## Cancellation

Queued cancellation is immediate. Running cancellation is checked at every
review stage boundary and before each qualification target; no later stage is
entered after cancellation is observed. Because the current synchronous
transport cannot interrupt an already-running blocking request, status may
remain running until that bounded request returns. No artifact or reviewed
report is published for a cancelled review.

## Publication And Retrieval

The shared service validates report/map/snapshot/templates/brief/routes/prompts,
qualifies case-role targets, runs the review, and atomically publishes a
digest-addressed v2 artifact and bound plan followed by a distinct reviewed
report. `audit_review_result` normally returns
metadata; `include_report: true` reloads the canonical report from its published
path rather than retaining it in job memory.

`audit_review_validate` performs the same full live binding verification as the
CLI/library verifier described in `report-contract.md`.

## Test Boundary

Hermetic MCP tests cover the typed tool schemas, path-only v2 boundary,
readable Markdown results, v1-object rejection, and asynchronous real-scan job.
The shared service's native review execution is covered in `auditor-review`;
live-provider qualification remains a separate evaluation boundary.

# MCP Interface Contract

`auditor-mcp` exposes typed JSON-RPC tools for deterministic scans. Every tool
is experimental and its description carries an `Experimental:` prefix
(`stability.md`). Tool argument structs reject unknown fields. Malformed
requests, unknown jobs, and policy failures are explicit errors rather than
silent defaults.

## Tools

| Tool | Purpose |
|---|---|
| `audit_scan_start` | Validate scan arguments and start an asynchronous deterministic scan. |
| `audit_job_get` | Poll a scan job and retrieve its sanitized result/error state. |
| `audit_catalog` | Return the detector catalog for a selected profile. |

The server advertises exact JSON Schemas through MCP `tools/list`. MCP text
content uses the shared typed-Markdown renderer while `structuredContent`
remains the native MCP structured result.

The Layer 6 review tools (`audit_review_validate`, `audit_review_plan`,
`audit_review_case`, `audit_review_start`, `audit_review_status`,
`audit_review_result`, and `audit_review_cancel`) were removed with the v2
review protocol while Layer 6 is rebuilt (see
[Layer 6](./layer6-llm-review.md)); calling one returns the unknown-tool error.

## Job Lifecycle

Scan jobs use `queued`, `running`, `complete`, or `failed`. The bounded store
retains at most 32 recent job records; result records contain paths, digests,
counts, status, and sanitized failures.

## Test Boundary

Hermetic MCP tests cover the typed tool schemas, readable Markdown results,
the removed review tools, and an asynchronous real-scan job.

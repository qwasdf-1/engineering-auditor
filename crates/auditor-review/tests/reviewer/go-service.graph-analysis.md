# Repository orientation

## Shape

- **Totals:** 3 files, 39 lines, 3 modules, 9 symbols
- **Languages:**
  - go: 3 files, 39 lines [language:go]
- **Source roles:**
  - production: 3 files, 39 lines [role:production]
- **Modules by size:**
  - internal/handlers: 1 file, 14 lines, 1 finding [module:internal/handlers]
  - cmd: 1 file, 13 lines, 2 findings [module:cmd]
  - internal/store: 1 file, 12 lines, 1 finding [module:internal/store]
- **Entry points:** 1 http_handler, 1 main
- **Programs and commands:**
  - main at cmd/svc/main.go:9, reaching `main` [entry:0, symbol:271606c7e17364a4db5d68270f0594531367d5ce56a156fb3a24324aebc8c46c]
- **HTTP routes by file:**
  - cmd/svc/main.go: 1 route [file:06dd21d220871a8a59521a8dca5b79ebbae4979a49ec919ce056a1f5de29acbe]

## Structure

- **Module dependencies:**
  - cmd → internal/handlers: 1 import [module-dependency:cmd->internal/handlers]
  - internal/handlers → internal/store: 1 import [module-dependency:internal/handlers->internal/store]

## Hotspots

- **Complexity × churn:** no history-weighted hotspot in the report (see Coverage)
- **Largest production files:**
  - internal/handlers/users.go: 14 lines, 1 finding [file:c5d307f5751894673f38e963d8f428e1a608d3c3574cfc73fa1f66c47b3f0e51]
  - cmd/svc/main.go: 13 lines, 2 findings [file:06dd21d220871a8a59521a8dca5b79ebbae4979a49ec919ce056a1f5de29acbe]
  - internal/store/store.go: 12 lines, 1 finding [file:b26d414d0eb20deebca0d2b785bd3e1090aa1a7a182e7dc30640d68c53cb40da]
- **Most-coupled production files:**
  - internal/handlers/users.go: imported by 1 file, imports 1 [file:c5d307f5751894673f38e963d8f428e1a608d3c3574cfc73fa1f66c47b3f0e51]
  - cmd/svc/main.go: imported by 0 files, imports 1 [file:06dd21d220871a8a59521a8dca5b79ebbae4979a49ec919ce056a1f5de29acbe]
  - internal/store/store.go: imported by 1 file, imports 0 [file:b26d414d0eb20deebca0d2b785bd3e1090aa1a7a182e7dc30640d68c53cb40da]
- **Files with the most findings:**
  - cmd/svc/main.go: 2 findings from 2 detectors [file:06dd21d220871a8a59521a8dca5b79ebbae4979a49ec919ce056a1f5de29acbe]
  - internal/handlers/users.go: 1 finding from 1 detector [file:c5d307f5751894673f38e963d8f428e1a608d3c3574cfc73fa1f66c47b3f0e51]
  - internal/store/store.go: 1 finding from 1 detector [file:b26d414d0eb20deebca0d2b785bd3e1090aa1a7a182e7dc30640d68c53cb40da]

## Findings

- **Totals:** 4 findings: 3 medium, 1 low
- **By layer:** L1 3, L4 1
- **By detector:**
  - go_errcheck (L1): 2 findings, highest medium [detector:go_errcheck]
  - bind_all_interfaces (L1): 1 finding, highest low [detector:bind_all_interfaces]
  - unbounded_concurrency (L4): 1 finding, highest medium [detector:unbounded_concurrency]
- **Densest symbols:**
  - `main` (function, cmd/svc/main.go:9-13): 2 findings from 2 detectors [symbol:271606c7e17364a4db5d68270f0594531367d5ce56a156fb3a24324aebc8c46c, finding:EA-16e38be1bd02105c, finding:EA-974d143b3c77886a]

## Coverage

- **Detectors:** 77 of 131 executed; 10 support none of the repository's languages; 38 not applicable
- **Missing inputs or failed:**
  - large_file_split_candidate (L2): missing_input, missing repo_graph [detector:large_file_split_candidate]
  - repo_graph_high_fanout_source_file (L4): missing_input, missing repo_graph [detector:repo_graph_high_fanout_source_file]
  - repo_graph_import_cycle (L4): missing_input, missing repo_graph [detector:repo_graph_import_cycle]
  - repo_graph_isolated_source_file (L4): missing_input, missing repo_graph [detector:repo_graph_isolated_source_file]
  - shotgun_surgery_cluster (L4): missing_input, missing co_edit_journal [detector:shotgun_surgery_cluster]
  - small_file_consolidation_candidate (L2): missing_input, missing repo_graph [detector:small_file_consolidation_candidate]
- **Analysis:** 0 syntax-recovered files; evidence graph complete; map: 6 unresolved calls, 3 unresolved imports, 0 dynamic imports, 0 call edges truncated
- **Unknown:**
  - History: no history digest was supplied, so change frequency, growth, and co-change are unknown; only the report's complexity × churn findings carry history.
  - Runtime: no measurements; cost judgments rest on structure alone.
  - Tests: the repository has no test files.


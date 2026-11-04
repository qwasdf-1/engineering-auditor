# Repository orientation

## Shape

- **Totals:** 11 files, 76 lines, 6 modules, 37 symbols
- **Languages:**
  - python: 10 files, 73 lines [language:python]
  - markdown: 1 file, 3 lines [language:markdown]
- **Source roles:**
  - production: 9 files, 68 lines [role:production]
  - test: 1 file, 5 lines [role:test]
  - documentation: 1 file, 3 lines [role:documentation]
- **Modules by size:**
  - app/store: 3 files, 27 lines, 2 findings [module:app/store]
  - app/api: 2 files, 18 lines, 0 findings [module:app/api]
  - app/core: 2 files, 12 lines, 3 findings [module:app/core]
  - app: 2 files, 11 lines, 0 findings [module:app]
  - tests: 1 file, 5 lines, 0 findings [module:tests]
  - .: 1 file, 3 lines, 0 findings [module:.]
- **Entry points:** 1 http_handler, 2 main
- **Programs and commands:**
  - main at app/main.py:10 (__main__ guard), reaching `main` [entry:1, entry:2, symbol:d94ddd04ec2197bf6d96579c8abebba01f1f8164d93500f61b8a17ef480fe192]
- **HTTP routes by file:**
  - app/api/routes.py: 1 route [file:cd3c3562c702032b8db9ca45b63260b417fdcbf8a4802381f82cbf1f3527fea5]

## Structure

- **Module dependencies:**
  - app → app/api: 1 import [module-dependency:app->app/api]
  - app → app/store: 1 import [module-dependency:app->app/store]
  - app/api → app/core: 1 import [module-dependency:app/api->app/core]
  - app/api → app/store: 1 import [module-dependency:app/api->app/store]
  - app/core → app/store: 1 import [module-dependency:app/core->app/store]
  - app/store → app/core: 1 import [module-dependency:app/store->app/core]
  - tests → app/store: 1 import [module-dependency:tests->app/store]
- **Module cycles:**
  - 2 modules import each other: app/core, app/store [module-cycle:1]
- **Structure findings:**
  - medium architectural_layering at app/core/audit.py:1: Resolved module cycle contains app/core/audit.py -> app/store/db.py. [finding:EA-9e9df25eb0094a0f]

## Hotspots

- **Complexity × churn:** no history-weighted hotspot in the report (see Coverage)
- **Largest production files:**
  - app/store/db.py: 21 lines, 2 findings [file:ba88e9b6a241109f9b15ade3e162c9ca0228b9e8e7ebf452ff7328c758022f39]
  - app/api/routes.py: 18 lines, 0 findings [file:cd3c3562c702032b8db9ca45b63260b417fdcbf8a4802381f82cbf1f3527fea5]
  - app/core/audit.py: 12 lines, 3 findings [file:f151bab917b662477cb00c50855d791cd225f3ad529e960791365ef2436843f9]
  - app/main.py: 11 lines, 0 findings [file:990f7e4140fab1ad82afc787505090b64d4024e63a891405cd9085e7e6b249dd]
  - app/store/base.py: 6 lines, 0 findings [file:621809453e69da3f8998c389c25d6985c1eaca8fcd128c05eca38a8b61c3e283]
- **Most-coupled production files:**
  - app/store/db.py: imported by 4 files, imports 2 [file:ba88e9b6a241109f9b15ade3e162c9ca0228b9e8e7ebf452ff7328c758022f39]
  - app/api/routes.py: imported by 1 file, imports 2 [file:cd3c3562c702032b8db9ca45b63260b417fdcbf8a4802381f82cbf1f3527fea5]
  - app/core/audit.py: imported by 2 files, imports 1 [file:f151bab917b662477cb00c50855d791cd225f3ad529e960791365ef2436843f9]
  - app/main.py: imported by 0 files, imports 2 [file:990f7e4140fab1ad82afc787505090b64d4024e63a891405cd9085e7e6b249dd]
  - app/store/base.py: imported by 1 file, imports 0 [file:621809453e69da3f8998c389c25d6985c1eaca8fcd128c05eca38a8b61c3e283]
- **Files with the most findings:**
  - app/core/audit.py: 3 findings from 3 detectors [file:f151bab917b662477cb00c50855d791cd225f3ad529e960791365ef2436843f9]
  - app/store/db.py: 2 findings from 2 detectors [file:ba88e9b6a241109f9b15ade3e162c9ca0228b9e8e7ebf452ff7328c758022f39]

## Findings

- **Totals:** 5 findings: 1 high, 3 medium, 1 low
- **By layer:** L1 4, L4 1
- **By detector:**
  - architectural_layering (L4): 1 finding, highest medium [detector:architectural_layering]
  - console_output_in_production (L1): 1 finding, highest low [detector:console_output_in_production]
  - shell_execution (L1): 1 finding, highest medium [detector:shell_execution]
  - sql_injection (L1): 1 finding, highest high [detector:sql_injection]
  - try_except_pass (L1): 1 finding, highest medium [detector:try_except_pass]
- **Densest symbols:**
  - `record` (function, app/core/audit.py:4-8): 2 findings from 2 detectors [symbol:81a76723aba6c62af8fa9eb258ef1c5867afe8103073caefbe1ad6c880ad5006, finding:EA-00d3f708c8441e96, finding:EA-b22f05623052061f]

## Coverage

- **Detectors:** 103 of 124 executed; 3 support none of the repository's languages; 18 not applicable
- **Analysis:** 0 syntax-recovered files; evidence graph complete; map: 17 unresolved calls, 3 unresolved imports, 0 dynamic imports, 0 call edges truncated
- **Unknown:**
  - History: no history digest was supplied, so change frequency, growth, and co-change are unknown; only the report's complexity × churn findings carry history.
  - Runtime: no measurements; cost judgments rest on structure alone.
  - Tests: 1 test file, but no coverage or results, so which code they exercise is unknown.


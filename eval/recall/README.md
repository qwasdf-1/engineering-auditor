# Recall Benchmark

Seeded cases that measure how often the algorithmic, systems, and
optimization detectors catch a known problem, and whether they stay silent
when the problem is fixed. The precision corpus in `eval/ecosystem/` measures
false positives on real code; this benchmark measures misses.

Each case is a small generated repository (`tooling/recall/cases.py`) in one
of four shapes:

| Shape | The expensive operation is |
|---|---|
| `direct` | in the loop or async function itself |
| `helper` | in a same-file helper that the loop calls |
| `cross_file` | in a helper imported from another module |
| `negative` | hoisted or batched; the detector must stay silent |

```sh
cargo build --release -p auditor-cli
python3 tooling/recall/run.py                              # print the table
python3 tooling/recall/run.py --check eval/recall/baseline.json
python3 tooling/recall/run.py --write-baseline eval/recall/baseline.json
```

`baseline.json` records each case's last accepted outcome. `--check` fails
when a detected case is missed or a silent negative fires, and reports new
detections, so the baseline only moves up; rewrite it with
`--write-baseline` after an improvement. CI runs the check.

## Current Results

Six detectors in Rust, Python, TypeScript, Go, and Java (116 cases; Go has no
`async`, so `blocking_io_in_async_context` covers four languages). Every
`direct` case is detected and every `negative` stays silent.
`per_item_external_io` and `blocking_io_in_async_context` follow calls into
repository functions, so their `helper` and `cross_file` cases are detected
too: 9 of 29 in each shape. The other four detectors still miss every
`helper` and `cross_file` case.

Helpers use neutral names (`details`, `refresh`, `matcher`) so a hit means
the detector followed the call, not that the helper's name looked like I/O.

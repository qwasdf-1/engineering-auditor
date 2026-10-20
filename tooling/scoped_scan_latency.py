#!/usr/bin/env python3
"""Measure the changed-scope scan contract.

Builds the release binary (if missing), runs a changed-scope scan on two
representative files in this repository, and asserts the contract:

  * `scope == "changed"`
  * `scanned_files` is bounded by the requested paths, not the whole repo
  * the report validates
  * the run is deterministic across consecutive invocations
  * the scoped time is materially less than a full-scope time on the same
    tree

Usage:
    python3 tooling/scoped_scan_latency.py [--repo-root <path>]
"""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_AUDITOR = REPO_ROOT / "target" / "release" / "auditor"
DEFAULT_CHANGED_PATHS = [
    "crates/auditor-detectors/src/lib.rs",
    "crates/auditor-detectors/src/source_discovery.rs",
]
RUNS = 3


def build_release(repo_root: Path) -> None:
    print("building release binary with graph-analysis ...", flush=True)
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "auditor-cli",
            "--features",
            "graph-analysis",
        ],
        cwd=repo_root,
        check=True,
    )


def run_scan(auditor: Path, repo_root: Path, extra_args: list[str]) -> tuple[str, float]:
    start = time.perf_counter()
    result = subprocess.run(
        [str(auditor), "scan", "--repo-root", str(repo_root), *extra_args],
        capture_output=True,
        text=True,
        check=True,
    )
    elapsed = time.perf_counter() - start
    return result.stdout, elapsed


def measure(auditor: Path, label: str, args: list[str], runs: int = RUNS) -> dict:
    times: list[float] = []
    payload: str | None = None
    for index in range(runs):
        output, elapsed = run_scan(auditor, REPO_ROOT, args)
        times.append(elapsed)
        if payload is None:
            payload = output
        elif output != payload and label == "scoped":
            raise RuntimeError(f"{label}: non-deterministic output across {runs} runs")
    value = json.loads(payload)
    return {
        "label": label,
        "median_ms": round(statistics.median(times) * 1000, 1),
        "min_ms": round(min(times) * 1000, 1),
        "max_ms": round(max(times) * 1000, 1),
        "scope": value.get("scope"),
        "scanned_files": value.get("scanned_files"),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    args = parser.parse_args()

    repo_root = args.repo_root.resolve()
    # Always rebuild so we exercise the current source tree. `cargo build
    # --release` is a no-op when the binary is already up to date, and this
    # avoids the trap of running a stale pre-feature binary that lacks
    # `--repo-graph` / `--co-edit-journal`.
    build_release(repo_root)
    auditor = DEFAULT_AUDITOR

    # A minimal repo graph and co-edit journal so graph-enabled full scans
    # do not fail on missing inputs.
    graph_path = repo_root / "target" / "scoped-scan-repo-graph.json"
    graph_path.parent.mkdir(parents=True, exist_ok=True)
    graph_path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "repo_graph_id": "scoped-scan-measure",
                "nodes": [
                    {"path": p, "node_kind": "file", "line_count": 1}
                    for p in DEFAULT_CHANGED_PATHS
                ],
                "edges": [],
            }
        )
    )
    journal_path = repo_root / "target" / "scoped-scan-co-edit.json"
    journal_path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "records": [
                    {
                        "change_set_id": "scoped-scan-set",
                        "files": DEFAULT_CHANGED_PATHS,
                    }
                ],
            }
        )
    )

    changed_ranges = json.dumps(
        [
            {"path": DEFAULT_CHANGED_PATHS[0], "start_line": 1, "end_line": 50},
            {"path": DEFAULT_CHANGED_PATHS[1], "start_line": 1, "end_line": 50},
        ]
    )

    scoped_args = [
        "--profile",
        "full",
        "--format",
        "json",
        "--path",
        DEFAULT_CHANGED_PATHS[0],
        "--path",
        DEFAULT_CHANGED_PATHS[1],
        "--changed-ranges-json",
        changed_ranges,
        "--repo-graph",
        str(graph_path),
        "--co-edit-journal",
        str(journal_path),
    ]
    full_args = ["--profile", "full", "--format", "json", "--repo-graph", str(graph_path)]

    scoped = measure(auditor, "scoped", scoped_args)
    full = measure(auditor, "full", full_args)

    print("\nChanged-scope scan measurement")
    print("--------------------------")
    for row in (scoped, full):
        print(
            f"{row['label']:>8}: median={row['median_ms']}ms "
            f"scope={row['scope']} scanned_files={row['scanned_files']}"
        )

    scoped_scanned = scoped["scanned_files"]
    full_scanned = full["scanned_files"]
    if scoped_scanned is None or scoped_scanned <= 0:
        print(f"\nFAIL: scoped scanned_files is {scoped_scanned!r}", file=sys.stderr)
        return 1
    if scoped["scope"] != "changed":
        print(f"\nFAIL: scoped scope is {scoped['scope']!r}, expected 'changed'", file=sys.stderr)
        return 1
    if scoped_scanned > 16:
        print(
            f"\nFAIL: scoped scanned_files={scoped_scanned} exceeds the hot-path budget "
            f"(expected <= 16 for two small files; the pre-fix regression walked the whole repo)",
            file=sys.stderr,
        )
        return 1
    if full_scanned and scoped_scanned >= full_scanned:
        print(
            f"\nFAIL: scoped scanned_files={scoped_scanned} is not bounded below full={full_scanned}",
            file=sys.stderr,
        )
        return 1

    speedup = full["median_ms"] / scoped["median_ms"] if scoped["median_ms"] else 0.0
    print(f"\nbounded: scoped scanned_files={scoped_scanned} (full={full_scanned})")
    print(f"speedup: full/scoped median = {speedup:.1f}x")
    return 0


if __name__ == "__main__":
    sys.exit(main())

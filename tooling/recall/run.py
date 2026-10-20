#!/usr/bin/env python3
"""Measure recall on the seeded cases in ``tooling/recall/cases.py``.

Usage:
    run.py [--auditor PATH] [--output PATH] [--check BASELINE] [--write-baseline BASELINE]

Writes each case into a fresh temporary repository, runs
``auditor scan --profile full --format json``, and records whether the case's
detector reported anything. A positive case is ``detected`` or ``missed``; a
negative case is ``silent`` or ``fired``. Prints a detector-by-shape table.

``--check`` compares with a recorded baseline and fails when a detected case
is now missed or a silent negative now fires; newly detected cases pass and
are reported, so the baseline only ratchets up. ``--write-baseline`` records
the current results.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tooling.recall.cases import SHAPES, Case, all_cases  # noqa: E402

DEFAULT_AUDITOR = Path("target/release/auditor")


def outcome(case: Case, detectors: set[str]) -> str:
    hit = case.detector in detectors
    if case.positive:
        return "detected" if hit else "missed"
    return "fired" if hit else "silent"


def scan(auditor: Path, case: Case) -> set[str]:
    with tempfile.TemporaryDirectory(prefix="ea-recall-") as root:
        for relative, body in case.files.items():
            path = Path(root) / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        completed = subprocess.run(
            [str(auditor), "scan", "--repo-root", root, "--profile", "full", "--format", "json"],
            check=True,
            capture_output=True,
            text=True,
        )
    return {finding["detector"] for finding in json.loads(completed.stdout)["findings"]}


def regressions(baseline: dict[str, str], results: dict[str, str]) -> list[str]:
    """Cases that got worse: detected -> missed, silent -> fired, or dropped."""
    worse = []
    for case_id, before in sorted(baseline.items()):
        after = results.get(case_id)
        if after is None:
            worse.append(f"{case_id}: no longer measured")
        elif (before, after) in {("detected", "missed"), ("silent", "fired")}:
            worse.append(f"{case_id}: {before} -> {after}")
    return worse


def improvements(baseline: dict[str, str], results: dict[str, str]) -> list[str]:
    return [
        f"{case_id}: {baseline.get(case_id, 'new')} -> {after}"
        for case_id, after in sorted(results.items())
        if after in {"detected", "silent"} and baseline.get(case_id) != after
    ]


def table(cases: list[Case], results: dict[str, str]) -> str:
    marks = {"detected": "yes", "missed": "-", "silent": "silent", "fired": "FIRES"}
    header = "| detector | language | " + " | ".join(SHAPES) + " |"
    lines = [header, "|" + " --- |" * (len(SHAPES) + 2)]
    rows: dict[tuple[str, str], dict[str, str]] = {}
    for case in cases:
        rows.setdefault((case.detector, case.language), {})[case.shape] = marks[results[case.id]]
    for (detector, language), shapes in rows.items():
        lines.append(f"| {detector} | {language} | " + " | ".join(shapes[shape] for shape in SHAPES) + " |")
    lines.append("")
    positives = [case for case in cases if case.positive]
    for shape in SHAPES[:-1]:
        shaped = [case for case in positives if case.shape == shape]
        found = sum(results[case.id] == "detected" for case in shaped)
        lines.append(f"{shape}: {found}/{len(shaped)} detected")
    negatives = [case for case in cases if not case.positive]
    fired = sum(results[case.id] == "fired" for case in negatives)
    lines.append(f"negative: {len(negatives) - fired}/{len(negatives)} silent")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description="Measure recall on seeded cases.")
    parser.add_argument("--auditor", type=Path, default=DEFAULT_AUDITOR)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", type=Path)
    parser.add_argument("--write-baseline", type=Path)
    args = parser.parse_args()

    cases = all_cases()
    results = {case.id: outcome(case, scan(args.auditor, case)) for case in cases}
    print(table(cases, results))
    if args.output:
        args.output.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.write_baseline:
        args.write_baseline.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.check:
        baseline = json.loads(args.check.read_text(encoding="utf-8"))
        for line in improvements(baseline, results):
            print(f"improved: {line}")
        worse = regressions(baseline, results)
        for line in worse:
            print(f"regressed: {line}", file=sys.stderr)
        if worse:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

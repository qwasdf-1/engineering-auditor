"""Per-language syntax coverage of an ecosystem scan, and its check against
the committed baseline in eval/ecosystem/syntax.json.

A file whose syntax tree has errors is `recovered` (structural detectors read
it without the declarations holding the errors) or `unparsed` (they skip
it). The check fails when any language's unparsed count rises above the
baseline, so a grammar or recovery regression cannot pass silently.
"""

import json
from collections import defaultdict
from pathlib import Path

BASELINE = Path(__file__).resolve().parents[2] / "eval" / "ecosystem" / "syntax.json"


def syntax_summary(reports) -> dict:
    """{language: {files, recovered, unparsed}} over the reports' files."""
    counts = defaultdict(lambda: {"files": 0, "recovered": 0, "unparsed": 0})
    for report in reports:
        for file in report.get("files", []):
            entry = counts[file["catalog_language"]]
            entry["files"] += 1
            if file.get("syntax") in ("recovered", "unparsed"):
                entry[file["syntax"]] += 1
    return {
        language: entry
        for language, entry in sorted(counts.items())
        if entry["recovered"] or entry["unparsed"]
    }


def regressions(current: dict, baseline: dict) -> list:
    """The languages whose unparsed count rose above the baseline's."""
    found = []
    for language, entry in sorted(current.items()):
        allowed = baseline.get(language, {}).get("unparsed", 0)
        if entry["unparsed"] > allowed:
            found.append(f"{language}: {entry['unparsed']} unparsed files (baseline {allowed})")
    return found


def improvements(current: dict, baseline: dict) -> list:
    """The languages whose unparsed count fell below the baseline's."""
    found = []
    for language, entry in sorted(baseline.items()):
        now = current.get(language, {}).get("unparsed", 0)
        if now < entry["unparsed"]:
            found.append(f"{language}: {now} unparsed files (baseline {entry['unparsed']})")
    return found


def load_baseline(slice_name: str) -> dict:
    if not BASELINE.exists():
        return {}
    return json.loads(BASELINE.read_text(encoding="utf-8")).get(slice_name, {})


def write_baseline(slice_name: str, current: dict) -> None:
    data = json.loads(BASELINE.read_text(encoding="utf-8")) if BASELINE.exists() else {}
    data[slice_name] = current
    BASELINE.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def check(summary: dict, slice_name: str, write: bool) -> int:
    """Checks a scan summary's syntax coverage, or records it with `write`."""
    current = summary.get("syntax", {})
    if write:
        write_baseline(slice_name, current)
        print(f"recorded {slice_name} syntax coverage in {BASELINE}")
        return 0
    baseline = load_baseline(slice_name)
    for line in improvements(current, baseline):
        print(f"improved: {line}; record it with --write")
    failures = regressions(current, baseline)
    for line in failures:
        print(f"syntax regression: {line}")
    return 1 if failures else 0

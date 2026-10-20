#!/usr/bin/env python3
"""Verify maintained language claims against the executable Rust catalog."""

from __future__ import annotations

import re
import sys

sys.dont_write_bytecode = True

from generate_language_support import LANGUAGES, LANG_SUPPORT, load_catalog


LAYER_RE = re.compile(r"^### Layer (?P<layer>[1-5])\b")


def parse_matrix(text: str) -> dict[str, dict]:
    detectors: dict[str, dict] = {}
    layer = None
    headers: list[str] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if match := LAYER_RE.match(line):
            layer = f"L{match.group('layer')}"
            headers = []
            continue
        if line.startswith("| Detector |"):
            headers = [cell.strip() for cell in line.strip("|").split("|")][1:]
            continue
        if not headers or not line.startswith("| `") or layer is None:
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        detector_id = cells[0].strip("`")
        detectors[detector_id] = {
            "layer": layer,
            "modes": cells[1:],
            "header_count": len(headers),
        }
    return detectors


def main() -> int:
    if not LANG_SUPPORT.is_file():
        print(f"ERROR: {LANG_SUPPORT} does not exist")
        return 1
    try:
        catalog = load_catalog()
    except Exception as error:  # subprocess/JSON errors must fail the release gate.
        print(f"ERROR: {error}")
        return 1
    documented = parse_matrix(LANG_SUPPORT.read_text(encoding="utf-8"))
    expected = {detector["id"]: detector for detector in catalog["detectors"]}
    failures: list[str] = []
    if set(documented) != set(expected):
        failures.append(
            "detector set mismatch: "
            f"missing={sorted(set(expected) - set(documented))}, "
            f"stale={sorted(set(documented) - set(expected))}"
        )
    for detector_id in sorted(set(documented) & set(expected)):
        doc = documented[detector_id]
        detector = expected[detector_id]
        if doc["layer"] != detector["layer"]:
            failures.append(
                f"{detector_id}: documented {doc['layer']}, catalog {detector['layer']}"
            )
        claims = {claim["language"]: claim["mode"] for claim in detector["language_support"]}
        modes = [claims[language] for language in LANGUAGES]
        if doc["header_count"] != len(LANGUAGES) or doc["modes"] != modes:
            failures.append(f"{detector_id}: language modes differ from executable catalog")
    if failures:
        print("Language-support verification failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(
        f"OK: {len(expected)} detectors and "
        f"{len(expected) * len(LANGUAGES)} language claims match the executable catalog."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

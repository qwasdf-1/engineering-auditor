#!/usr/bin/env python3
"""Print one version's CHANGELOG section, for release notes.

Usage: changelog_section.py <version> [changelog-path]

The section starts at the `## <version>` heading (optionally followed by
` - title`) and ends before the next `## ` heading. A missing or empty
section is an error, so a release never publishes without notes.
"""

from __future__ import annotations

import sys
from pathlib import Path

USAGE = "usage: changelog_section.py <version> [changelog-path]"


def section(text: str, version: str) -> str:
    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        heading = line.removeprefix("## ")
        if line.startswith("## ") and (heading == version or heading.startswith(f"{version} ")):
            start = index + 1
            break
    if start is None:
        raise LookupError(f"CHANGELOG has no `## {version}` section")
    end = next(
        (index for index in range(start, len(lines)) if lines[index].startswith("## ")),
        len(lines),
    )
    body = "\n".join(lines[start:end]).strip()
    if not body:
        raise LookupError(f"CHANGELOG section `## {version}` is empty")
    return body + "\n"


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(USAGE, file=sys.stderr)
        return 2
    path = Path(sys.argv[2]) if len(sys.argv) == 3 else Path("CHANGELOG.md")
    try:
        sys.stdout.write(section(path.read_text(encoding="utf-8"), sys.argv[1]))
    except LookupError as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

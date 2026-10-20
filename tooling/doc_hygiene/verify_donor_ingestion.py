#!/usr/bin/env python3
"""Verify that maintained donor references reflect the public donor manifest.

Checks:
  - Every donor in auditor-donors.lock.json has an entry in the tracked donor
    gap/parity references.
  - The check has no dependency on ignored donor checkouts or work records.
"""

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
LOCK_PATH = REPO_ROOT / "auditor-donors.lock.json"
DONOR_DOCS = (
    REPO_ROOT / "docs" / "reference" / "donor-gaps.md",
    REPO_ROOT / "docs" / "reference" / "donor-parity-spec.md",
)


def main() -> int:
    if not LOCK_PATH.exists():
        print(f"FAIL: {LOCK_PATH} not found")
        return 1
    missing_docs = [path for path in DONOR_DOCS if not path.exists()]
    if missing_docs:
        print(f"FAIL: missing donor docs: {', '.join(map(str, missing_docs))}")
        return 1

    with open(LOCK_PATH) as f:
        lock = json.load(f)
    lock_donor_ids = {d["id"] for d in lock.get("donors", [])}
    donor_text = "\n".join(path.read_text() for path in DONOR_DOCS)

    # Check every lock donor is in the ingestion doc
    missing_from_doc = []
    for donor_id in sorted(lock_donor_ids):
        if donor_id not in donor_text:
            missing_from_doc.append(donor_id)

    errors = 0

    if missing_from_doc:
        print(f"Donors in lock file but missing from maintained docs: {', '.join(missing_from_doc)}")
        errors += 1

    if errors == 0:
        print("OK: all donors tracked in maintained references")
    else:
        print(f"FAIL: {errors} error(s) found")

    return errors


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Validate detector catalog, ownership, emission, and capability structure."""

from __future__ import annotations

import sys

from detector_gate_contract import audit_detector_architecture


def main() -> int:
    result = audit_detector_architecture()
    print(
        f"catalog={result.catalog_count} owners={result.owner_count} "
        f"capabilities={result.capability_count} functions={result.function_count}"
    )
    if result.errors:
        for error in result.errors:
            print(f"ERROR: {error}")
        return 1
    print("detector architecture: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())

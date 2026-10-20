#!/usr/bin/env python3
"""Compliance checker for detector catalog and emission ownership.

Detector IDs are collected from the registry's spec! entries and production
Finding::new calls. Test modules are excluded because their constructed
findings are assertions, not runtime emission sites.

Violations flagged:
  - Detector ID emitted from >1 source file (cross-file duplication)
  - Category mismatch between emission sites
  - Category mismatch between emission site and registry
  - Detector ID in registry but never emitted
  - Detector ID emitted but not registered
"""

import re
import sys
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DETECTORS_DIR = REPO_ROOT / "crates" / "auditor-detectors" / "src"
REGISTRY_FILES = (DETECTORS_DIR / "registry.rs", DETECTORS_DIR / "registry_families.rs")

# IDs emitted by an owner-specific public entry point through a typed shared
# constructor. The compliance checker cannot regex-match these dynamic IDs, so
# this list documents the intentional constructor boundary.
DYNAMIC_DISPATCH = {
    "console_output_in_production",
    "dead_code",
    "debugger_in_production",
    "empty_catch_block",
    "orphan_symbol",
    "stale_link_reference",
    "stale_path_reference",
    "too_many_locals",
    "too_many_returns",
    "too_many_statements",
}

def parse_registry(filepath: Path) -> dict[str, str]:
    """Return {detector_id: category} from spec!(...) calls in the registry."""
    text = filepath.read_text(encoding="utf-8")
    registry: dict[str, str] = {}
    # Match: spec!("id", "category", ...)
    for m in re.finditer(
        r'spec!\(\s*"([^"]+)"\s*,\s*"([^"]+)"',
        text,
    ):
        detector_id = m.group(1)
        category = m.group(2)
        # First occurrence wins; warn on duplicates later
        if detector_id in registry and registry[detector_id] != category:
            print(
                f"ERROR: Duplicate registry entry for '{detector_id}' "
                f"with conflicting categories: '{registry[detector_id]}' vs '{category}'"
            )
        registry[detector_id] = category
    return registry


def record_emission(
    emissions: dict[str, list[dict]],
    detector_id: str,
    category: str,
    line_num: int,
    filepath: Path,
) -> None:
    emissions.setdefault(detector_id, []).append(
        {"category": category, "line": line_num, "file": filepath.name}
    )


def record_literal_emissions(
    text: str,
    emissions: dict[str, list[dict]],
    filepath: Path,
    pattern: str,
    fixed_category: str | None = None,
) -> None:
    for match in re.finditer(pattern, text):
        detector_id = match.group(1)
        category = fixed_category or match.group(2)
        line_num = text[: match.start()].count("\n") + 1
        record_emission(emissions, detector_id, category, line_num, filepath)


def parse_source_emissions(filepath: Path) -> dict[str, list[dict]]:
    """Return {detector_id: [{category, line, file}]} from finding constructor calls."""
    text = production_rust_source(filepath.read_text(encoding="utf-8"))
    emissions: dict[str, list[dict]] = {}
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'(?:Finding::new|\bfinding!)\(\s*"([^"]+)"\s*,\s*"([^"]+)"',
    )
    # Repository-graph owners share a typed constructor but still pass literal
    # detector IDs and categories at each owner-specific call site.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\bfinding\(\s*"([^"]+)"\s*,\s*"([^"]+)"',
    )
    # Count-bearing Layer 3 owners share a constructor whose category is fixed
    # by the module while retaining a literal detector ID at each call site.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\bcounted_finding\(\s*"([^"]+)"',
        "algorithmic",
    )
    # Layer 4 effect owners pass a literal detector ID through one typed
    # systems-economics constructor. Keep those owner call sites visible to
    # this independent checker without maintaining a duplicate ID allowlist.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\beffect_finding\(\s*[^,]+,\s*"([^"]+)"',
        "systems_economics",
    )
    # Grouped Layer 4 owners share a constructor whose category is fixed by the
    # module, while each owner still supplies its detector ID literally.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\bgrouped_finding\(\s*"([^"]+)"',
        "systems_economics",
    )
    # Lexical owners share a constructor whose category is fixed by the module.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\blexical_finding\(\s*"([^"]+)"',
        "lexical",
    )
    # Structural semantic owners declare their detector ID in a `FindingText`
    # constant passed to one `structural_finding` constructor.
    record_literal_emissions(
        text,
        emissions,
        filepath,
        r'\bFindingText\s*\{\s*detector:\s*"([^"]+)"',
        "structural",
    )
    for match in re.finditer(
        r'const\s+\w+\s*:\s*Rule\s*=\s*Rule\s*\{[^}]*?\bid:\s*"([^"]+)"',
        text,
        re.DOTALL,
    ):
        detector_id = match.group(1)
        line_num = text[: match.start()].count("\n") + 1
        record_emission(emissions, detector_id, "correctness", line_num, filepath)
    return emissions


def production_rust_source(text: str) -> str:
    """Discard the conventional inline test module from Rust source."""
    return text.split("\n#[cfg(test)]", maxsplit=1)[0]


def collect_emissions() -> dict[str, list[dict]]:
    all_emissions: dict[str, list[dict]] = defaultdict(list)
    for source_file in sorted(DETECTORS_DIR.glob("*.rs")):
        if source_file.stem.endswith(("_test", "_tests")):
            continue
        for detector_id, entries in parse_source_emissions(source_file).items():
            all_emissions[detector_id].extend(entries)
    return all_emissions


def check_cross_file_duplication(all_emissions: dict[str, list[dict]]) -> bool:
    failed = False
    for detector_id, entries in sorted(all_emissions.items()):
        source_files = {entry["file"] for entry in entries}
        if len(source_files) > 1:
            files_text = ", ".join(sorted(source_files))
            print(f"ERROR: '{detector_id}' emitted from multiple files: {files_text}")
            failed = True
    return failed


def check_emission_categories(all_emissions: dict[str, list[dict]]) -> bool:
    failed = False
    for detector_id, entries in sorted(all_emissions.items()):
        categories = {entry["category"] for entry in entries}
        if len(categories) > 1:
            details = "; ".join(
                f"{entry['file']}:{entry['line']} -> '{entry['category']}'"
                for entry in entries
            )
            print(f"ERROR: '{detector_id}' has inconsistent categories: {details}")
            failed = True
    return failed


def check_registry_categories(
    registry: dict[str, str], all_emissions: dict[str, list[dict]]
) -> bool:
    failed = False
    for detector_id, entries in sorted(all_emissions.items()):
        registry_category = registry.get(detector_id)
        if registry_category is None:
            continue
        for entry in entries:
            if entry["category"] != registry_category:
                print(
                    f"ERROR: '{detector_id}' emitted with category "
                    f"'{entry['category']}' at {entry['file']}:{entry['line']} "
                    f"but registry says '{registry_category}'"
                )
                failed = True
    return failed


def check_registered_emissions(
    registry: dict[str, str], all_emissions: dict[str, list[dict]]
) -> bool:
    failed = False
    for detector_id in sorted(registry):
        if detector_id in DYNAMIC_DISPATCH:
            continue
        if detector_id not in all_emissions:
            print(f"WARNING: '{detector_id}' registered but never emitted")
            failed = True
    return failed


def check_unregistered_emissions(
    registry: dict[str, str], all_emissions: dict[str, list[dict]]
) -> bool:
    failed = False
    for detector_id in sorted(all_emissions):
        if detector_id not in registry:
            file_hint = all_emissions[detector_id][0]["file"]
            print(f"ERROR: '{detector_id}' emitted at {file_hint} but not in registry")
            failed = True
    return failed


def main() -> int:
    registry = {
        detector_id: category
        for path in REGISTRY_FILES
        for detector_id, category in parse_registry(path).items()
    }
    all_emissions = collect_emissions()
    failed = any(
        (
            check_cross_file_duplication(all_emissions),
            check_emission_categories(all_emissions),
            check_registry_categories(registry, all_emissions),
            check_registered_emissions(registry, all_emissions),
            check_unregistered_emissions(registry, all_emissions),
        )
    )
    if not failed:
        print("OK: All detector IDs follow architecture conventions.")
    else:
        print("\nFAILED with exit code 1")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())

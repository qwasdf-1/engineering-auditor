"""Catalog and ownership checks for the detector architecture lint."""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

from detector_gate_source import Function, functions_in

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from tooling.ecosystem.label_precision import standing  # noqa: E402
SRC = ROOT / "crates/auditor-detectors/src"
# The core catalog table and the family tables it chains.
REGISTRY_FILES = (SRC / "registry.rs", SRC / "registry_families.rs")
DISPATCHER = SRC / "dispatcher.rs"
PRECISION = ROOT / "eval/ecosystem/precision.json"

FORBIDDEN_PUBLIC_BAGS = {
    "ast_complexity_findings",
    "best_practices_findings",
    "correctness_findings",
    "design_checker_findings",
    "errorprone_findings",
    "kotlin_correctness_findings",
    "language_specific_findings",
    "security_findings",
    "source_metric_findings",
    "systems_economics_findings",
}

CATALOG_RE = re.compile(
    r"spec!\(\s*"
    r'"(?P<id>[^"]+)",\s*'
    r'"(?P<category>[^"]+)",\s*'
    r"(?P<severity>[A-Za-z]+),\s*"
    r"(?P<profile>Gating|Full),\s*"
    r'"(?P<layer>L[0-9]+)",\s*'
    r"(?P<owner>[A-Za-z0-9_]+)\s*\)",
    re.MULTILINE,
)
EMISSION_RE = re.compile(
    r"Finding::new\(\s*"
    r'"(?P<id>[^"]+)"\s*,\s*'
    r'"(?P<category>[^"]+)"',
    re.MULTILINE,
)
CASE_RE = re.compile(r'CapabilityCase::new\(\s*"([^"]+)"')
# Other builder calls (`.with_roles(..)`) may precede it in the chain.
FULL_PROFILE_RE = re.compile(r"\A(?:\s*\.\w+\([^()]*\))*?\s*\.full_profile_in\(&\[(?P<languages>[^\]]*)\]\)")


@dataclass(frozen=True)
class CatalogEntry:
    detector_id: str
    category: str
    profile: str
    layer: str
    owner: str
    # Languages where a gating detector runs only in the `full` profile.
    full_profile_languages: tuple[str, ...] = ()


@dataclass(frozen=True)
class ArchitectureAudit:
    errors: list[str]
    catalog_count: int
    owner_count: int
    capability_count: int
    function_count: int


def catalog() -> list[CatalogEntry]:
    source = "\n".join(path.read_text(encoding="utf-8") for path in REGISTRY_FILES)
    return [
        CatalogEntry(
            detector_id=match.group("id"),
            category=match.group("category"),
            profile=match.group("profile").lower(),
            layer=match.group("layer"),
            owner=match.group("owner"),
            full_profile_languages=full_profile_languages(source[match.end():]),
        )
        for match in CATALOG_RE.finditer(source)
    ]


def full_profile_languages(after_spec: str) -> tuple[str, ...]:
    """The languages of a `.full_profile_in(&[..])` call chained onto a spec."""
    match = FULL_PROFILE_RE.match(after_spec)
    return tuple(re.findall(r'"([^"]+)"', match.group("languages"))) if match else ()


def validate_catalog(
    entries: list[CatalogEntry],
) -> tuple[list[str], dict[str, CatalogEntry], dict[str, str]]:
    errors: list[str] = []
    by_id: dict[str, CatalogEntry] = {}
    by_owner: dict[str, str] = {}
    for entry in entries:
        expected_owner = "".join(part.capitalize() for part in entry.detector_id.split("_"))
        if entry.owner != expected_owner:
            errors.append(
                f"owner {entry.owner} must be {expected_owner} for {entry.detector_id}"
            )
        if entry.detector_id in by_id:
            errors.append(f"duplicate catalog ID: {entry.detector_id}")
        by_id[entry.detector_id] = entry
        prior = by_owner.get(entry.owner)
        if prior is not None:
            errors.append(
                f"owner {entry.owner} is shared by {prior} and {entry.detector_id}"
            )
        by_owner[entry.owner] = entry.detector_id
    return errors, by_id, by_owner


def validate_scheduling(entries: list[CatalogEntry]) -> list[str]:
    errors: list[str] = []
    dispatcher_source = DISPATCHER.read_text(encoding="utf-8")
    for entry in entries:
        if f"DetectorOwner::{entry.owner}" not in dispatcher_source:
            errors.append(
                f"catalog owner {entry.owner} ({entry.detector_id}) is not scheduled"
            )
    return errors


def capability_case_ids() -> list[str]:
    case_ids: list[str] = []
    for path in sorted(SRC.glob("capability_cases_*.rs")):
        case_ids.extend(CASE_RE.findall(path.read_text(encoding="utf-8")))
    return case_ids


def validate_capabilities(case_ids: list[str], catalog_ids: set[str]) -> list[str]:
    errors: list[str] = []
    duplicate_cases = sorted({item for item in case_ids if case_ids.count(item) > 1})
    if duplicate_cases:
        errors.append(f"duplicate capability cases: {duplicate_cases}")
    missing_cases = sorted(catalog_ids - set(case_ids))
    extra_cases = sorted(set(case_ids) - catalog_ids)
    if missing_cases or extra_cases:
        errors.append(
            f"catalog/capability mismatch: missing={missing_cases} extra={extra_cases}"
        )
    return errors


def detector_functions() -> list[Function]:
    functions: list[Function] = []
    excluded = {
        "capability_tests.rs",
        "capability_cases_correctness.rs",
        "capability_cases_performance.rs",
        "capability_cases_structural.rs",
        "donor_parity_tests.rs",
        "lib_test.rs",
        "oracle_runner_tests.rs",
        "test_support.rs",
    }
    for path in sorted(SRC.glob("*.rs")):
        if path.name not in excluded and not path.name.endswith("_tests.rs"):
            functions.extend(functions_in(path))
    return functions


def validate_function_shape(function: Function, ids: set[str]) -> list[str]:
    errors: list[str] = []
    if len(ids) > 1:
        errors.append(
            f"mixed detector constructor {function.path.name}::{function.name} emits {sorted(ids)}"
        )
    if function.public and function.name in FORBIDDEN_PUBLIC_BAGS:
        errors.append(
            f"forbidden mixed detector entry point remains: {function.path.name}::{function.name}"
        )
    if "method != crate::AnalysisMethod::Ast" in function.body or (
        "method != AnalysisMethod::Ast" in function.body
    ):
        errors.append(f"bad token gate in {function.path.name}::{function.name}: method != Ast")
    if "method == crate::AnalysisMethod::Auto" in function.body or (
        "method == AnalysisMethod::Auto" in function.body
    ):
        errors.append(f"ambiguous Auto gate in {function.path.name}::{function.name}")
    return errors


def validate_emissions(
    function: Function,
    emitted: list[re.Match[str]],
    by_id: dict[str, CatalogEntry],
    emission_files: dict[str, set[str]],
) -> list[str]:
    errors: list[str] = []
    for match in emitted:
        detector_id = match.group("id")
        category = match.group("category")
        entry = by_id.get(detector_id)
        if entry is None:
            errors.append(
                f"unregistered emission {detector_id} in {function.path.name}::{function.name}"
            )
            continue
        if entry.category != category:
            errors.append(
                f"category mismatch for {detector_id}: catalog={entry.category} "
                f"emission={category} in {function.path.name}::{function.name}"
            )
        emission_files[detector_id].add(function.path.name)
    return errors


def validate_functions(
    functions: list[Function], by_id: dict[str, CatalogEntry]
) -> list[str]:
    errors: list[str] = []
    emission_files: dict[str, set[str]] = defaultdict(set)
    for function in functions:
        emitted = list(EMISSION_RE.finditer(function.body))
        ids = {match.group("id") for match in emitted}
        errors.extend(validate_function_shape(function, ids))
        errors.extend(validate_emissions(function, emitted, by_id, emission_files))
    for detector_id, paths in sorted(emission_files.items()):
        if len(paths) > 1:
            errors.append(
                f"cross-file detector construction for {detector_id}: {sorted(paths)}"
            )
    return errors


def validate_entry_points(
    entries: list[CatalogEntry], functions: list[Function]
) -> list[str]:
    paths_by_name: dict[str, list[str]] = defaultdict(list)
    for function in functions:
        if function.public:
            paths_by_name[function.name].append(function.path.name)
    errors: list[str] = []
    for entry in entries:
        expected = f"{entry.detector_id}_findings"
        paths = paths_by_name.get(expected, [])
        if len(paths) != 1:
            errors.append(
                f"detector {entry.detector_id} requires one public {expected} entry point; found {paths}"
            )
    return errors


def validate_precision(entries: list[CatalogEntry], table: dict) -> list[str]:
    """A gating detector must not fall below the precision bar overall or in any language.

    `table` is the `labels.py precision --json` snapshot. A language listed
    in `full_profile_in` runs only in the `full` profile, so it is exempt,
    and the overall standing is re-judged without it. Unverified detectors
    keep their profile; one whose corpus sample is too small to decide gates
    only with no live false positive where it gates (detector-promotion.md).
    """
    known = {entry.detector_id for entry in entries}
    errors = [f"precision snapshot names unknown detector: {detector}" for detector in sorted(set(table) - known)]
    for entry in entries:
        row = table.get(entry.detector_id)
        if entry.profile != "gating" or row is None:
            continue
        scoped = set(entry.full_profile_languages)
        below = [language for language, cell in row["by_language"].items()
                 if cell["standing"] == "below_bar" and language not in scoped]
        overall = gating_standing(row, scoped) if scoped else row["standing"]
        if overall == "below_bar" or below:
            errors.append(
                f"gating detector {entry.detector_id} is below the precision bar "
                f"({', '.join(below) or 'overall'}); demote it to full or scope out the language"
            )
        elif overall == "insufficient_sample" and (false_positives := gating_false_positives(row, scoped)):
            errors.append(
                f"gating detector {entry.detector_id} has {false_positives} live false positive(s) "
                "on an insufficient sample; fix them, or demote it to full or scope out the language"
            )
    return errors


def gating_false_positives(row: dict, scoped: set) -> int:
    """Live false positives in the languages the detector gates in."""
    if not scoped:
        return row.get("false_positive", 0)
    return sum(cell.get("false_positive", 0) for language, cell in row["by_language"].items()
               if language not in scoped)


def gating_standing(row: dict, scoped: set) -> str:
    """The detector's standing over the languages it still gates in."""
    cells = [cell for language, cell in row["by_language"].items() if language not in scoped]
    totals = [sum(cell.get(key, 0) for cell in cells)
              for key in ("true_positive", "false_positive", "unverified", "accepted")]
    return standing(*totals)[1]


def audit_detector_architecture() -> ArchitectureAudit:
    entries = catalog()
    errors, by_id, by_owner = validate_catalog(entries)
    errors.extend(validate_scheduling(entries))
    case_ids = capability_case_ids()
    errors.extend(validate_capabilities(case_ids, set(by_id)))
    functions = detector_functions()
    errors.extend(validate_functions(functions, by_id))
    errors.extend(validate_entry_points(entries, functions))
    if PRECISION.exists():
        errors.extend(validate_precision(entries, json.loads(PRECISION.read_text(encoding="utf-8"))))
    else:
        errors.append(f"missing precision snapshot: {PRECISION.relative_to(ROOT)}")
    return ArchitectureAudit(
        errors=errors,
        catalog_count=len(entries),
        owner_count=len(by_owner),
        capability_count=len(case_ids),
        function_count=len(functions),
    )

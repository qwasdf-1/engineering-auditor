#!/usr/bin/env python3
"""Generate the detector/language matrix from the executable Rust catalog."""

from __future__ import annotations

import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
LANG_SUPPORT = REPO_ROOT / "docs/reference/language-support.md"

LANGUAGES = [
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "kotlin",
    "csharp",
    "c",
    "cpp",
    "ruby",
    "julia",
    "shell",
    "markdown",
    "structured",
]

DISPLAY = {
    "rust": "Rust",
    "python": "Python",
    "javascript": "JavaScript",
    "typescript": "TypeScript",
    "go": "Go",
    "java": "Java",
    "kotlin": "Kotlin",
    "csharp": "C#",
    "c": "C",
    "cpp": "C++",
    "ruby": "Ruby",
    "julia": "Julia",
    "shell": "Shell",
    "markdown": "Markdown",
    "structured": "Structured",
}

LAYER_INFO = {
    "L1": ("Layer 1 - Correctness", "Correctness and security defects."),
    "L2": ("Layer 2 - Structural Engineering", "Structure, complexity, duplication, and maintainability."),
    "L3": ("Layer 3 - Algorithmic Engineering", "Algorithmic cost and repeated-work patterns."),
    "L4": ("Layer 4 - Systems Engineering", "Repository and systems-economics behavior."),
    "L5": ("Layer 5 - Optimization Opportunities", "Bounded optimization opportunities."),
}


def load_catalog() -> dict:
    result = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "auditor-cli",
            "--",
            "catalog",
            "--profile",
            "full",
            "--format",
            "json",
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "auditor catalog failed")
    return json.loads(result.stdout)


def table_header() -> str:
    names = " | ".join(DISPLAY[language] for language in LANGUAGES)
    return f"| Detector | {names} |\n|---" + "|---" * len(LANGUAGES) + "|"


def detector_row(detector: dict) -> str:
    claims = {
        claim["language"]: claim["mode"]
        for claim in detector["language_support"]
    }
    if set(claims) != set(LANGUAGES):
        missing = sorted(set(LANGUAGES) - set(claims))
        extra = sorted(set(claims) - set(LANGUAGES))
        raise ValueError(
            f"{detector['id']} language contract mismatch: missing={missing}, extra={extra}"
        )
    cells = " | ".join(claims[language] for language in LANGUAGES)
    return f"| `{detector['id']}` | {cells} |"


def coverage_matrix(detectors: list[dict]) -> list[str]:
    sections: list[str] = []
    for layer, (title, description) in LAYER_INFO.items():
        layer_detectors = sorted(
            (detector for detector in detectors if detector["layer"] == layer),
            key=lambda detector: detector["id"],
        )
        if not layer_detectors:
            continue
        sections.extend([f"### {title}", "", description, "", table_header()])
        sections.extend(detector_row(detector) for detector in layer_detectors)
        sections.append("")
    return sections


def source_coverage_summary(
    detectors: list[dict], source_languages: list[str]
) -> list[str]:
    sections = [
        "## Source Coverage Summary",
        "",
        "Markdown and structured-data cells are excluded from these source-language totals.",
        "",
        "| Layer | Semantic | Exact lexical | Partial | Unsupported | Not applicable |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for layer in LAYER_INFO:
        counts = defaultdict(int)
        for detector in (item for item in detectors if item["layer"] == layer):
            claims = {
                claim["language"]: claim for claim in detector["language_support"]
            }
            for language in source_languages:
                counts[claims[language]["mode"]] += 1
        sections.append(
            f"| {layer} | {counts['semantic']} | {counts['exact_lexical']} | "
            f"{counts['partial']} | {counts['unsupported']} | {counts['not_applicable']} |"
        )
    return sections


def deferred_source_capabilities(
    detectors: list[dict], source_languages: list[str]
) -> list[str]:
    deferred = defaultdict(lambda: {"pairs": 0, "detectors": set(), "languages": set()})
    sources = set(source_languages)
    for detector in detectors:
        for claim in detector["language_support"]:
            if claim["language"] not in sources or claim["mode"] != "unsupported":
                continue
            reason = claim["limitation"]
            deferred[reason]["pairs"] += 1
            deferred[reason]["detectors"].add(detector["id"])
            deferred[reason]["languages"].add(claim["language"])

    sections = [
        "",
        "## Deferred Source Capabilities",
        "",
        "Each unsupported catalog cell carries one of these executable limitation reasons.",
        "",
        "| Missing capability | Pairs | Detector families | Languages |",
        "|---|---:|---:|---|",
    ]
    for reason, summary in sorted(deferred.items()):
        languages = ", ".join(DISPLAY[item] for item in sorted(summary["languages"]))
        sections.append(
            f"| {reason} | {summary['pairs']} | {len(summary['detectors'])} | {languages} |"
        )
    return sections



def not_applicable_source_capabilities(
    detectors: list[dict], source_languages: list[str]
) -> list[str]:
    reasons = defaultdict(lambda: {"detectors": set(), "languages": set()})
    sources = set(source_languages)
    for detector in detectors:
        for claim in detector["language_support"]:
            if claim["language"] not in sources or claim["mode"] != "not_applicable":
                continue
            reasons[claim["limitation"]]["detectors"].add(detector["id"])
            reasons[claim["limitation"]]["languages"].add(claim["language"])

    sections = [
        "",
        "## Not-Applicable Source Capabilities",
        "",
        "Each not-applicable source cell carries one of these reviewed reasons.",
        "",
        "| Reason | Detectors | Languages |",
        "|---|---|---|",
    ]
    for reason, summary in sorted(reasons.items()):
        names = ", ".join(f"`{name}`" for name in sorted(summary["detectors"]))
        languages = ", ".join(DISPLAY[item] for item in sorted(summary["languages"]))
        sections.append(f"| {reason} | {names} | {languages} |")
    return sections


def render(catalog: dict) -> str:
    detectors = catalog["detectors"]
    sections = [
        "# Language Support Reference",
        "",
        "This matrix is generated from the executable detector catalog. The Rust registry",
        "owns detector identity, emission and context roles, and every detector-language",
        "support mode; this document does not apply layer-wide defaults.",
        "",
        "Support modes:",
        "",
        "- `semantic`: grammar-backed semantic facts are part of the capability.",
        "- `exact_lexical`: a bounded language-aware lexical rule is the capability.",
        "- `partial`: a tested heuristic exists, with a declared limitation.",
        "- `unsupported`: no executable capability is claimed for the pair.",
        "- `not_applicable`: the capability does not exist for the language; each cell carries a reviewed reason.",
        "",
        "Language tiers:",
        "",
        "| Tier | Languages | Promise |",
        "|---|---|---|",
        "| 1 | Rust, Python, JavaScript, TypeScript, Go, Java | Every applicable detector family is supported or carries a reviewed limitation; precision is measured on the ecosystem corpus. |",
        "| 2 | Kotlin, C#, C, C++ | Layers 1 and 2 at the Tier-1 standard; Layers 3 to 5 are best-effort and documented per cell. |",
        "| 3 | Ruby, Julia, Shell | Structural metrics only (complexity, Halstead density, file size, and Ruby `!!`); no other capability is claimed. |",
        "",
        "The catalog is available as JSON with `auditor catalog --profile full --format json`.",
        "Regenerate this page with `python3 tooling/doc_hygiene/generate_language_support.py`.",
        "",
        "## Coverage Matrix",
        "",
    ]
    source_languages = LANGUAGES[:-2]
    sections.extend(coverage_matrix(detectors))
    sections.extend(source_coverage_summary(detectors, source_languages))
    sections.extend(not_applicable_source_capabilities(detectors, source_languages))
    sections.extend(deferred_source_capabilities(detectors, source_languages))

    sections.extend(
        [
            "## Role Contracts",
            "",
            "`emission_roles` authorize finding locations. `context_roles` authorize read-only",
            "repository observation, such as resolving a production symbol referenced by a test.",
            "Both are emitted by the executable catalog and covered by registry tests.",
            "",
        ]
    )
    return "\n".join(sections)


def main() -> int:
    try:
        catalog = load_catalog()
        LANG_SUPPORT.write_text(render(catalog), encoding="utf-8")
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(f"Generated {LANG_SUPPORT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

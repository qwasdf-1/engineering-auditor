"""Seeded recall cases for the algorithmic, systems, and optimization layers.

Each case is a small repository with one known problem, written in one of
four shapes:

- ``direct``: the expensive operation sits in the loop (or async function).
- ``helper``: it sits in a same-file helper the loop calls.
- ``cross_file``: that helper is imported from another module.
- ``negative``: the work is hoisted or batched; the detector must stay silent.

The ``direct`` sources follow the detectors' capability examples, so a miss
there is a regression rather than a gap.
"""

from __future__ import annotations

from dataclasses import dataclass

from tooling.recall.cases_go import GO
from tooling.recall.cases_java import JAVA
from tooling.recall.cases_python import PYTHON
from tooling.recall.cases_rust import RUST
from tooling.recall.cases_typescript import TYPESCRIPT

SHAPES = ("direct", "helper", "cross_file", "negative")


@dataclass(frozen=True)
class Case:
    detector: str
    language: str
    shape: str
    files: dict[str, str]

    @property
    def id(self) -> str:
        return f"{self.detector}/{self.language}/{self.shape}"

    @property
    def positive(self) -> bool:
        return self.shape != "negative"


# Per detector and language: the four shapes. A cross-file case is a pair of
# files; the others are one file.
SOURCES = {
    "rust": RUST,
    "python": PYTHON,
    "typescript": TYPESCRIPT,
    "go": GO,
    "java": JAVA,
}


def all_cases() -> list[Case]:
    cases = []
    for language, detectors in SOURCES.items():
        for detector, shapes in detectors.items():
            for shape in SHAPES:
                cases.append(Case(detector, language, shape, shapes[shape]))
    return cases

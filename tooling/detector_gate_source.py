"""Rust source-boundary helpers for the detector architecture lint."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

FUNCTION_RE = re.compile(
    r"(?m)^(?P<public>pub\(crate\)\s+)?fn\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\("
)
TEST_MODULE_RE = re.compile(r"(?m)^#\[cfg\(test\)\]\s*\nmod tests\s*\{")


@dataclass(frozen=True)
class Function:
    path: Path
    name: str
    public: bool
    start: int
    end: int
    body: str


def production_source(path: Path) -> str:
    source = path.read_text(encoding="utf-8")
    scrubbed = scrub_for_braces(source)
    output = list(source)
    search_from = 0
    while test_module := TEST_MODULE_RE.search(scrubbed, search_from):
        opening = scrubbed.find("{", test_module.start())
        end = matching_brace_end(scrubbed, opening)
        blank_non_newlines(output, test_module.start(), end)
        search_from = end
    return "".join(output)


def matching_brace_end(source: str, opening: int) -> int:
    depth = 0
    for cursor in range(opening, len(source)):
        if source[cursor] == "{":
            depth += 1
        elif source[cursor] == "}":
            depth -= 1
            if depth == 0:
                return cursor + 1
    return len(source)


def line_comment_end(source: str, index: int) -> int | None:
    if not source.startswith("//", index):
        return None
    end = source.find("\n", index)
    return len(source) if end < 0 else end


def block_comment_end(source: str, index: int) -> int | None:
    if not source.startswith("/*", index):
        return None
    end = source.find("*/", index + 2)
    return len(source) if end < 0 else end + 2


def raw_string_end(source: str, index: int) -> int | None:
    if source[index] != "r" or index + 1 >= len(source):
        return None
    cursor = index + 1
    while cursor < len(source) and source[cursor] == "#":
        cursor += 1
    if cursor >= len(source) or source[cursor] != '"':
        return None
    closing = '"' + "#" * (cursor - index - 1)
    end = source.find(closing, cursor + 1)
    return len(source) if end < 0 else end + len(closing)


def quoted_literal_end(source: str, index: int) -> int | None:
    if source[index] not in {'"', "'"}:
        return None
    if source[index] == "'" and source[index + 1 : index + 2] != "\\":
        return index + 3 if source[index + 2 : index + 3] == "'" else None
    quote = source[index]
    cursor = index + 1
    escaped = False
    while cursor < len(source):
        char = source[cursor]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == quote:
            return cursor + 1
        cursor += 1
    return len(source)


def ignored_span_end(source: str, index: int) -> int | None:
    for finder in (line_comment_end, block_comment_end, raw_string_end, quoted_literal_end):
        end = finder(source, index)
        if end is not None:
            return end
    return None


def blank_non_newlines(output: list[str], start: int, end: int) -> None:
    for position in range(start, end):
        if output[position] != "\n":
            output[position] = " "


def scrub_for_braces(source: str) -> str:
    """Replace Rust comments and literals while preserving byte positions."""
    output = list(source)
    index = 0
    while index < len(source):
        end = ignored_span_end(source, index)
        if end is None:
            index += 1
            continue
        blank_non_newlines(output, index, end)
        index = end
    return "".join(output)


def functions_in(path: Path) -> list[Function]:
    source = production_source(path)
    scrubbed = scrub_for_braces(source)
    functions: list[Function] = []
    for match in FUNCTION_RE.finditer(scrubbed):
        opening = scrubbed.find("{", match.end())
        if opening < 0:
            continue
        end = matching_brace_end(scrubbed, opening)
        functions.append(
            Function(
                path=path,
                name=match.group("name"),
                public=bool(match.group("public")),
                start=match.start(),
                end=end,
                body=source[match.start() : end],
            )
        )
    return functions

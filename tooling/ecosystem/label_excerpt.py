"""Source excerpts for labelling bundles."""

from tooling.ecosystem import run

EXCERPT_BEFORE, EXCERPT_AFTER = 5, 15
# A function-level finding shows the whole function, up to this many lines.
FUNCTION_EXCERPT_MAX = 120


def indent(line) -> int:
    return len(line) - len(line.lstrip())


def function_end(lines, anchor) -> int:
    """The index just past the function whose header starts at `anchor`.

    Brace languages end where the braces opened after the header balance;
    Python ends at the first line indented no deeper than the `def` once the
    header's `:` is seen. Capped at FUNCTION_EXCERPT_MAX lines.
    """
    limit = min(anchor + FUNCTION_EXCERPT_MAX, len(lines))
    depth, opened, header_end = 0, False, None
    for number in range(anchor, limit):
        line = lines[number]
        depth += line.count("{") - line.count("}")
        opened = opened or "{" in line
        if opened and depth <= 0:
            return number + 1
        if header_end is None and not opened and line.rstrip().endswith(":"):
            header_end = number
        if header_end is not None and number > header_end and line.strip():
            # The decorator or `def` line, not a continued parameter line.
            if indent(line) <= min(indent(lines[n]) for n in range(anchor, header_end + 1)):
                return number
    return limit


def excerpt(repository_id, entry, whole_function=False) -> str:
    source = run.cache_root() / "repos" / repository_id / entry["path"]
    if not entry["path"] or not source.is_file() or "line" not in entry:
        return ""
    lines = source.read_text(encoding="utf-8", errors="replace").splitlines()
    start = max(entry["line"] - 1 - EXCERPT_BEFORE, 0)
    end = min(entry["line"] + EXCERPT_AFTER, len(lines))
    if whole_function:
        end = max(end, function_end(lines, entry["line"] - 1))
    return "\n".join(f"{number + 1:>5}{'>' if number + 1 == entry['line'] else ' '} {lines[number]}"
                     for number in range(start, end))

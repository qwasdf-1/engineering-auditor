"""Judges that compare an oracle's facts with one labelled finding's claim.

A judge returns ("confirmed" | "refuted", detail) or None to abstain. Budgets
mirror EA's thresholds, with refutation margins where tools count slightly
differently.
"""

import re
from dataclasses import dataclass

from tooling.ecosystem import run


def short_name(name: str) -> str:
    return re.split(r"::|\.", name or "")[-1].strip("()* ")


def evidence(finding) -> dict:
    value = finding.get("evidence")
    return value if isinstance(value, dict) else {}


def evidence_lines(finding, key="lines") -> list:
    return [int(line) for line in evidence(finding).get(key, []) if str(line).isdigit()]


ANONYMOUS = ("", "<anonymous>", "(anonymous)", "<anon>")


def nearest(candidates, line, window):
    placed = [fact for fact in candidates if fact.get("start") is not None]
    best = min(placed, key=lambda fact: abs(fact["start"] - line), default=None)
    return best if best is not None and abs(best["start"] - line) <= window else None


def match_function(facts, name, line):
    """The function fact for a finding: same short name nearest its line, or
    for an anonymous function the fact starting at that line. EA names an
    arrow function bound to a constant by its binding, which other tools
    leave anonymous, so a named finding falls back to an anonymous fact
    starting on its line."""
    name = short_name(name)
    candidates = [fact for fact in facts if short_name(fact["name"]) == name] if name else facts
    if line is None:
        return candidates[0] if name and len(candidates) == 1 else None
    found = nearest(candidates, line, 5 if name else 1)
    if found is None and name:
        found = nearest([fact for fact in facts if fact["name"] in ANONYMOUS], line, 0)
    if found is None and name and not any(fact.get("start") is not None for fact in candidates):
        found = candidates[0] if len(candidates) == 1 else None
    return found


@dataclass(frozen=True)
class Budget:
    """A detector's budget as an oracle applies it: confirm over `threshold`,
    refute at or below `refute_at`, no opinion in between."""
    threshold: float
    refute_at: float
    evidence_key: str  # the finding's own measurement, shown beside the oracle's


# EA's budgets (cognitive.rs, structural_metrics.rs), with refutation margins
# for metrics whose counting rules differ slightly between tools.
BRANCHES = Budget(10, 8, "branch_count")
COGNITIVE = Budget(15, 12, "cognitive_complexity")
NPATH = Budget(200, 160, "npath")
PARAMETERS = Budget(5, 5, "parameter_count")
RETURNS = Budget(5, 5, "return_count")
HALSTEAD_VOLUME, HALSTEAD_DIFFICULTY, HALSTEAD_DIFFICULTY_ALONE = 1500, 15, 30
HALSTEAD_MARGIN = 0.8  # refute only when clear of both rules by a fifth
SIGNATURE_WINDOW = (1, 8)  # ruff reports a function rule on its `def` line
VALUE_WINDOW = (1, 3)


def compare(value, budget, detail):
    if value > budget.threshold:
        return "confirmed", detail
    if value <= budget.refute_at:
        return "refuted", detail
    return None


def show(value) -> str:
    return f"{value:g}" if isinstance(value, (int, float)) else str(value)


def nested_decisions(fact, facts):
    """EA counts the decisions of closures nested in a function toward it, so
    sum lizard's per-function decisions (CCN - 1) over the matched span."""
    if fact.get("end") is None:
        return None
    return sum(other["metrics"]["ccn"] - 1 for other in facts
               if fact["start"] <= other["start"] and other["end"] <= fact["end"])


def rca_decisions(fact, facts):
    """rust-code-analysis's cyclomatic sum covers nested closures and adds one
    base path per function space, so subtract one per space in the span."""
    spaces = sum(1 for other in facts if fact["start"] <= other["start"] and other["end"] <= fact["end"])
    return fact["metrics"]["cyclomatic"] - spaces


def metric_judge(tool, metric, budget, measure=None):
    """Confirm when the oracle's value for the same function is over the
    budget; refute when it is clearly under. `measure(fact, facts)` derives
    the value; by default it is the function's own `metric`."""
    def judge(entry, finding, facts):
        fact = match_function(facts, evidence(finding).get("function", ""), entry.get("line"))
        value = None if fact is None else (measure(fact, facts) if measure else fact["metrics"].get(metric))
        if value is None:
            return None
        return compare(value, budget, f"{tool} {metric} {show(value)} vs budget {show(budget.threshold)} "
                                      f"(EA {show(evidence(finding).get(budget.evidence_key))})")
    return judge


def over_halstead(volume, difficulty, scale=1.0) -> bool:
    return ((volume > HALSTEAD_VOLUME * scale and difficulty >= HALSTEAD_DIFFICULTY * scale)
            or difficulty > HALSTEAD_DIFFICULTY_ALONE * scale)


def halstead_judge(entry, finding, facts):
    """EA's rule: volume over budget with enough difficulty, or difficulty alone."""
    fact = match_function(facts, evidence(finding).get("function", ""), entry.get("line"))
    if fact is None:
        return None
    volume, difficulty = fact["metrics"]["halstead_volume"], fact["metrics"]["halstead_difficulty"]
    detail = (f"rca volume {volume:.0f}, difficulty {difficulty:.1f} (EA {show(evidence(finding).get('halstead_volume'))}, "
              f"{evidence(finding).get('halstead_difficulty', 0):.1f})")
    if over_halstead(volume, difficulty):
        return "confirmed", detail
    if not over_halstead(volume, difficulty, HALSTEAD_MARGIN):
        return "refuted", detail
    return None


def diagnostic_judge(tool, rules, window=(0, 0), lines_from=None, refutes=True):
    """Confirm when the oracle reports one of `rules` at the finding's line(s)
    (or a window after it); refute when it analyzed the file and did not."""
    def judge(entry, finding, facts):
        lines = evidence_lines(finding, lines_from) if lines_from else []
        lines = lines or [entry.get("line")]
        if lines == [None]:
            return None
        hits = [fact for fact in facts if fact["rule"] in rules
                and any(line - window[0] <= fact["line"] <= line + window[1] for line in lines)]
        where = ", ".join(map(str, lines))
        if hits:
            value = f" ({show(hits[0]['value'])})" if hits[0].get("value") is not None else ""
            return "confirmed", f"{tool} {hits[0]['rule']}{value} at line {hits[0]['line']}"
        return ("refuted", f"{tool} reports no {'/'.join(rules)} at line {where}") if refutes else None
    return judge


def valued_diagnostic_judge(tool, rule, budget, by_name=False):
    """A diagnostic that carries the measured value. The oracle reports every
    function (threshold 0 or 1), so a matched value can refute; silence (a
    location the oracle did not report) is no opinion."""
    def judge(entry, finding, facts):
        line = entry.get("line")
        if line is None:
            return None
        name = short_name(evidence(finding).get("function", ""))
        hits = [fact for fact in facts if fact["rule"] == rule and fact.get("value") is not None
                and (short_name(fact.get("name", "")) == name if by_name and name
                     else line - VALUE_WINDOW[0] <= fact["line"] <= line + VALUE_WINDOW[1])]
        if by_name and hits:
            hits = [min(hits, key=lambda fact: abs(fact["line"] - line))]
            if abs(hits[0]["line"] - line) > 40:
                return None
        if not hits:
            return None
        value = max(hit["value"] for hit in hits)
        return compare(value, budget, f"{tool} {rule} {show(value)} vs budget {show(budget.threshold)} "
                                      f"(EA {show(evidence(finding).get(budget.evidence_key))})")
    return judge


def nesting_judge(entry, finding, facts):
    """eslint max-depth reports blocks deeper than 5; silence is no opinion
    because its block depth starts from the function body, not EA's."""
    line = evidence(finding).get("deepest_line")
    hits = [fact for fact in facts if fact["rule"] == "max-depth" and line is not None
            and abs(fact["line"] - line) <= 2]
    if hits:
        return "confirmed", f"eslint max-depth ({show(hits[0]['value'])}) at line {hits[0]['line']}"
    return None


def goconst_judge(entry, finding, facts):
    literal = evidence(finding).get("literal")
    if literal is None:
        return None
    hits = [fact for fact in facts if fact["message"] == literal]
    if hits:
        return "confirmed", f"goconst reports `{literal}` repeated in this file (line {hits[0]['line']})"
    # goconst skips some literal contexts (format verbs, call arguments), so
    # its silence is no refutation.
    return None


def large_file_judge(entry, finding, facts):
    if not facts or evidence(finding).get("advisory_threshold") is None:
        return None
    lines, threshold = facts[0]["lines"], evidence(finding)["advisory_threshold"]
    return compare(lines, Budget(threshold, threshold, "lines"), f"{lines} nonblank lines vs threshold {threshold} "
                                                f"(EA {show(evidence(finding).get('lines'))})")


def occurrence_ranges(finding) -> list:
    ranges = []
    for occurrence in evidence(finding).get("occurrences", []):
        match = re.match(r"(.+):(\d+):\d+-(\d+):\d+$", occurrence)
        if match:
            ranges.append((match.group(1), int(match.group(2)), int(match.group(3))))
    return ranges


def overlaps(left, right) -> bool:
    return left[0] == right[0] and left[1] <= right[2] and right[1] <= left[2]


def clone_judge(entry, finding, facts):
    """Confirm when jscpd reports a duplicate pair overlapping two of the
    clone class's occurrences. jscpd tokenizes differently, so silence is no
    refutation."""
    ranges = occurrence_ranges(finding)
    for first, second in facts:
        hit_first = [r for r in ranges if overlaps(first, r)]
        hit_second = [r for r in ranges if overlaps(second, r)]
        if hit_first and hit_second and (len(set(hit_first + hit_second)) >= 2):
            return "confirmed", (f"jscpd duplicate {first[0]}:{first[1]}-{first[2]} ~ "
                                 f"{second[0]}:{second[1]}-{second[2]}")
    return None


def clone_paths(finding) -> list:
    return sorted({path for path, _, _ in occurrence_ranges(finding)})


def es_imports_only(entry, finding) -> bool:
    """eslint no-duplicate-imports sees ES `import` statements, not require()."""
    source = run.cache_root() / "repos" / entry["repository"] / entry["path"]
    lines = source.read_text(encoding="utf-8", errors="replace").splitlines() if source.is_file() else []
    numbers = evidence_lines(finding) or [entry.get("line") or 0]
    return all(0 < n <= len(lines) and lines[n - 1].lstrip().startswith("import") for n in numbers)

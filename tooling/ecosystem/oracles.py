#!/usr/bin/env python3
"""Check labelled corpus findings against independent reference tools.

Commands:
  provision          install the pinned oracle tools under $EA_ORACLE_ROOT
                     (default target/oracles); needs network access
  run --scan L [--tool T..] [--write]
                     run the oracles over every labelled finding present in
                     scan L, print per-detector agreement, and with --write
                     record the checks in the label files
  overlap --scan L [--tool T..] [--json F]
                     run the oracles over every finding in scan L, labelled
                     or not, and print per detector and language how many
                     findings an oracle confirms, refutes, or cannot judge;
                     --json writes the per-finding claims for stratified
                     sampling (agreed findings are spot-checked, refuted
                     ones reviewed)

An oracle check says whether a reference tool confirms or refutes the factual
claim of one finding: the function's measured complexity is over the budget,
the linter flags the same call, the other clone detector finds the same
clone. It does not judge scope or idiom (test code, a deliberate lazy import),
so a confirmed claim never overturns a false-positive label; see
label_store.check_agreement. Oracles run only on the files that hold labelled
findings, in the pinned corpus checkouts, with inline suppressions ignored.
"""

import argparse
import json
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tooling.ecosystem import label_store, label_workflow, run  # noqa: E402
from tooling.ecosystem.oracle_judges import (  # noqa: E402
    BRANCHES,
    COGNITIVE,
    NPATH,
    PARAMETERS,
    RETURNS,
    SIGNATURE_WINDOW,
    clone_judge,
    clone_paths,
    diagnostic_judge,
    es_imports_only,
    goconst_judge,
    halstead_judge,
    large_file_judge,
    metric_judge,
    nested_decisions,
    nesting_judge,
    rca_decisions,
    valued_diagnostic_judge,
)
from tooling.ecosystem.oracle_overlap import live_entries, overlap, overlap_markdown  # noqa: E402
from tooling.ecosystem.oracle_tools import COLLECTORS, JS_LANGUAGES, provision, versions  # noqa: E402


@dataclass(frozen=True)
class Rule:
    detector: str
    languages: frozenset
    tool: str
    judge: object
    paths: object = None  # finding -> extra files the tool must see
    applies: object = None  # (entry, finding) -> whether the tool can see this claim


def rules() -> list:
    every = frozenset({"python", "rust", "java", "typescript", "javascript", "go"})
    py, java, go = frozenset({"python"}), frozenset({"java"}), frozenset({"go"})
    tier_two, c_family = frozenset({"kotlin", "csharp", "c", "cpp"}), frozenset({"c", "cpp"})
    return [
        # lizard mis-spans TypeScript arrow-function constants and typed signatures.
        Rule("function_complexity", (every - JS_LANGUAGES) | tier_two, "lizard",
             metric_judge("lizard", "decisions incl. nested functions", BRANCHES, nested_decisions)),
        Rule("function_complexity", JS_LANGUAGES, "rca",
             metric_judge("rca", "decisions incl. nested functions", BRANCHES, rca_decisions)),
        Rule("too_many_parameters", py, "ruff", diagnostic_judge("ruff", ("PLR0913",), window=SIGNATURE_WINDOW)),
        Rule("too_many_parameters", (every - py) | tier_two, "lizard", metric_judge("lizard", "params", PARAMETERS)),
        Rule("too_many_returns", py, "ruff", diagnostic_judge("ruff", ("PLR0911",), window=SIGNATURE_WINDOW)),
        Rule("too_many_returns", frozenset({"rust", "java", "typescript", "javascript"}) | c_family, "rca",
             metric_judge("rca", "nexits", RETURNS)),
        Rule("too_many_statements", py, "ruff", diagnostic_judge("ruff", ("PLR0915",), window=SIGNATURE_WINDOW)),
        # ruff counts a subset of EA's names (no loop or `with` targets, no
        # `__import__` calls), so its silence cannot refute.
        Rule("too_many_locals", py, "ruff",
             diagnostic_judge("ruff", ("PLR0914",), window=SIGNATURE_WINDOW, refutes=False)),
        Rule("cognitive_complexity", py, "complexipy", metric_judge("complexipy", "cognitive", COGNITIVE)),
        Rule("cognitive_complexity", go, "gocognit", metric_judge("gocognit", "cognitive", COGNITIVE)),
        Rule("cognitive_complexity", JS_LANGUAGES, "eslint",
             valued_diagnostic_judge("eslint", "sonarjs/cognitive-complexity", COGNITIVE)),
        Rule("cognitive_complexity", java, "pmd",
             valued_diagnostic_judge("pmd", "CognitiveComplexity", COGNITIVE, by_name=True)),
        # rca leaves C/C++ conditional expressions out of cognitive complexity.
        Rule("cognitive_complexity", frozenset({"rust"}), "rca",
             metric_judge("rca", "cognitive", COGNITIVE)),
        Rule("npath_complexity", java, "pmd",
             valued_diagnostic_judge("pmd", "NPathComplexity", NPATH, by_name=True)),
        Rule("halstead_information_density", (every - go) | c_family, "rca", halstead_judge),
        Rule("excessive_nesting_depth", JS_LANGUAGES, "eslint", nesting_judge),
        Rule("large_file", every | {"kotlin", "csharp", "c", "cpp"}, "nonblank", large_file_judge),
        Rule("assert_in_production", py, "ruff", diagnostic_judge("ruff", ("S101",))),
        Rule("console_output_in_production", py, "ruff", diagnostic_judge("ruff", ("T201", "T203"))),
        Rule("console_output_in_production", JS_LANGUAGES, "eslint", diagnostic_judge("eslint", ("no-console",))),
        Rule("function_local_import", py, "ruff", diagnostic_judge("ruff", ("PLC0415",), lines_from="lines", refutes=False)),
        Rule("duplicate_import", JS_LANGUAGES, "eslint",
             diagnostic_judge("eslint", ("no-duplicate-imports",), lines_from="lines"), applies=es_imports_only),
        Rule("empty_catch_block", py, "ruff", diagnostic_judge("ruff", ("S110",))),
        Rule("empty_catch_block", JS_LANGUAGES, "eslint", diagnostic_judge("eslint", ("no-empty",))),
        Rule("no_await_in_loop", JS_LANGUAGES, "eslint", diagnostic_judge("eslint", ("no-await-in-loop",))),
        Rule("shell_execution", py, "ruff", diagnostic_judge("ruff", ("S602", "S604", "S605"))),
        Rule("random_for_security", py, "ruff", diagnostic_judge("ruff", ("S311",))),
        # B113 covers `requests` and `httpx`; `urlopen` is beyond it.
        Rule("network_call_without_timeout", py, "bandit", diagnostic_judge("bandit", ("B113",)),
             applies=lambda entry, finding: "urlopen" not in finding["message"]),
        Rule("naming_convention", java, "pmd",
             diagnostic_judge("pmd", ("FieldNamingConventions", "LocalVariableNamingConventions",
                                      "MethodNamingConventions", "ClassNamingConventions"))),
        Rule("go_errcheck", go, "errcheck", diagnostic_judge("errcheck", ("errcheck",))),
        Rule("go_repeated_string_literal", go, "goconst", goconst_judge),
        Rule("source_duplicate_detection", every, "jscpd", clone_judge, paths=clone_paths),
    ]


# Running -----------------------------------------------------------------------------------

def plan(label_sets, reports, tools=None) -> list:
    """[(rule, entry, finding)] for every labelled finding still in the scan."""
    findings = {(repository, finding["id"]): finding
                for repository, report in reports.items() for finding in report["findings"]}
    planned = []
    for rule in rules():
        if tools and rule.tool not in tools:
            continue
        for entry in label_sets.get(rule.detector, []):
            finding = findings.get((entry["repository"], entry["id"]))
            if (finding is not None and entry["language"] in rule.languages and entry["path"]
                    and (rule.applies is None or rule.applies(entry, finding))):
                planned.append((rule, entry, finding))
    return planned


def collect(planned) -> tuple:
    """Run each tool once per repository over the files it must see."""
    wanted = defaultdict(set)
    for rule, entry, finding in planned:
        wanted[(rule.tool, entry["repository"])].add(entry["path"])
        if rule.paths:
            wanted[(rule.tool, entry["repository"])].update(rule.paths(finding))
    facts, failures = {}, {}
    for (tool, repository), paths in sorted(wanted.items()):
        checkout = run.cache_root() / "repos" / repository
        present = sorted(path for path in paths if (checkout / path).is_file())
        try:
            facts[(tool, repository)], failed = COLLECTORS[tool](checkout, present)
        except (RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
            facts[(tool, repository)], failed = {}, set(present)
            print(f"{tool} failed on {repository}: {error}", file=sys.stderr)
        failures[(tool, repository)] = failed | (paths - set(present))
    return facts, failures


def judge_all(planned, facts, failures) -> tuple:
    """({source: [check]}, Counter of (detector, tool, outcome))."""
    tool_versions = versions()
    checks, outcomes = defaultdict(list), Counter()
    for rule, entry, finding in planned:
        key = (rule.tool, entry["repository"])
        if entry["path"] in failures.get(key, ()):
            outcomes[(rule.detector, rule.tool, "tool_failed")] += 1
            continue
        verdict = rule.judge(entry, finding, facts.get(key, {}).get(entry["path"], []))
        if verdict is None:
            outcomes[(rule.detector, rule.tool, "no_opinion")] += 1
            continue
        claim, detail = verdict
        outcomes[(rule.detector, rule.tool, claim)] += 1
        checks[f"oracle:{rule.tool}@{tool_versions[rule.tool]}"].append(
            {"id": entry["id"], "claim": claim, "detail": detail})
    return dict(checks), outcomes


def summary_markdown(outcomes, label_sets, checks) -> str:
    claims = {check["id"]: check["claim"] for source in checks.values() for check in source}
    verdicts = {entry["id"]: entry.get("verdict") for entries in label_sets.values() for entry in entries}
    rows = defaultdict(Counter)
    for (detector, tool, outcome), count in outcomes.items():
        rows[(detector, tool)][outcome] += count
    agreement = defaultdict(Counter)
    for source in checks.values():
        for check in source:
            verdict = verdicts.get(check["id"])
            agreement[check["claim"]][verdict] += 1
    lines = ["| Detector | Oracle | Confirmed | Refuted | No opinion | Tool failed |", "|---|---|---:|---:|---:|---:|"]
    for (detector, tool), counts in sorted(rows.items()):
        lines.append(f"| `{detector}` | {tool} | {counts['confirmed']} | {counts['refuted']} "
                     f"| {counts['no_opinion']} | {counts['tool_failed']} |")
    lines += ["", (f"Checks: {len(claims)}. Confirmed vs pre-label: {dict(agreement['confirmed'])}. "
                   f"Refuted vs pre-label: {dict(agreement['refuted'])}.")]
    return "\n".join(lines) + "\n"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("provision")
    running = commands.add_parser("run")
    running.add_argument("--scan", required=True, help="scan label whose findings are checked")
    running.add_argument("--tool", nargs="+", choices=sorted(COLLECTORS), help="oracles to run (default: all)")
    running.add_argument("--write", action="store_true", help="record the checks in the label files")
    running.add_argument("--json", type=Path, help="also write the checks here")
    overlapping = commands.add_parser("overlap")
    overlapping.add_argument("--scan", required=True, help="scan label whose findings are checked")
    overlapping.add_argument("--tool", nargs="+", choices=sorted(COLLECTORS), help="oracles to run (default: all)")
    overlapping.add_argument("--json", type=Path, help="write the per-finding claims here")
    args = parser.parse_args(argv)

    if args.command == "provision":
        provision()
        return 0
    if args.command == "overlap":
        entries = live_entries(run.load_results(args.scan))
        planned = plan(entries, run.load_results(args.scan), set(args.tool or ()))
        checks, _ = judge_all(planned, *collect(planned))
        rows, claims = overlap(entries, planned, checks, label_store.by_id(label_store.load_labels()))
        print(overlap_markdown(rows), end="")
        if args.json:
            args.json.write_text(json.dumps(claims, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        return 0
    label_sets = label_store.load_labels()
    planned = plan(label_sets, run.load_results(args.scan), set(args.tool or ()))
    facts, failures = collect(planned)
    checks, outcomes = judge_all(planned, facts, failures)
    print(summary_markdown(outcomes, label_sets, checks), end="")
    if args.json:
        args.json.write_text(json.dumps(checks, indent=2) + "\n", encoding="utf-8")
    if args.write:
        ran = {rule.tool for rule, _, _ in planned}
        for tool in sorted(ran):
            source = f"oracle:{tool}@{versions()[tool]}"
            _, skipped = label_workflow.apply_checks(label_sets, checks.get(source, []), source)
            for message in skipped:
                print(f"skipped {message}", file=sys.stderr)
        label_store.write_labels(label_sets)
        corroborated = sum(entry["status"] == "corroborated" for entries in label_sets.values() for entry in entries)
        print(f"recorded checks from {len(ran)} oracles; {corroborated} labels corroborated")
    return 0


if __name__ == "__main__":
    sys.exit(main())

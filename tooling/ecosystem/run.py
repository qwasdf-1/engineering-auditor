#!/usr/bin/env python3
"""Scan the pinned ecosystem corpus and diff Engineering Auditor revisions.

Commands:
  fetch              fetch every manifest repository at its pinned commit
  scan --label L     scan the corpus with one auditor binary into results/L

`--slice applications` selects the application repositories instead of the
library corpus. Each names the detectors it measures (ones the libraries
cannot, such as outbound network calls); its report keeps only their
findings, so the slice never moves another detector's standing.
  diff A B           compare two scan labels per detector and repository
  compare --rev-a X --rev-b Y
                     build both revisions, scan the corpus with each, and diff
  syntax-check L     fail if scan L leaves more files of any language unparsed
                     than eval/ecosystem/syntax.json allows (`--write` records)

Checkouts, builds, and results live under $EA_ECOSYSTEM_CACHE (default
target/ecosystem). Checkouts are depth-1, so history-dependent detectors such
as churn_complexity_hotspot stay silent and every scan is reproducible.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tooling.ecosystem import syntax_coverage  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "eval" / "ecosystem" / "manifest.json"
DETAIL_LIMIT = 20


def cache_root() -> Path:
    return Path(os.environ.get("EA_ECOSYSTEM_CACHE", REPO_ROOT / "target" / "ecosystem"))


SLICES = {"library": "repositories", "applications": "applications"}


# The held-out repositories: one per library language and HOLDOUT_APPLICATIONS
# applications, chosen by seeded hash so no one picks them. Detector fixes
# never read their findings or labels; their standing measures whether a fix
# generalizes beyond the repositories that motivated it.
HOLDOUT_SEED = "ea-holdout-v1"
HOLDOUT_APPLICATIONS = 6


def holdout_rank(repository_id) -> str:
    return hashlib.sha256(f"{HOLDOUT_SEED}:{repository_id}".encode()).hexdigest()


def holdout_choice(manifest) -> set:
    """The ids the seeded rule holds out of `manifest`."""
    by_language = defaultdict(list)
    for repository in manifest["repositories"]:
        by_language[repository["language"]].append(repository["id"])
    chosen = {min(ids, key=holdout_rank) for ids in by_language.values()}
    applications = sorted((repository["id"] for repository in manifest.get("applications", [])), key=holdout_rank)
    return chosen | set(applications[:HOLDOUT_APPLICATIONS])


def holdout_ids() -> set:
    """The ids the manifest marks `holdout`."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    return {repository["id"] for slice_name in SLICES.values() for repository in manifest.get(slice_name, [])
            if repository.get("holdout")}


def load_manifest(only=None, slice_name="library") -> list:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    repositories = manifest.get(SLICES[slice_name], [])
    if only:
        unknown = set(only) - {repository["id"] for repository in repositories}
        if unknown:
            raise SystemExit(f"unknown corpus ids: {sorted(unknown)}")
        repositories = [repository for repository in repositories if repository["id"] in only]
    return repositories


def git(args, cwd) -> str:
    result = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed in {cwd}: {result.stderr.strip()}")
    return result.stdout.strip()


def checkout_is_pinned(checkout: Path, commit: str) -> bool:
    if not (checkout / ".git").exists():
        return False
    try:
        head = git(["rev-parse", "HEAD"], checkout)
        dirty = git(["status", "--porcelain"], checkout)
    except RuntimeError:
        return False
    return head == commit and not dirty


def fetch_repository(url: str, commit: str, checkout: Path) -> None:
    """Fetch exactly `commit` (tags can move) and verify the checkout."""
    if checkout_is_pinned(checkout, commit):
        return
    shutil.rmtree(checkout, ignore_errors=True)
    checkout.mkdir(parents=True)
    git(["init", "-q"], checkout)
    git(["remote", "add", "origin", url], checkout)
    git(["fetch", "-q", "--depth", "1", "origin", commit], checkout)
    git(["checkout", "-q", "--detach", "FETCH_HEAD"], checkout)
    if not checkout_is_pinned(checkout, commit):
        raise RuntimeError(f"{url} did not resolve to pinned commit {commit}")


def fetch(repositories) -> None:
    for repository in repositories:
        fetch_repository(repository["url"], repository["commit"], checkout_path(repository))
        print(f"fetched {repository['id']} @ {repository['commit'][:12]}")


def checkout_path(repository) -> Path:
    return cache_root() / "repos" / repository["id"]


def finding_language(finding, languages) -> str:
    locations = finding.get("locations") or [{}]
    return languages.get(locations[0].get("path"), "repository")


def summarize(reports) -> dict:
    """Aggregate per-detector counts from {repository: (manifest entry, report)}."""
    by_detector = defaultdict(lambda: {"total": 0, "by_language": Counter(), "by_repository": Counter()})
    repositories = []
    for repository_id, (repository, report) in sorted(reports.items()):
        languages = {file["path"]: file["catalog_language"] for file in report.get("files", [])}
        for finding in report["findings"]:
            entry = by_detector[finding["detector"]]
            entry["total"] += 1
            entry["by_language"][finding_language(finding, languages)] += 1
            entry["by_repository"][repository_id] += 1
        repositories.append({
            "id": repository_id,
            "language": repository["language"],
            "commit": repository["commit"],
            "findings": len(report["findings"]),
        })
    return {
        "repositories": repositories,
        "syntax": syntax_coverage.syntax_summary(report for _, report in reports.values()),
        "by_detector": {
            detector: {
                "total": entry["total"],
                "by_language": dict(sorted(entry["by_language"].items())),
                "by_repository": dict(sorted(entry["by_repository"].items())),
            }
            for detector, entry in sorted(by_detector.items())
        },
    }


def summary_markdown(summary, label) -> str:
    languages = sorted({language for entry in summary["by_detector"].values() for language in entry["by_language"]})
    lines = [f"# Ecosystem scan `{label}`", "", "| Repository | Language | Findings |", "|---|---|---:|"]
    lines += [f"| {r['id']} | {r['language']} | {r['findings']} |" for r in summary["repositories"]]
    lines += ["", "| Detector | Total | " + " | ".join(languages) + " |", "|---|---:|" + "---:|" * len(languages)]
    for detector, entry in summary["by_detector"].items():
        cells = " | ".join(str(entry["by_language"].get(language, "")) for language in languages)
        lines.append(f"| `{detector}` | {entry['total']} | {cells} |")
    return "\n".join(lines) + "\n"


def run_scan(auditor: Path, repositories, label: str) -> int:
    out = cache_root() / "results" / label
    out.mkdir(parents=True, exist_ok=True)
    reports, failures = {}, []
    for repository in repositories:
        fetch_repository(repository["url"], repository["commit"], checkout_path(repository))
        report_path = out / f"{repository['id']}.json"
        started = time.monotonic()
        result = subprocess.run(
            [str(auditor), "scan", "--repo-root", str(checkout_path(repository)), "--profile", "full",
             "--format", "json", "--output", str(report_path)],
            capture_output=True, text=True,
        )
        elapsed = time.monotonic() - started
        if result.returncode != 0:
            failures.append(f"{repository['id']}: {result.stderr.strip()[:300]}")
            continue
        report = targeted(json.loads(report_path.read_text(encoding="utf-8")), repository)
        report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        reports[repository["id"]] = (repository, report)
        print(f"scanned {repository['id']} in {elapsed:.1f}s")
    summary = summarize(reports)
    summary["label"] = label
    summary["auditor_version"] = subprocess.run(
        [str(auditor), "version"], capture_output=True, text=True).stdout.strip()
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    (out / "summary.md").write_text(summary_markdown(summary, label), encoding="utf-8")
    for failure in failures:
        print(f"scan failed: {failure}", file=sys.stderr)
    return 1 if failures else 0


def targeted(report, repository) -> dict:
    """An application repository's report with only the findings of the
    detectors it measures; a library repository's report unchanged."""
    detectors = repository.get("detectors")
    if detectors is None:
        return report
    return {**report, "findings": [finding for finding in report["findings"] if finding["detector"] in detectors]}


def finding_index(report) -> dict:
    return {finding["id"]: finding for finding in report["findings"]}


def diff_reports(before: dict, after: dict) -> dict:
    """Compare {repository: report} maps per detector.

    Both runs scan the same pinned corpus commits, so a resolved and a new
    finding at the same detector, path, and line are one finding whose ID
    changed: they are reported as `rekeyed`, not as new plus resolved.
    """
    detectors = defaultdict(lambda: {"new": [], "resolved": [], "rekeyed": [], "unchanged": 0})
    for repository_id in sorted(set(before) | set(after)):
        old = finding_index(before.get(repository_id, {"findings": []}))
        new = finding_index(after.get(repository_id, {"findings": []}))
        added = {finding_id: new[finding_id] for finding_id in set(new) - set(old)}
        removed = {finding_id: old[finding_id] for finding_id in set(old) - set(new)}
        for old_id, new_id in rekeyed_pairs(removed, added):
            detectors[added[new_id]["detector"]]["rekeyed"].append(
                {**brief(repository_id, added[new_id]), "previous_id": old_id})
            del added[new_id], removed[old_id]
        for finding_id in sorted(added):
            detectors[added[finding_id]["detector"]]["new"].append(brief(repository_id, added[finding_id]))
        for finding_id in sorted(removed):
            detectors[removed[finding_id]["detector"]]["resolved"].append(brief(repository_id, removed[finding_id]))
        for finding_id in set(old) & set(new):
            detectors[new[finding_id]["detector"]]["unchanged"] += 1
    return dict(sorted(detectors.items()))


def rekeyed_pairs(removed: dict, added: dict) -> list:
    """Pair findings that share one unique detector/path/line site."""
    def sites(findings):
        grouped = defaultdict(list)
        for finding_id, finding in findings.items():
            location = (finding.get("locations") or [{}])[0]
            grouped[(finding["detector"], location.get("path"), location.get("line"))].append(finding_id)
        return grouped
    old_sites, new_sites = sites(removed), sites(added)
    return sorted((old_sites[site][0], new_sites[site][0]) for site in old_sites
                  if len(old_sites[site]) == 1 and len(new_sites.get(site, [])) == 1)


def brief(repository_id, finding) -> dict:
    location = (finding.get("locations") or [{}])[0]
    return {
        "repository": repository_id,
        "id": finding["id"],
        "path": location.get("path"),
        "line": location.get("line"),
        "message": finding["message"],
    }


def diff_markdown(diff, label_a, label_b) -> str:
    totals = {kind: sum(len(entry[kind]) for entry in diff.values()) for kind in ("new", "resolved", "rekeyed")}
    lines = [f"# Ecosystem diff `{label_a}` → `{label_b}`", "",
             f"New: {totals['new']}. Resolved: {totals['resolved']}. Re-keyed: {totals['rekeyed']}.", "",
             "| Detector | New | Resolved | Re-keyed | Unchanged |", "|---|---:|---:|---:|---:|"]
    lines += [f"| `{detector}` | {len(e['new'])} | {len(e['resolved'])} | {len(e['rekeyed'])} | {e['unchanged']} |"
              for detector, e in diff.items() if e["new"] or e["resolved"] or e["rekeyed"]]
    for detector, entry in diff.items():
        for kind in ("new", "resolved", "rekeyed"):
            if entry[kind]:
                lines += ["", f"## `{detector}` {kind}", ""]
                lines += [f"- {f['repository']}:{f['path']}:{f['line']}: {f['message']}"
                          for f in entry[kind][:DETAIL_LIMIT]]
                if len(entry[kind]) > DETAIL_LIMIT:
                    lines.append(f"- … {len(entry[kind]) - DETAIL_LIMIT} more")
    return "\n".join(lines) + "\n"


def load_results(label) -> dict:
    out = cache_root() / "results" / label
    if not (out / "summary.json").exists():
        raise SystemExit(f"no scan results for label {label!r} under {out}")
    summary = json.loads((out / "summary.json").read_text(encoding="utf-8"))
    return {
        repository["id"]: json.loads((out / f"{repository['id']}.json").read_text(encoding="utf-8"))
        for repository in summary["repositories"]
    }


def run_diff(label_a, label_b) -> int:
    diff = diff_reports(load_results(label_a), load_results(label_b))
    out = cache_root() / "results" / f"diff-{label_a}-{label_b}"
    out.mkdir(parents=True, exist_ok=True)
    (out / "diff.json").write_text(json.dumps(diff, indent=2) + "\n", encoding="utf-8")
    markdown = diff_markdown(diff, label_a, label_b)
    (out / "diff.md").write_text(markdown, encoding="utf-8")
    print(markdown)
    return 0


def build_revision(revision: str) -> Path:
    """Build auditor-cli at `revision` once and cache the binary by commit."""
    commit = git(["rev-parse", f"{revision}^{{commit}}"], REPO_ROOT)
    binary = cache_root() / "builds" / commit / "auditor"
    if binary.exists():
        return binary
    worktree = cache_root() / "worktrees" / commit
    shutil.rmtree(worktree, ignore_errors=True)
    git(["worktree", "add", "-q", "--detach", str(worktree), commit], REPO_ROOT)
    try:
        environment = {**os.environ, "CARGO_TARGET_DIR": str(cache_root() / "builds" / "target")}
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "auditor-cli"],
                       cwd=worktree, env=environment, check=True)
        binary.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(cache_root() / "builds" / "target" / "release" / "auditor", binary)
    finally:
        git(["worktree", "remove", "--force", str(worktree)], REPO_ROOT)
    return binary


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("fetch", "scan", "compare"):
        command = commands.add_parser(name)
        command.add_argument("--only", nargs="+", help="corpus ids to include")
        command.add_argument("--slice", choices=sorted(SLICES), default="library",
                             help="library corpus (default) or application repositories")
    commands.choices["scan"].add_argument("--label", required=True)
    commands.choices["scan"].add_argument("--auditor", type=Path,
                                          default=REPO_ROOT / "target" / "release" / "auditor")
    commands.choices["compare"].add_argument("--rev-a", required=True)
    commands.choices["compare"].add_argument("--rev-b", required=True)
    syntax = commands.add_parser("syntax-check")
    syntax.add_argument("label")
    syntax.add_argument("--slice", choices=sorted(SLICES), default="library")
    syntax.add_argument("--write", action="store_true", help="record the scan as the baseline")
    diff = commands.add_parser("diff")
    diff.add_argument("label_a")
    diff.add_argument("label_b")
    args = parser.parse_args(argv)

    if args.command == "diff":
        return run_diff(args.label_a, args.label_b)
    if args.command == "syntax-check":
        summary = json.loads((cache_root() / "results" / args.label / "summary.json").read_text(encoding="utf-8"))
        return syntax_coverage.check(summary, args.slice, args.write)
    repositories = load_manifest(args.only, args.slice)
    if args.command == "fetch":
        fetch(repositories)
        return 0
    if args.command == "scan":
        return run_scan(args.auditor, repositories, args.label)
    labels = []
    for revision in (args.rev_a, args.rev_b):
        label = git(["rev-parse", "--short=12", f"{revision}^{{commit}}"], REPO_ROOT)
        if run_scan(build_revision(revision), repositories, label) != 0:
            return 1
        labels.append(label)
    return run_diff(*labels)


if __name__ == "__main__":
    sys.exit(main())

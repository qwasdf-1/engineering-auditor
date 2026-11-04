#!/usr/bin/env python3
"""Prepare engineering-review inputs for the ecosystem corpus without
keeping clones.

Each repository is fetched at its pinned commit with only a window of
history (a shallow fetch deepened to the window plus a margin, with blobs:
a blobless clone makes `--numstat` fetch every blob separately), scanned
with a repository map, summarized into a history digest, optionally handed
to a review command while its checkout exists, and deleted. What stays,
under $EA_ECOSYSTEM_CACHE/review-inputs/<label>/<id>/, is the report, its
repository map, the digest, and inputs.json with timings and sizes.

  prepare --label L [--slice applications] [--only ID ...]
          [--window-days 365] [--jobs 2] [--auditor PATH]
          [--review-command TEMPLATE]

Jobs run in parallel (two by default, for four cores), so one repository
fetches while another scans. `--review-command` runs once per repository
before its checkout is deleted; in each argument `{repo}`, `{report}`,
`{history}`, and `{out}` become paths.
"""

import argparse
import json
import shlex
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tooling.ecosystem.run import REPO_ROOT, SLICES, cache_root, git, load_manifest  # noqa: E402

DEFAULT_WINDOW_DAYS = 365
# The shallow boundary commit's diff is the whole tree, so the digest drops
# it; fetching past the window keeps the boundary outside it.
MARGIN_DAYS = 31


def fetch_window(url: str, commit: str, checkout: Path, window_days: int) -> None:
    """Fetch `commit` with the history of `window_days` (plus the margin)
    before it, and check it out."""
    shutil.rmtree(checkout, ignore_errors=True)
    checkout.mkdir(parents=True)
    git(["init", "-q"], checkout)
    git(["remote", "add", "origin", url], checkout)
    git(["fetch", "-q", "--depth", "1", "origin", commit], checkout)
    head_time = int(git(["log", "-1", "--format=%ct", "FETCH_HEAD"], checkout))
    since = head_time - (window_days + MARGIN_DAYS) * 86_400
    git(["fetch", "-q", f"--shallow-since=@{since}", "origin", commit], checkout)
    git(["checkout", "-q", "--detach", "FETCH_HEAD"], checkout)
    head = git(["rev-parse", "HEAD"], checkout)
    if head != commit:
        raise RuntimeError(f"{url} resolved to {head}, not pinned commit {commit}")


def directory_bytes(path: Path) -> int:
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def auditor_run(arguments) -> None:
    result = subprocess.run([str(argument) for argument in arguments], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(map(str, arguments[:3]))} failed: {result.stderr.strip()[:300]}")


@dataclass(frozen=True)
class Settings:
    auditor: Path
    window_days: int = DEFAULT_WINDOW_DAYS
    # Run per repository before its checkout is deleted.
    review_command: str | None = None


def prepare_repository(repository, settings: Settings, out_root: Path) -> dict:
    """Fetch, scan, digest, optionally review, and delete one repository."""
    auditor, window_days = settings.auditor, settings.window_days
    out = out_root / repository["id"]
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    checkout = cache_root() / "review-checkouts" / repository["id"]
    report, history = out / "audit.json", out / "history.json"
    inputs = {"id": repository["id"], "url": repository["url"], "commit": repository["commit"],
              "window_days": window_days}
    try:
        started = time.monotonic()
        fetch_window(repository["url"], repository["commit"], checkout, window_days)
        inputs["fetch_seconds"] = round(time.monotonic() - started, 1)
        inputs["checkout_bytes"] = directory_bytes(checkout)
        started = time.monotonic()
        auditor_run([auditor, "scan", "--repo-root", checkout, "--profile", "full",
                     "--format", "json", "--output", report])
        inputs["scan_seconds"] = round(time.monotonic() - started, 1)
        started = time.monotonic()
        auditor_run([auditor, "review", "history", "--repo-root", checkout,
                     "--window-days", window_days, "--output", history])
        inputs["history_seconds"] = round(time.monotonic() - started, 1)
        if settings.review_command:
            places = {"repo": checkout, "report": report, "history": history, "out": out}
            command = [argument.format(**places)
                       for argument in shlex.split(settings.review_command)]
            started = time.monotonic()
            subprocess.run(command, check=True)
            inputs["review_seconds"] = round(time.monotonic() - started, 1)
    finally:
        shutil.rmtree(checkout, ignore_errors=True)
    inputs["kept_bytes"] = directory_bytes(out)
    (out / "inputs.json").write_text(json.dumps(inputs, indent=2) + "\n", encoding="utf-8")
    return inputs


def prepare(repositories, settings: Settings, label: str, jobs: int) -> int:
    out_root = cache_root() / "review-inputs" / label
    out_root.mkdir(parents=True, exist_ok=True)
    prepared, failures = [], []

    def job(repository):
        try:
            inputs = prepare_repository(repository, settings, out_root)
            print(f"prepared {repository['id']}: fetch {inputs['fetch_seconds']}s, "
                  f"scan {inputs['scan_seconds']}s, kept {inputs['kept_bytes']} bytes")
            prepared.append(inputs)
        except (RuntimeError, OSError, ValueError, subprocess.CalledProcessError) as error:
            failures.append({"id": repository["id"], "error": str(error)[:500]})
            print(f"failed {repository['id']}: {error}", file=sys.stderr)

    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        list(pool.map(job, repositories))
    checkouts = cache_root() / "review-checkouts"
    if checkouts.is_dir() and not any(checkouts.iterdir()):
        checkouts.rmdir()
    summary = {"label": label, "window_days": settings.window_days,
               "repositories": sorted(prepared, key=lambda inputs: inputs["id"]),
               "failures": sorted(failures, key=lambda failure: failure["id"])}
    (out_root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return 1 if failures else 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("prepare")
    command.add_argument("--label", required=True)
    command.add_argument("--slice", choices=sorted(SLICES), default="applications")
    command.add_argument("--only", nargs="+", help="corpus ids to include")
    command.add_argument("--window-days", type=int, default=DEFAULT_WINDOW_DAYS)
    command.add_argument("--jobs", type=int, default=2)
    command.add_argument("--auditor", type=Path, default=REPO_ROOT / "target" / "release" / "auditor")
    command.add_argument("--review-command", help="run per repository before its checkout is deleted")
    args = parser.parse_args(argv)
    if args.window_days < 1:
        parser.error("--window-days must be at least 1")
    repositories = load_manifest(args.only, args.slice)
    settings = Settings(args.auditor, args.window_days, args.review_command)
    return prepare(repositories, settings, args.label, args.jobs)


if __name__ == "__main__":
    sys.exit(main())

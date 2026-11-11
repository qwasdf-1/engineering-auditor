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
          [--review-command TEMPLATE] [--review-runs N]
  qualify --label L --route-set NAME [--corpus PATH] [--auditor PATH]

Jobs run in parallel (two by default, for four cores), so one repository
fetches while another scans. `--review-command` runs `--review-runs` times
per repository before its checkout is deleted; in each argument `{repo}`,
`{report}`, `{history}`, and `{out}` become paths and `{run}` the run's
number, so runs write `{out}/review-{run}.json`.

Preparing a label again continues it: a repository whose inputs and runs
are recorded is skipped, a prepared one keeps its report and digest, a
finished run is kept, and a run whose record says it stopped (a provider
outage outlasted the retries, or the run was interrupted after a
checkpoint) is resumed with `--resume` on that record, so no answered
question is paid for twice.

`qualify` checks the recorded runs offline against the qualification
corpus (config/auditor/review-qualification-corpus-v1.json): for each of
its repositories, `auditor review qualify` over the kept report, history,
`review-*.json` runs, and `usefulness-*.json` sheets, with the corpus's
route set, brief, and stability policy. The summary says whether the gates
passed on every repository and how the counted usefulness compares with
the corpus threshold, which stays unset until the first measured runs.
"""

import argparse
import hashlib
import json
import operator
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
CORPUS = REPO_ROOT / "config" / "auditor" / "review-qualification-corpus-v1.json"
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
    review_runs: int = 1


def finished(record: Path) -> bool:
    """A recorded run that did not stop; a stopped one resumes."""
    return record.exists() and not json.loads(record.read_text(encoding="utf-8")).get("stopped")


def write_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def timed(action) -> float:
    started = time.monotonic()
    action()
    return round(time.monotonic() - started, 1)


def scan_inputs(repository, settings: Settings, checkout: Path, out: Path, fetch_seconds) -> dict:
    """Record the scan report and history digest of a fresh checkout."""
    auditor = settings.auditor
    inputs = {"id": repository["id"], "url": repository["url"], "commit": repository["commit"],
              "window_days": settings.window_days, "fetch_seconds": fetch_seconds,
              "checkout_bytes": directory_bytes(checkout), "reviews": []}
    inputs["scan_seconds"] = timed(lambda: auditor_run(
        [auditor, "scan", "--repo-root", checkout, "--profile", "full", "--format", "json",
         "--output", out / "audit.json"]))
    inputs["history_seconds"] = timed(lambda: auditor_run(
        [auditor, "review", "history", "--repo-root", checkout, "--window-days",
         settings.window_days, "--output", out / "history.json"]))
    return inputs


def prepare_repository(repository, settings: Settings, out_root: Path) -> dict:
    """Fetch, scan, digest, optionally review, and delete one repository;
    continue what an earlier preparation of the label left."""
    out = out_root / repository["id"]
    inputs_path = out / "inputs.json"
    inputs = json.loads(inputs_path.read_text(encoding="utf-8")) if inputs_path.exists() else None
    runs = range(1, settings.review_runs + 1) if settings.review_command else range(0)
    # Without inputs the directory is wiped, so every run is pending.
    pending = [run for run in runs if inputs is None or not finished(out / f"review-{run}.json")]
    if inputs is not None and not pending:
        return {**inputs, "stopped_runs": []}
    checkout = cache_root() / "review-checkouts" / repository["id"]
    try:
        fetch_seconds = timed(lambda: fetch_window(
            repository["url"], repository["commit"], checkout, settings.window_days))
        if inputs is None:
            shutil.rmtree(out, ignore_errors=True)
            out.mkdir(parents=True)
            inputs = scan_inputs(repository, settings, checkout, out, fetch_seconds)
            write_json(inputs_path, inputs)
        places = {"repo": checkout, "report": out / "audit.json",
                  "history": out / "history.json", "out": out}
        for run in pending:
            record = out / f"review-{run}.json"
            command = [argument.format(**places, run=run)
                       for argument in shlex.split(settings.review_command)]
            resumed = record.exists()
            started = time.monotonic()
            result = subprocess.run(command + ["--resume", str(record)] * resumed)
            inputs["reviews"].append({"run": run, "resumed": resumed, "exit": result.returncode,
                                      "seconds": round(time.monotonic() - started, 1)})
            write_json(inputs_path, inputs)
            result.check_returncode()
    finally:
        shutil.rmtree(checkout, ignore_errors=True)
    inputs["kept_bytes"] = directory_bytes(out)
    write_json(inputs_path, inputs)
    records = [out / f"review-{run}.json" for run in pending]
    stopped = [run for run, record in zip(pending, records) if record.exists() and not finished(record)]
    return {**inputs, "stopped_runs": stopped}


def prepare(repositories, settings: Settings, label: str, jobs: int) -> int:
    out_root = cache_root() / "review-inputs" / label
    out_root.mkdir(parents=True, exist_ok=True)
    prepared, failures = [], []

    def job(repository):
        try:
            inputs = prepare_repository(repository, settings, out_root)
            print(f"prepared {repository['id']}: fetch {inputs['fetch_seconds']}s, "
                  f"scan {inputs['scan_seconds']}s, kept {inputs['kept_bytes']} bytes")
            if inputs["stopped_runs"]:
                failures.append({"id": repository["id"],
                                 "error": f"runs {inputs['stopped_runs']} stopped; prepare again to resume"})
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
    write_json(out_root / "summary.json", summary)
    return 1 if failures else 0


def file_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def load_corpus(path: Path = CORPUS) -> dict:
    """The qualification corpus, with every bound file at its digest and
    every repository in the ecosystem manifest's slice."""
    corpus = json.loads(path.read_text(encoding="utf-8"))
    bound = [*corpus["bindings"].values(), *corpus["route_sets"].values()]
    for binding in bound:
        if file_digest(REPO_ROOT / binding["path"]) != binding["digest"]:
            raise SystemExit(f"{binding['path']} changed since the corpus bound it")
    repositories = corpus["repositories"]
    known = {repository["id"] for repository in load_manifest(None, repositories["slice"])}
    unknown = sorted(set(repositories["ids"]) - known)
    if unknown:
        raise SystemExit(f"the corpus names repositories outside the {repositories['slice']} slice: {unknown}")
    return corpus


def qualify_repository(corpus: dict, route_set: str, auditor: Path, out: Path) -> dict:
    """`auditor review qualify` over one repository's recorded runs."""
    runs = sorted(out.glob("review-*.json"))
    entry = {"id": out.name, "runs": len(runs)}
    wanted = corpus["run_plan"]["runs_per_repository"]
    if len(runs) < wanted:
        return {**entry, "passed": False, "error": f"{len(runs)} recorded runs, {wanted} wanted"}
    command = [auditor, "review", "qualify", "--report", out / "audit.json",
               "--routes", REPO_ROOT / corpus["route_sets"][route_set]["path"],
               "--brief", REPO_ROOT / corpus["bindings"]["brief"]["path"],
               "--top", corpus["stability"]["top"],
               "--min-overlap", corpus["stability"]["min_overlap"],
               "--output", out / f"qualification-{route_set}.json"]
    if (out / "history.json").exists():
        command += ["--history", out / "history.json"]
    for run in runs:
        command += ["--review", run]
    for sheet in sorted(out.glob("usefulness-*.json")):
        command += ["--scores", sheet]
    result = subprocess.run([str(argument) for argument in command], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        return {**entry, "passed": False, "error": result.stderr.strip()[:500]}
    qualification = json.loads(result.stdout)
    usefulness = qualification["usefulness"] or {}
    return {
        **entry,
        "passed": qualification["passed"],
        "failed_gates": sorted({f"{run['label']}: {gate['gate']}" for run in qualification["runs"]
                                for gate in run["gates"] if not gate["passed"]}),
        "stability_passed": qualification["stability"]["passed"],
        "useful": usefulness.get("useful", 0),
        "useful_beyond_report": usefulness.get("useful_beyond_report", 0),
        "counted": usefulness.get("counted", 0),
        "pending": usefulness.get("pending", 0),
        "disputed": usefulness.get("disputed", 0),
    }


def qualify(corpus: dict, route_set: str, auditor: Path, label: str) -> int:
    """Qualify one route set on every corpus repository; 0 only when the
    gates pass everywhere and usefulness meets a set threshold."""
    if route_set not in corpus["route_sets"]:
        raise SystemExit(f"the corpus has no route set {route_set}; it has {sorted(corpus['route_sets'])}")
    out_root = cache_root() / "review-inputs" / label
    repositories = [qualify_repository(corpus, route_set, auditor, out_root / repository)
                    for repository in corpus["repositories"]["ids"]]
    gates_passed = all(repository["passed"] for repository in repositories)
    useful, beyond, counted = (sum(repository.get(key, 0) for repository in repositories)
                               for key in ("useful", "useful_beyond_report", "counted"))
    bar = corpus["usefulness"]
    thresholds = [bar["threshold"], bar["beyond_report_threshold"]]
    shares = [count / counted if counted else None for count in (useful, beyond)]
    if not gates_passed:
        decision = "fails the automatic gates"
    elif None in thresholds:
        decision = "gates pass; the usefulness bar is not set yet"
    elif counted and all(map(operator.ge, shares, thresholds)):
        decision = "qualifies"
    else:
        decision = "below the usefulness bar"
    summary = {"corpus_id": corpus["corpus_id"], "label": label, "route_set": route_set,
               "repositories": repositories, "gates_passed_everywhere": gates_passed,
               "usefulness": {"useful": useful, "useful_beyond_report": beyond, "counted": counted,
                              "share": shares[0], "beyond_report_share": shares[1],
                              "threshold": thresholds[0],
                              "beyond_report_threshold": thresholds[1]},
               "decision": decision}
    out_root.mkdir(parents=True, exist_ok=True)
    write_json(out_root / f"qualification-{route_set}.json", summary)
    print(f"{route_set}: {decision} ({useful}/{counted} counted items useful, "
          f"{beyond} beyond the report)")
    return 0 if decision == "qualifies" else 1


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
    command.add_argument("--review-runs", type=int, default=1, help="times to run the review command")
    command = commands.add_parser("qualify")
    command.add_argument("--label", required=True)
    command.add_argument("--route-set", required=True)
    command.add_argument("--corpus", type=Path, default=CORPUS)
    command.add_argument("--auditor", type=Path, default=REPO_ROOT / "target" / "release" / "auditor")
    args = parser.parse_args(argv)
    if args.command == "qualify":
        return qualify(load_corpus(args.corpus), args.route_set, args.auditor, args.label)
    if args.window_days < 1:
        parser.error("--window-days must be at least 1")
    if args.review_runs < 1:
        parser.error("--review-runs must be at least 1")
    if args.review_runs > 1 and "{run}" not in (args.review_command or ""):
        parser.error("--review-runs above 1 needs {run} in --review-command, or the runs overwrite each other")
    repositories = load_manifest(args.only, args.slice)
    settings = Settings(args.auditor, args.window_days, args.review_command, args.review_runs)
    return prepare(repositories, settings, args.label, args.jobs)


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Have a second, independent model label sampled findings blind.

Commands:
  run --scan L --model PROVIDER/MODEL [--detector D..] [--batch N] [--limit N] [--workers N] [--all]
                     send blind labelling bundles to the model through
                     `opencode run` and write {id, verdict, reason} JSON Lines
                     to --out; responses are cached per batch, so an
                     interrupted run resumes; findings this model already
                     checked are not re-sent (their verdicts are copied to
                     --out) unless --all

The model sees the finding, the detector's rationale, and the source excerpt,
never the first labeller's verdict or the oracle checks. Record its verdicts
as checks with `labels.py checks OUT --source model:<id>`; a check from the
same model that made the pre-label is rejected there.
"""

import argparse
import hashlib
import functools
import json
import os
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tooling.ecosystem import label_store, label_workflow, run  # noqa: E402

BATCH = 8
TIMEOUT = 600
ATTEMPTS = 3
# opencode's free tier only serves its own agent with its tools defined, so the
# tools stay and every permission is "ask", which `opencode run` auto-rejects.
SANDBOX_CONFIG = '{"$schema": "https://opencode.ai/config.json", "permission": {"*": "ask"}}\n'

INSTRUCTIONS = """\
Do not use any tools; everything you need is below. Answer directly.

You are reviewing findings from a static code-quality analyzer. For each
finding, decide whether it is a true positive or a false positive, using only
the finding, the detector's stated rationale, and the source excerpt (the line
marked `>` is the reported line).

Verdict standard:
- true_positive: the finding's claim is factually correct, and the detector's
  rationale applies at that site within its stated scope. Intentional code
  still counts when the claim holds.
- false_positive: the claim is wrong (miscounted, wrong symbol, misparsed
  syntax); or the site is outside the detector's scope (test, fixture,
  generated, example, benchmark, or build/tooling code, except that test code
  is the scope of the test-quality detectors `test_without_assertion` and
  `tautological_assertion`); or the rationale does not apply because the code
  is an idiom the rule should recognise.
- unclear: you would need context the excerpt does not show (for example a
  count over a function longer than the excerpt). Prefer unclear to guessing.

Answer with only a JSON array, one object per finding, in the input order:
[{"id": "EA-...", "verdict": "true_positive|false_positive|unclear",
  "reason": "one sentence citing the code"}]

Findings:
"""


def bundle_text(item) -> str:
    fields = {key: item.get(key) for key in ("id", "detector", "language", "path", "line", "title", "message",
                                              "recommendation", "evidence")}
    return json.dumps(fields, ensure_ascii=False) + "\nExcerpt:\n" + (item.get("excerpt") or "(none)") + "\n"


def prompt(items) -> str:
    return INSTRUCTIONS + "\n---\n".join(bundle_text(item) for item in items)


def json_value(text, opening, closing):
    start, end = text.find(opening), text.rfind(closing)
    if start < 0 or end <= start:
        return None
    try:
        return json.loads(text[start:end + 1])
    except json.JSONDecodeError:
        return None


def parse_answer(text, ids) -> list:
    """The verdict objects for `ids` from a model answer, or [] if unusable.

    Accepts the requested array, an object keyed by finding ID, or an object
    wrapping the array, since free models drift between them.
    """
    answer = json_value(text, "[", "]")
    if not isinstance(answer, list):
        wrapped = json_value(text, "{", "}")
        if isinstance(wrapped, dict):
            lists = [value for value in wrapped.values() if isinstance(value, list)]
            answer = lists[0] if lists else [{"id": key, **value} for key, value in wrapped.items()
                                             if isinstance(value, dict)]
    verdicts = []
    for item in answer if isinstance(answer, list) else []:
        if (isinstance(item, dict) and item.get("id") in ids and item.get("verdict") in label_store.VERDICTS
                and str(item.get("reason", "")).strip()):
            verdicts.append({"id": item["id"], "verdict": item["verdict"], "reason": str(item["reason"]).strip()})
    return verdicts


@functools.lru_cache(maxsize=None)
def plugin_free(binary) -> tuple:
    """`--pure` (no external plugins) where the CLI has it; OpenCode 2 dropped the flag."""
    help_text = subprocess.run([binary, "run", "--help"], capture_output=True, text=True, timeout=60)
    return ("--pure",) if "--pure" in help_text.stdout + help_text.stderr else ()


def opencode_text(model, text) -> str:
    """One sandboxed `opencode run` turn in an empty directory; returns the reply text.

    Tool calls are auto-rejected and Claude Code instruction files are not
    loaded, so the model sees only the prompt.
    """
    binary = os.environ.get("EA_OPENCODE_BIN", str(Path.home() / ".opencode" / "bin" / "opencode"))
    with tempfile.TemporaryDirectory() as scratch:
        (Path(scratch) / "opencode.json").write_text(SANDBOX_CONFIG, encoding="utf-8")
        result = subprocess.run([binary, "run", *plugin_free(binary), "-m", model, "--format", "json", text],
                                cwd=scratch,
                                capture_output=True, text=True, timeout=TIMEOUT,
                                env={**os.environ, "PWD": scratch, "OPENCODE_DISABLE_CLAUDE_CODE": "1"})
    parts = []
    for line in result.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "text":
            parts.append(event.get("part", {}).get("text", ""))
    return "".join(parts)


@dataclass(frozen=True)
class Route:
    """The reviewing model and the call that sends it one prompt."""
    model: str
    ask: object = opencode_text  # (model, prompt) -> reply text


def earlier_verdicts(labels, source) -> dict:
    """{id: {id, verdict, reason}} for every label already checked by `source`.

    `labels.py checks` replaces all of a source's checks, so a run that skips
    these findings writes them back out unchanged.
    """
    return {entry["id"]: {"id": entry["id"], "verdict": check["verdict"], "reason": check["reason"]}
            for entries in labels.values() for entry in entries
            for check in entry.get("checks", []) if check.get("source") == source}


def batch_key(model, items) -> str:
    digest = hashlib.sha256(prompt(items).encode() + model.encode()).hexdigest()
    return digest[:20]


def review_batch(chunk, route: Route, cache: Path) -> list:
    ids = {item["id"] for item in chunk}
    cached = cache / f"{batch_key(route.model, chunk)}.json"
    if cached.exists():
        return json.loads(cached.read_text(encoding="utf-8"))
    for _ in range(ATTEMPTS):
        try:
            answer = parse_answer(route.ask(route.model, prompt(chunk)), ids)
        except subprocess.TimeoutExpired:
            answer = []
        if answer:
            cached.write_text(json.dumps(answer, indent=1) + "\n", encoding="utf-8")
            return answer
    return []


def review(items, route: Route, cache: Path, batch=BATCH, workers=1) -> list:
    """Label `items` in batches, reusing cached batch answers; verdicts keep input order."""
    cache.mkdir(parents=True, exist_ok=True)
    chunks = [items[start:start + batch] for start in range(0, len(items), batch)]
    answers = [None] * len(chunks)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = {pool.submit(review_batch, chunk, route, cache): index for index, chunk in enumerate(chunks)}
        for done, future in enumerate(as_completed(futures), 1):
            index = futures[future]
            answers[index] = future.result()
            print(f"batch {done}/{len(chunks)}: {len(answers[index])}/{len(chunks[index])} verdicts",
                  file=sys.stderr, flush=True)
    return [verdict for answer in answers for verdict in answer]


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    running = commands.add_parser("run")
    running.add_argument("--scan", required=True)
    running.add_argument("--model", required=True, help="opencode provider/model, e.g. opencode/step-5-preview-free")
    running.add_argument("--detector", nargs="+")
    running.add_argument("--batch", type=int, default=BATCH)
    running.add_argument("--limit", type=int, help="review at most this many findings")
    running.add_argument("--workers", type=int, default=1, help="concurrent opencode runs")
    running.add_argument("--out", type=Path, required=True, help="JSON Lines of {id, verdict, reason}")
    running.add_argument("--all", action="store_true", help="also re-send findings this model already checked")
    running.add_argument("--skip-checked-by", nargs="+", default=[], metavar="MODEL",
                         help="also skip findings these models checked (the same model through another provider)")
    args = parser.parse_args(argv)

    labels, reports = label_store.load_labels(), run.load_results(args.scan)
    live = {finding["id"] for report in reports.values() for finding in report["findings"]}
    items = [item for item in label_workflow.context(labels, reports, label_store.REVIEWED, args.detector, blind=True)
             if item["id"] in live]
    earlier = {} if args.all else earlier_verdicts(labels, f"model:{args.model}")
    checked = {finding_id for model in args.skip_checked_by for finding_id in earlier_verdicts(labels, f"model:{model}")}
    items = [item for item in items if item["id"] not in earlier and item["id"] not in checked]
    items = items[:args.limit] if args.limit else items
    cache = run.cache_root() / "model-review" / args.model.replace("/", "_")
    verdicts = review(items, Route(args.model), cache, args.batch, workers=args.workers)
    kept = [verdict for finding_id, verdict in earlier.items() if finding_id not in {v["id"] for v in verdicts}]
    args.out.write_text("".join(json.dumps(verdict) + "\n" for verdict in [*kept, *verdicts]), encoding="utf-8")
    print(f"{len(verdicts)}/{len(items)} findings labelled by {args.model}; {len(kept)} earlier verdicts kept")
    return 0


if __name__ == "__main__":
    sys.exit(main())

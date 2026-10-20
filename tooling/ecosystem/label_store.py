"""Precision label files: loading, writing, validation, and check semantics.

Label files live in eval/ecosystem/labels/<detector>.json (tracked) and key on
finding-ID v3, so they survive rescans. Only a human labeller can make a
verdict final.

A model verdict stays pre_label until an independent check corroborates it: a
reference tool (oracle) confirming or refuting the finding's factual claim, or
a second, different model reaching the same verdict blind. A corroborated
label counts toward precision beside final labels; a contradicted one is
disputed and waits for review. See docs/reference/detector-precision.md.
"""

import json
from pathlib import Path

from tooling.ecosystem import run

LABELS = run.REPO_ROOT / "eval" / "ecosystem" / "labels"
VERDICTS = ("true_positive", "false_positive", "unclear")
DECIDED = ("true_positive", "false_positive")
CLAIMS = ("confirmed", "refuted")
REVIEWED = ("pre_label", "corroborated")


def source_key(source: str) -> str:
    """A check source without its version: `oracle:ruff@0.17.0` -> `oracle:ruff`."""
    return source.split("@", 1)[0]


def check_agreement(entry) -> tuple:
    """(agreeing, contradicting) independent checks of the entry's verdict.

    An oracle that confirms the factual claim agrees with a true positive; one
    that refutes it agrees with a false positive and contradicts a true
    positive. A confirmed claim is neutral toward a false positive, because
    those are mostly scope or idiom judgements (test code, a deliberate lazy
    import) that a linter does not make. A second model agrees when it reaches
    the same decided verdict and contradicts when it reaches the other one.
    """
    verdict = entry.get("verdict")
    if verdict not in DECIDED:
        return 0, 0
    agree = contradict = 0
    for check in entry.get("checks", []):
        if "claim" in check:
            refuted = check["claim"] == "refuted"
            agree += refuted == (verdict == "false_positive")
            contradict += refuted and verdict == "true_positive"
        elif check.get("verdict") in DECIDED:
            agree += check["verdict"] == verdict
            contradict += check["verdict"] != verdict
    return agree, contradict


def reviewed_status(entry) -> str:
    agree, contradict = check_agreement(entry)
    return "corroborated" if agree and not contradict else "pre_label"


def is_disputed(entry) -> bool:
    return entry.get("status") in REVIEWED and check_agreement(entry)[1] > 0


def check_errors(entry) -> list:
    errors, sources = [], set()
    for check in entry.get("checks", []):
        source = str(check.get("source", ""))
        if source_key(source) in sources:
            errors.append(f"duplicate check source {source_key(source)}")
        sources.add(source_key(source))
        if source.startswith("oracle:"):
            if check.get("claim") not in CLAIMS or not check.get("detail"):
                errors.append(f"{source}: an oracle check needs a claim in {CLAIMS} and a detail")
        elif source.startswith("model:"):
            if check.get("verdict") not in VERDICTS or not check.get("reason"):
                errors.append(f"{source}: a model check needs a verdict and a reason")
            if source_key(source) == source_key(str(entry.get("labeller", ""))):
                errors.append(f"{source}: a check must come from a different model than the labeller")
        else:
            errors.append(f"{source}: check source must be oracle:<tool> or model:<id>")
    return errors


def label_errors(document) -> list:
    """Rules the schema subset cannot express."""
    errors = []
    seen = set()
    for index, entry in enumerate(document.get("labels", [])):
        where = f"labels[{index}] {entry.get('id')}"
        if entry.get("id") in seen:
            errors.append(f"{where}: duplicate id")
        seen.add(entry.get("id"))
        errors += [f"{where}: {error}" for error in check_errors(entry)]
        status = entry.get("status")
        if status == "pending":
            if any(key in entry for key in ("verdict", "reason", "labeller")):
                errors.append(f"{where}: pending entries carry no verdict, reason, or labeller")
            continue
        errors += [f"{where}: {status} entry needs {key}" for key in ("verdict", "reason", "labeller")
                   if key not in entry]
        if status == "final" and not str(entry.get("labeller", "")).startswith("human:"):
            errors.append(f"{where}: only a human labeller can make a verdict final")
        if status in REVIEWED and status != reviewed_status(entry):
            errors.append(f"{where}: status {status} does not match its checks ({reviewed_status(entry)})")
    return errors


def load_labels(root=LABELS) -> dict:
    labels = {}
    for path in sorted(Path(root).glob("*.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        errors = label_errors(document)
        if errors:
            raise SystemExit(f"{path}: " + "; ".join(errors))
        labels[document["detector"]] = document["labels"]
    return labels


def write_labels(labels, root=LABELS) -> None:
    Path(root).mkdir(parents=True, exist_ok=True)
    for detector, entries in sorted(labels.items()):
        entries = sorted(entries, key=lambda e: (e["language"], e["repository"], e["path"], e.get("line", 0), e["id"]))
        document = {"schema_version": 1, "detector": detector, "labels": entries}
        (Path(root) / f"{detector}.json").write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


def selected(labels, detectors=None):
    """(detector, entries) pairs in detector order, limited to `detectors` when given."""
    return [(detector, entries) for detector, entries in sorted(labels.items())
            if not detectors or detector in detectors]


def by_id(labels) -> dict:
    return {entry["id"]: entry for entries in labels.values() for entry in entries}


def drop_checks(entry, source) -> None:
    """Remove every check from `source`'s tool or model, whatever its version."""
    entry["checks"] = [check for check in entry.get("checks", []) if source_key(check["source"]) != source_key(source)]
    if not entry["checks"]:
        del entry["checks"]


def read_jsonl(path) -> list:
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines() if line.strip()]

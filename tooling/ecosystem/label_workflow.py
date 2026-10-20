"""Precision labelling workflow: sample findings, record verdicts and checks.

Sampling is deterministic: each detector and language bucket holds at most a
cap of samples, drawn round-robin across repositories in seeded-hash order,
and topping up never resamples a labelled finding.
"""

import hashlib
from collections import defaultdict
from dataclasses import dataclass

from tooling.ecosystem.label_excerpt import excerpt
from tooling.ecosystem.label_store import (
    REVIEWED,
    VERDICTS,
    by_id,
    check_errors,
    drop_checks,
    reviewed_status,
    selected,
)

SAMPLE_CAP = 30
DEFAULT_SEED = "ea-v1"


def finding_entry(repository_id, finding, languages, sampled_from) -> dict:
    location = (finding.get("locations") or [{}])[0]
    language = languages.get(location.get("path")) or finding.get("provenance", {}).get("language") or "repository"
    entry = {
        "id": finding["id"],
        "repository": repository_id,
        "language": language,
        "path": location.get("path") or "",
        "message": finding["message"],
        "status": "pending",
        "sampled_from": sampled_from,
    }
    if location.get("line") is not None:
        entry["line"] = location["line"]
    return entry


def sample_order(seed: str, finding_id: str) -> str:
    return hashlib.sha256(f"{seed}:{finding_id}".encode()).hexdigest()


@dataclass(frozen=True)
class SamplePolicy:
    sampled_from: str
    seed: str = DEFAULT_SEED
    cap: int = SAMPLE_CAP
    detectors: tuple = ()
    # Findings an agreement audit already accepts; never drawn.
    accepted: frozenset = frozenset()


def sample(reports, existing, policy: SamplePolicy) -> dict:
    """Return {detector: [new pending entries]} topping each bucket up to the cap.

    `reports` is {repository: report}; `existing` is {detector: [entries]}.
    A bucket is one detector and file language; already-labelled findings
    still present in the scan count toward the cap, and no labelled finding
    is drawn again.
    """
    buckets = defaultdict(lambda: defaultdict(list))
    for repository_id, report in sorted(reports.items()):
        languages = {file["path"]: file["catalog_language"] for file in report.get("files", [])}
        for finding in report["findings"]:
            if not policy.detectors or finding["detector"] in policy.detectors:
                entry = finding_entry(repository_id, finding, languages, policy.sampled_from)
                buckets[(finding["detector"], entry["language"])][repository_id].append(entry)

    added = defaultdict(list)
    for (detector, language), by_repository in sorted(buckets.items()):
        labelled = {entry["id"] for entry in existing.get(detector, [])}
        live = {entry["id"] for entries in by_repository.values() for entry in entries}
        # Stale labels (finding gone from this scan) do not fill the bucket.
        held = sum(1 for entry in existing.get(detector, []) if entry["language"] == language and entry["id"] in live)
        excluded = labelled | policy.accepted
        queues = [sorted((e for e in entries if e["id"] not in excluded), key=lambda e: sample_order(policy.seed, e["id"]))
                  for _, entries in sorted(by_repository.items())]
        # Round-robin: the i-th pick of every repository precedes any (i+1)-th.
        picks = sorted((rank, position, entry) for position, queue in enumerate(queues)
                       for rank, entry in enumerate(queue))
        added[detector] += [entry for _, _, entry in picks[:max(policy.cap - held, 0)]]
    return {detector: entries for detector, entries in added.items() if entries}


def relocate(labels, reports) -> list:
    """Reset labels whose finding ID now points at a different site.

    Same-named functions in one file are told apart by ordinal, so when one
    finding disappears a sibling can inherit its ID. A label whose live
    finding moved therefore describes another site: it returns to pending
    at the new location and is labelled again. Returns the reset IDs.
    """
    findings = {(repository_id, finding["id"]): finding
                for repository_id, report in reports.items() for finding in report["findings"]}
    reset = []
    for _, entries in selected(labels):
        for entry in entries:
            finding = findings.get((entry["repository"], entry["id"]))
            location = (finding or {}).get("locations", [{}])[0]
            if finding is None or (location.get("path"), location.get("line")) == (entry["path"], entry.get("line")):
                continue
            for key in ("verdict", "reason", "labeller", "checks"):
                entry.pop(key, None)
            entry.update(status="pending", path=location.get("path") or entry["path"], message=finding["message"])
            entry.pop("line", None)
            if location.get("line") is not None:
                entry["line"] = location["line"]
            reset.append(entry["id"])
    return reset


def apply_verdicts(labels, verdicts, labeller) -> tuple:
    """Apply [{id, verdict, reason}] by `labeller`; returns (applied, skipped messages).

    Human verdicts are final. Model verdicts are pre_label and never replace
    a final verdict.
    """
    human = labeller.startswith("human:")
    index = by_id(labels)
    applied, skipped = 0, []
    for verdict in verdicts:
        entry = index.get(verdict.get("id"))
        if entry is None:
            skipped.append(f"{verdict.get('id')}: not sampled")
        elif verdict.get("verdict") not in VERDICTS or not str(verdict.get("reason", "")).strip():
            skipped.append(f"{verdict['id']}: needs a verdict in {VERDICTS} and a reason")
        elif entry["status"] == "final" and not human:
            skipped.append(f"{verdict['id']}: already final")
        else:
            entry.update(verdict=verdict["verdict"], reason=verdict["reason"].strip(), labeller=labeller)
            if not human:
                drop_checks(entry, labeller)  # The labeller's own model cannot also be its check.
            entry["status"] = "final" if human else reviewed_status(entry)
            applied += 1
    return applied, skipped


def check_record(check, source) -> dict:
    if source.startswith("oracle:"):
        return {"source": source, "claim": check.get("claim"), "detail": str(check.get("detail", "")).strip()}
    return {"source": source, "verdict": check.get("verdict"), "reason": str(check.get("reason", "")).strip()}


def apply_checks(labels, checks, source) -> tuple:
    """Replace `source`'s checks with [{id, claim, detail}] or [{id, verdict, reason}].

    Returns (applied, skipped messages). A run of one source replaces all of
    that source's earlier checks, whatever their version, so a finding the
    source no longer covers loses its stale check. Pre-label statuses are then
    recomputed.
    """
    if not source.startswith(("oracle:", "model:")):
        raise ValueError("check source must be oracle:<tool> or model:<id>")
    index = by_id(labels)
    for entry in index.values():
        drop_checks(entry, source)
    applied, skipped = 0, []
    for check in checks:
        entry, record = index.get(check.get("id")), check_record(check, source)
        errors = ["not sampled"] if entry is None else check_errors({**entry, "checks": [record]})
        if entry is None or errors:
            skipped.append(f"{check.get('id')}: {errors[0]}")
            continue
        entry["checks"] = sorted([*entry.get("checks", []), record], key=lambda c: c["source"])
        applied += 1
    for entry in index.values():
        if entry["status"] in REVIEWED:
            entry["status"] = reviewed_status(entry)
    return applied, skipped


def confirm(labels, labeller, detectors=None, ids=None) -> int:
    """Accept reviewed pre-labels as final verdicts by a human labeller."""
    if not labeller.startswith("human:"):
        raise ValueError("only a human labeller can confirm pre-labels")
    chosen = [entry for _, entries in selected(labels, detectors) for entry in entries
              if entry["status"] in REVIEWED and (not ids or entry["id"] in ids)]
    for entry in chosen:
        entry.update(status="final", labeller=labeller)
    return len(chosen)


def bundle_item(detector, entry, finding, blind) -> dict:
    item = {
        "id": entry["id"], "detector": detector, "repository": entry["repository"],
        "language": entry["language"], "path": entry["path"], "line": entry.get("line"),
        # The scan's message, not the one sampled: a count can change since,
        # and a stale message contradicts the current evidence.
        "title": finding.get("title"), "message": finding.get("message", entry["message"]),
        "recommendation": finding.get("recommendation"), "evidence": finding.get("evidence"),
        # A function metric needs the whole body to be checked.
        "excerpt": excerpt(entry["repository"], entry, bool((finding.get("evidence") or {}).get("function"))),
    }
    if not blind:
        item["current"] = {key: entry[key] for key in ("verdict", "reason", "labeller", "checks") if key in entry}
    return item


def context(labels, reports, statuses=("pending",), detectors=None, blind=False) -> list:
    """Labelling bundles; `blind` drops the current verdict and checks."""
    findings = {(repository_id, finding["id"]): finding
                for repository_id, report in reports.items() for finding in report["findings"]}
    return [bundle_item(detector, entry, findings.get((entry["repository"], entry["id"]), {}), blind)
            for detector, entries in selected(labels, detectors) for entry in entries
            if entry["status"] in statuses]

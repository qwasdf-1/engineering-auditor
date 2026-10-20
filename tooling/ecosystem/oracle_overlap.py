"""Oracle coverage of every finding in a scan, and the agreement audit.

`oracles.py overlap` judges every finding, labelled or not, and these helpers
split each detector's findings by oracle claim (confirmed, refuted, other),
count the verified labels in each stratum, and estimate stratified precision.
The gate's use of the audit is in label_precision.accepted_findings.
"""

from collections import Counter, defaultdict

from tooling.ecosystem import label_workflow
from tooling.ecosystem.label_precision import AUDIT_MINIMUM, wilson_lower


def live_entries(reports) -> dict:
    """{detector: [entry]} for every finding in the scan, labelled or not."""
    entries = defaultdict(list)
    for repository_id, report in sorted(reports.items()):
        languages = {file["path"]: file["catalog_language"] for file in report.get("files", [])}
        for finding in report["findings"]:
            entries[finding["detector"]].append(label_workflow.finding_entry(repository_id, finding, languages, ""))
    return dict(entries)


def overlap(entries, planned, checks, labels) -> tuple:
    """(rows, claims): per (detector, language) counts of findings, oracle
    coverage, claims, and the labelled verdicts in each claim stratum
    (`<stratum>_tp`/`<stratum>_fp`, stratum confirmed, refuted, or other),
    and {id: claim record} for every judged finding. `labels` is {id: entry}."""
    tools = {(rule.detector, entry["id"]): rule.tool for rule, entry, _ in planned}
    claims = {}
    for source, records in checks.items():
        for check in records:
            claims[check["id"]] = {"source": source, "claim": check["claim"], "detail": check["detail"]}
    rows = defaultdict(Counter)
    for detector, detector_entries in entries.items():
        for entry in detector_entries:
            counts = rows[(detector, entry["language"])]
            claim = claims.get(entry["id"], {}).get("claim") if (detector, entry["id"]) in tools else None
            counts["findings"] += 1
            counts["covered"] += (detector, entry["id"]) in tools
            counts[claim or ("unjudged" if (detector, entry["id"]) in tools else "uncovered")] += 1
            label = labels.get(entry["id"], {})
            verdict = label.get("verdict") if label.get("status") in ("final", "corroborated") else None
            if verdict in ("true_positive", "false_positive"):
                counts["labelled"] += 1
                counts[f"{claim if claim in ('confirmed', 'refuted') else 'other'}_{verdict[0]}p"] += 1
            if claim:
                claims[entry["id"]].update(detector=detector, **{key: entry.get(key) for key in
                                                                ("repository", "language", "path", "line")})
    return dict(rows), claims


def stratified(counts) -> tuple:
    """(estimate, effective sample). Precision is weighted by stratum size,
    using each stratum's labelled precision; None when a non-empty stratum has
    no labels. The effective sample treats every confirmed finding as a true
    positive when its audit holds at least AUDIT_MINIMUM labels and no false
    positive; otherwise only the audited ones count."""
    sizes = {"confirmed": counts["confirmed"], "refuted": counts["refuted"],
             "other": counts["findings"] - counts["confirmed"] - counts["refuted"]}
    estimate = 0.0
    for stratum, size in sizes.items():
        if not size:
            continue
        labelled = counts[f"{stratum}_tp"] + counts[f"{stratum}_fp"]
        if not labelled:
            return None, None
        estimate += size / counts["findings"] * counts[f"{stratum}_tp"] / labelled
    audited = counts["confirmed_tp"] + counts["confirmed_fp"]
    clean = counts["confirmed_fp"] == 0 and audited >= AUDIT_MINIMUM
    sample = (sizes["confirmed"] if clean else audited) + sum(
        counts[f"{stratum}_{kind}"] for stratum in ("refuted", "other") for kind in ("tp", "fp"))
    return estimate, sample


def overlap_markdown(rows) -> str:
    lines = ["| Detector | Language | Findings | Labelled | Oracle covers | Confirmed | Refuted | Unjudged |",
             "|---|---|---:|---:|---:|---:|---:|---:|"]
    for (detector, language), counts in sorted(rows.items()):
        if counts["covered"]:
            lines.append(f"| `{detector}` | {language} | {counts['findings']} | {counts['labelled']} "
                         f"| {counts['covered']} | {counts['confirmed']} | {counts['refuted']} | {counts['unjudged']} |")
    pooled = defaultdict(Counter)
    for (detector, _), counts in rows.items():
        pooled[detector].update(counts)
    lines += ["", "| Detector | Findings | Confirmed | Agreement audit (TP/FP) | Audit lower bound "
              "| Refuted labelled (TP/FP) | Other labelled (TP/FP) | Stratified precision | Sample if agreement accepted |",
              "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for detector, counts in sorted(pooled.items()):
        if not counts["covered"]:
            continue
        audited = counts["confirmed_tp"] + counts["confirmed_fp"]
        estimate, sample = stratified(counts)
        bound = f"{wilson_lower(counts['confirmed_tp'], audited):.0%}" if audited else "—"
        lines.append(
            f"| `{detector}` | {counts['findings']} | {counts['confirmed']} "
            f"| {counts['confirmed_tp']}/{counts['confirmed_fp']} "
            f"| {bound} "
            f"| {counts['refuted_tp']}/{counts['refuted_fp']} | {counts['other_tp']}/{counts['other_fp']} "
            f"| {'—' if estimate is None else f'{estimate:.0%}'} | {'—' if sample is None else sample} |")
    uncovered = sorted(set(pooled) - {detector for detector, counts in pooled.items() if counts["covered"]})
    lines += ["", f"No oracle: {', '.join(f'`{name}`' for name in uncovered)}."]
    return "\n".join(lines) + "\n"

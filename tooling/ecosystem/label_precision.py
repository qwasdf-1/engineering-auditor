"""Per-detector precision and gating standing from final and corroborated
labels, plus the oracle-confirmed findings a clean agreement audit accepts."""

from collections import Counter, defaultdict

from tooling.ecosystem.label_store import DECIDED, is_disputed

# A detector meets the bar when the 95% lower confidence bound of its
# precision reaches GATING_LOWER_BOUND (22 clean labels at the least); it is
# below the bar when even its best-case point precision misses
# GATING_PRECISION.
GATING_LOWER_BOUND = 0.85
GATING_PRECISION = 0.90
GATING_SAMPLE = 10
# A clean audit of 30 bounds the agreed findings' precision at about 89%.
AUDIT_MINIMUM = 30


def wilson_lower(successes, total, z=1.96) -> float:
    """95% Wilson score lower bound of a proportion."""
    if not total:
        return 0.0
    share, spread = successes / total, z * z / total
    centre = share + spread / 2
    margin = z * ((share * (1 - share) + spread / 4) / total) ** 0.5
    return (centre - margin) / (1 + spread)


def standing(true_positive, false_positive, unverified=0, accepted=0) -> tuple:
    """(labelled precision, standing) against the gating bar.

    `unverified` counts decided labels that are neither final nor
    corroborated. They might be either verdict, so the standing is a sound
    bound: meets_bar only if the lower confidence bound holds even were every
    unverified label a false positive, below_bar only if the point precision
    fails even were every one a true positive, unverified if verifying them
    could reach the bar, and insufficient_sample otherwise. Corroboration
    comes easily for true positives (a linter confirms the measurement) and
    rarely for scope false positives, so counting only verified labels would
    inflate precision. `accepted` counts unlabelled oracle-confirmed findings
    taken as true positives after a clean agreement audit.
    """
    labelled = true_positive + false_positive
    ratio = true_positive / labelled if labelled else None
    correct, total = true_positive + accepted, labelled + accepted + unverified
    if total < GATING_SAMPLE:
        return ratio, "insufficient_sample"
    if (correct + unverified) / total < GATING_PRECISION:
        return ratio, "below_bar"
    if wilson_lower(correct, total) >= GATING_LOWER_BOUND:
        return ratio, "meets_bar"
    if unverified and wilson_lower(correct + unverified, total) >= GATING_LOWER_BOUND:
        return ratio, "unverified"
    return ratio, "insufficient_sample"


def agreement_audit(live, confirmed) -> tuple:
    """(true positives, false positives) among verified labels whose finding
    an oracle confirmed: how often oracle agreement is a correct finding."""
    audit = Counter(entry["verdict"] for entry in live
                    if entry["id"] in confirmed and entry["status"] in ("final", "corroborated"))
    return audit["true_positive"], audit["false_positive"]


def accepted_findings(entries, live, confirmed) -> dict:
    """{id: language} of unlabelled confirmed findings, when the audit holds
    at least AUDIT_MINIMUM labels and no false positive."""
    audit_tp, audit_fp = agreement_audit(live, confirmed)
    if audit_fp or audit_tp < AUDIT_MINIMUM:
        return {}
    labelled = {entry["id"] for entry in entries}
    return {finding_id: language for finding_id, language in confirmed.items() if finding_id not in labelled}


def check_kind(check, verdict):
    """("oracle" | "model", agrees) for a check that can disagree with `verdict`, else None.

    Only a second model's decided verdict and an oracle claim, except a
    confirmed claim on a false positive, can disagree.
    """
    if "claim" in check:
        neutral = check["claim"] == "confirmed" and verdict == "false_positive"
        return None if neutral else ("oracle", (check["claim"] == "refuted") == (verdict == "false_positive"))
    return ("model", check["verdict"] == verdict) if check.get("verdict") in DECIDED else None


def agreement(entries) -> dict:
    """How often independent checks agree with the labeller, per source kind."""
    tally = {"model": [0, 0], "oracle": [0, 0]}
    for entry in entries:
        compared = [check_kind(check, entry.get("verdict")) for check in entry.get("checks", [])]
        for kind, agrees in filter(None, compared if entry.get("verdict") in DECIDED else []):
            tally[kind][0] += agrees
            tally[kind][1] += 1
    return tally


def detector_row(entries, live, confirmed=None) -> dict:
    """`confirmed` is {id: language} of live findings an oracle confirmed."""
    confirmed = confirmed or {}
    accepted = accepted_findings(entries, live, confirmed)
    accepted_by_language = Counter(accepted.values())
    counts, provisional, by_language, tiers = Counter(), Counter(), defaultdict(Counter), Counter()
    for entry in live:
        if entry["status"] in ("final", "corroborated"):
            counts[entry["verdict"]] += 1
            tiers[entry["status"]] += 1
            by_language[entry["language"]][entry["verdict"]] += 1
        else:
            provisional[entry.get("verdict", "pending")] += 1
    pre_decided = provisional["true_positive"] + provisional["false_positive"]
    ratio, bar = standing(counts["true_positive"], counts["false_positive"], pre_decided, len(accepted))
    unverified_by_language = Counter(entry["language"] for entry in live
                                     if entry["status"] not in ("final", "corroborated")
                                     and entry.get("verdict") in DECIDED)
    return {
        "true_positive": counts["true_positive"], "false_positive": counts["false_positive"],
        "unclear": counts["unclear"], "stale": len(entries) - len(live),
        "precision": ratio, "standing": bar, "unverified": pre_decided,
        "audit": list(agreement_audit(live, confirmed)), "accepted": len(accepted),
        "final": tiers["final"], "corroborated": tiers["corroborated"], "disputed": sum(map(is_disputed, live)),
        "provisional": dict(sorted(provisional.items())),
        "provisional_precision": provisional["true_positive"] / pre_decided if pre_decided else None,
        "agreement": agreement(live),
        "by_language": {
            language: {**dict(sorted(by_language[language].items())),
                       "unverified": unverified_by_language[language],
                       "accepted": accepted_by_language[language],
                       "standing": standing(by_language[language]["true_positive"],
                                            by_language[language]["false_positive"],
                                            unverified_by_language[language],
                                            accepted_by_language[language])[1]}
            for language in sorted(set(by_language) | set(unverified_by_language) | set(accepted_by_language))
        },
    }


def restrict(labels, claims, repositories, keep) -> tuple:
    """`labels` and `claims` limited to findings whose repository is in
    `repositories` (`keep`) or outside it (not `keep`)."""
    inside = lambda record: (record.get("repository") in repositories) == keep  # noqa: E731
    return ({detector: [entry for entry in entries if inside(entry)] for detector, entries in labels.items()},
            None if claims is None else {key: record for key, record in claims.items() if inside(record)})


def precision(labels, live_ids=None, claims=None) -> dict:
    """Per-detector precision from final and corroborated labels.

    Uncorroborated pre-labels are reported apart and never count. With
    `live_ids`, labels whose finding is absent from that scan are stale and
    excluded. `claims` is the `oracles.py overlap --json` snapshot
    ({id: {detector, claim, language, ...}}); its confirmed findings feed the
    agreement audit and, when it is clean, count as accepted true positives.
    """
    confirmed = confirmed_by_detector(claims, live_ids)
    return {detector: detector_row(entries, [e for e in entries if live_ids is None or e["id"] in live_ids],
                                   confirmed.get(detector))
            for detector, entries in sorted(labels.items())}


def confirmed_by_detector(claims, live_ids=None) -> dict:
    """{detector: {id: language}} of live findings an oracle confirmed."""
    confirmed = defaultdict(dict)
    for finding_id, record in (claims or {}).items():
        if record["claim"] == "confirmed" and (live_ids is None or finding_id in live_ids):
            confirmed[record["detector"]][finding_id] = record["language"]
    return confirmed


def accepted_ids(labels, live_ids, claims) -> set:
    """Live findings a clean agreement audit accepts, pending samples included.

    Sampling skips them and drops their pending labels: once a detector's
    audit holds, a confirmed finding already counts as a true positive, and a
    pending label for it would only count against the bound as unverified.
    """
    confirmed = confirmed_by_detector(claims, live_ids)
    accepted = set()
    for detector, entries in labels.items():
        decided = [entry for entry in entries if entry["status"] != "pending"]
        live = [entry for entry in decided if entry["id"] in live_ids]
        accepted |= set(accepted_findings(decided, live, confirmed.get(detector, {})))
    return accepted


def percent(value) -> str:
    return "—" if value is None else f"{value:.0%}"


def ratio_cell(pair) -> str:
    agree, compared = pair
    return f"{agree}/{compared}" if compared else "—"


def precision_markdown(table) -> str:
    lines = [(f"Gating bar: the 95% lower confidence bound of precision >= {GATING_LOWER_BOUND:.0%} over final or "
              "corroborated TP+FP labels plus accepted oracle-confirmed findings, holding even if every unverified "
              f"pre-label were a false positive; below the bar when the best-case precision is under "
              f"{GATING_PRECISION:.0%}. Confirmed findings are accepted only after an agreement audit of at least "
              f"{AUDIT_MINIMUM} verified labels with no false positive."), "",
             ("| Detector | TP | FP | Unclear | Precision | Agreement audit | Accepted | Standing "
              "| Final / corroborated | Disputed | Uncorroborated | Oracle agrees | Model agrees |"),
             "|---|---:|---:|---:|---:|---:|---:|---|---:|---:|---|---:|---:|"]
    for detector, row in table.items():
        provisional = ", ".join(f"{key} {value}" for key, value in row["provisional"].items()) or "—"
        audit = "{}/{}".format(*row["audit"]) if sum(row["audit"]) else "—"
        lines.append(f"| `{detector}` | {row['true_positive']} | {row['false_positive']} | {row['unclear']} "
                     f"| {percent(row['precision'])} | {audit} | {row['accepted']} | {row['standing']} "
                     f"| {row['final']} / {row['corroborated']} "
                     f"| {row['disputed']} | {provisional} | {ratio_cell(row['agreement']['oracle'])} "
                     f"| {ratio_cell(row['agreement']['model'])} |")
    return "\n".join(lines) + "\n"

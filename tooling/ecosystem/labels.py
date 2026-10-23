#!/usr/bin/env python3
"""Sample, label, and measure detector precision on the ecosystem corpus.

Commands:
  sample --scan L [--claims F]
                     add pending samples from scan L to the label files, and
                     reset labels whose finding ID moved to another site; with
                     F, skip (and drop pending) findings a clean agreement
                     audit already accepts
  context --scan L   print pending or pre-labelled samples with source excerpts
                     as JSON Lines, for a human or model labeller
  import FILE --labeller human:NAME|model:ID
                     apply {id, verdict, reason} JSON Lines verdicts
  checks FILE --source oracle:TOOL|model:ID [--detector D..]
                     record independent checks of pre-labels ({id, claim, detail}
                     for an oracle, {id, verdict, reason} for a second model); a
                     run replaces the source's earlier checks, only for D if given
  confirm --labeller human:NAME [--detector D..] [--ids ID..]
                     accept reviewed pre-labels as final human verdicts
  precision [--scan L] [--claims F] [--json F] [--split all|development|holdout]
                     print per-detector precision from final and corroborated
                     labels, plus the oracle-confirmed findings in F that a
                     clean agreement audit accepts; --split measures on the
                     held-out repositories or the rest

The label files and their rules are in label_store.py, sampling and verdict
recording in label_workflow.py, review excerpts in label_excerpt.py, and the
precision table in label_precision.py.
See docs/reference/detector-precision.md.
"""

import argparse
import json
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tooling.ecosystem import label_precision, label_store, label_workflow, run  # noqa: E402
from tooling.ecosystem.corpus_holdout import holdout_ids  # noqa: E402


def report_skipped(skipped) -> None:
    for message in skipped:
        print(f"skipped {message}", file=sys.stderr)


def sample_command(args, labels) -> str:
    reports = run.load_results(args.scan)
    reset = label_workflow.relocate(labels, reports)
    accepted, dropped = frozenset(), 0
    if args.claims:
        live = {finding["id"] for report in reports.values() for finding in report["findings"]}
        claims = json.loads(args.claims.read_text(encoding="utf-8"))
        accepted = frozenset(label_precision.accepted_ids(labels, live, claims))
        for detector, entries in labels.items():
            kept = [entry for entry in entries if not (entry["status"] == "pending" and entry["id"] in accepted)]
            dropped += len(entries) - len(kept)
            labels[detector] = kept
    policy = label_workflow.SamplePolicy(args.scan, args.seed, args.cap, tuple(args.detector or ()), accepted)
    added = label_workflow.sample(reports, labels, policy)
    for detector, entries in added.items():
        labels.setdefault(detector, []).extend(entries)
    label_store.write_labels(labels)
    return (f"sampled {sum(map(len, added.values()))} findings across {len(added)} detectors; "
            f"{len(reset)} labels reset because their finding moved; "
            f"{dropped} pending samples dropped as accepted")


def context_command(args, labels) -> str:
    bundle = label_workflow.context(labels, run.load_results(args.scan), tuple(args.status), args.detector,
                                    args.blind)
    return "\n".join(json.dumps(item) for item in bundle)


def import_command(args, labels) -> str:
    applied, skipped = label_workflow.apply_verdicts(labels, label_store.read_jsonl(args.verdicts), args.labeller)
    label_store.write_labels(labels)
    report_skipped(skipped)
    return f"applied {applied} verdicts as {'final' if args.labeller.startswith('human:') else 'pre_label'}"


def checks_command(args, labels) -> str:
    applied, skipped = label_workflow.apply_checks(labels, label_store.read_jsonl(args.checks), args.source,
                                                   args.detector)
    label_store.write_labels(labels)
    report_skipped(skipped)
    corroborated = sum(entry["status"] == "corroborated" for entry in label_store.by_id(labels).values())
    return f"applied {applied} checks from {args.source}; {corroborated} labels corroborated"


def confirm_command(args, labels) -> str:
    confirmed = label_workflow.confirm(labels, args.labeller, args.detector, set(args.ids or ()))
    label_store.write_labels(labels)
    return f"confirmed {confirmed} pre-labels as final"


def precision_command(args, labels) -> str:
    live = None if args.scan is None else {f["id"] for scan in args.scan
                                           for report in run.load_results(scan).values()
                                           for f in report["findings"]}
    claims = json.loads(args.claims.read_text(encoding="utf-8")) if args.claims else None
    chosen = dict(label_store.selected(labels, args.detector))
    if args.split != "all":
        chosen, claims = label_precision.restrict(chosen, claims, holdout_ids(), args.split == "holdout")
    table = label_precision.precision(chosen, live, claims)
    if args.json:
        args.json.write_text(json.dumps(table, indent=2) + "\n", encoding="utf-8")
    return label_precision.precision_markdown(table).rstrip("\n")


def labeller(value: str) -> str:
    if not value.startswith(("human:", "model:")):
        raise argparse.ArgumentTypeError("must be human:<name> or model:<id>")
    return value


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = root.add_subparsers(dest="command", required=True)
    sampling = commands.add_parser("sample")
    sampling.add_argument("--scan", required=True, help="scan label to sample from")
    sampling.add_argument("--seed", default=label_workflow.DEFAULT_SEED)
    sampling.add_argument("--cap", type=int, default=label_workflow.SAMPLE_CAP)
    sampling.add_argument("--claims", type=Path,
                          help="oracle claims snapshot; skip findings a clean agreement audit accepts")
    bundling = commands.add_parser("context")
    bundling.add_argument("--scan", required=True)
    bundling.add_argument("--status", nargs="+", default=["pending"],
                          choices=["pending", "pre_label", "corroborated", "final"])
    bundling.add_argument("--blind", action="store_true", help="omit current verdicts and checks (for a second labeller)")
    measuring = commands.add_parser("precision")
    measuring.add_argument("--scan", nargs="+",
                           help="exclude labels absent from these scans (the library scan and the application scan)")
    measuring.add_argument("--json", type=Path, help="also write the table here")
    measuring.add_argument("--split", choices=["all", "development", "holdout"], default="all",
                           help="measure on every repository, or only on the held-out ones or the rest")
    measuring.add_argument("--claims", type=Path,
                           help="oracles.py overlap --json snapshot; audited oracle agreement adds accepted findings")
    for command in (sampling, bundling, measuring):
        command.add_argument("--detector", nargs="+", help="detectors to include")
    imported = commands.add_parser("import")
    imported.add_argument("verdicts", type=Path, help="JSON Lines of {id, verdict, reason}")
    imported.add_argument("--labeller", required=True, type=labeller, help="human:<name> or model:<id>")
    checking = commands.add_parser("checks")
    checking.add_argument("checks", type=Path, help="JSON Lines of {id, claim, detail} or {id, verdict, reason}")
    checking.add_argument("--source", required=True, help="oracle:<tool>[@version] or model:<id>")
    checking.add_argument("--detector", nargs="+",
                          help="replace this source's checks only for these detectors (default: all)")
    confirming = commands.add_parser("confirm")
    confirming.add_argument("--labeller", required=True, help="human:<name>")
    confirming.add_argument("--detector", nargs="+", help="detectors to confirm (default: all)")
    confirming.add_argument("--ids", nargs="+", help="finding IDs to confirm (default: all pre-labels)")
    handlers = {"sample": sample_command, "context": context_command, "import": import_command,
                "checks": checks_command, "confirm": confirm_command, "precision": precision_command}
    for name, handler in handlers.items():
        commands.choices[name].set_defaults(handler=handler)
    return root


def main(argv=None) -> int:
    args = parser().parse_args(argv)
    try:
        output = args.handler(args, label_store.load_labels())
    except ValueError as error:
        raise SystemExit(str(error)) from error
    if output:
        print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())

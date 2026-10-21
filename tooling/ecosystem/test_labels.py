"""Behavior tests for ecosystem precision labelling (no network)."""

import json
import os
import tempfile
import unittest
from pathlib import Path

from tooling.ecosystem import label_excerpt, label_precision, label_store, label_workflow
from tooling.ecosystem.test_manifest import REPO_ROOT, schema_errors

SCHEMA = json.loads((REPO_ROOT / "contracts" / "ecosystem-labels.schema.json").read_text(encoding="utf-8"))


def policy(**knobs):
    return label_workflow.SamplePolicy("scan", **knobs)


def finding(index, detector="too_many_parameters", path="a.py", line=1):
    return {"id": f"EA-{index:016x}", "detector": detector, "message": f"m{index}",
            "title": "t", "recommendation": "r", "locations": [{"path": path, "line": line}]}


def report(findings, languages):
    return {"findings": findings,
            "files": [{"path": path, "catalog_language": language} for path, language in languages.items()]}


def corpus(per_repository=25):
    return {
        "alpha": report([finding(i) for i in range(per_repository)], {"a.py": "python"}),
        "beta": report([finding(1000 + i) for i in range(per_repository)], {"a.py": "python"}),
    }


def labelled(entry, verdict, labeller="human:reviewer", status="final"):
    return {**entry, "status": status, "verdict": verdict, "reason": "because", "labeller": labeller}


class SampleTest(unittest.TestCase):
    def test_sample_is_seeded_capped_and_balanced_across_repositories(self):
        drawn = label_workflow.sample(corpus(), {}, policy(cap=30, seed="s"))["too_many_parameters"]
        self.assertEqual(len(drawn), 30)
        self.assertEqual(sum(entry["repository"] == "alpha" for entry in drawn), 15)
        self.assertEqual(drawn, label_workflow.sample(corpus(), {}, policy(cap=30, seed="s"))["too_many_parameters"])
        self.assertNotEqual([e["id"] for e in drawn],
                            [e["id"] for e in label_workflow.sample(corpus(), {}, policy(cap=30, seed="t"))["too_many_parameters"]])

    def test_small_buckets_are_sampled_whole_and_languages_are_separate_buckets(self):
        reports = {"alpha": report([finding(1), finding(2, path="b.go")], {"a.py": "python", "b.go": "go"})}
        drawn = label_workflow.sample(reports, {}, policy(cap=30))["too_many_parameters"]
        self.assertEqual(sorted(entry["language"] for entry in drawn), ["go", "python"])
        self.assertTrue(all(entry["status"] == "pending" for entry in drawn))

    def test_accepted_findings_are_never_drawn(self):
        reports = {"alpha": report([finding(1), finding(2)], {"a.py": "python"})}
        accepted = frozenset({finding(1)["id"]})
        drawn = label_workflow.sample(reports, {}, policy(cap=30, accepted=accepted))["too_many_parameters"]
        self.assertEqual([entry["id"] for entry in drawn], [finding(2)["id"]])

    def test_accepted_ids_follow_a_clean_audit_and_include_pending_samples(self):
        def label(index, verdict="true_positive", status="corroborated"):
            return {"id": finding(index)["id"], "language": "python", "verdict": verdict, "status": status}
        audited = [label(index) for index in range(label_precision.AUDIT_MINIMUM)]
        pending = {**label(100), "status": "pending"}
        claims = {finding(index)["id"]: {"detector": "d", "claim": "confirmed", "language": "python"}
                  for index in [*range(label_precision.AUDIT_MINIMUM), 100, 101]}
        live = set(claims)
        accepted = label_precision.accepted_ids({"d": [*audited, pending]}, live, claims)
        self.assertEqual(accepted, {finding(100)["id"], finding(101)["id"]})
        dirty = [*audited[1:], label(0, verdict="false_positive")]
        self.assertEqual(label_precision.accepted_ids({"d": dirty}, live, claims), set())

    def test_top_up_counts_existing_labels_and_never_redraws_them(self):
        first = label_workflow.sample(corpus(), {}, policy(cap=10))["too_many_parameters"]
        topped = label_workflow.sample(corpus(), {"too_many_parameters": first}, policy(cap=30))["too_many_parameters"]
        self.assertEqual(len(topped), 20)
        self.assertFalse({e["id"] for e in first} & {e["id"] for e in topped})


    def test_stale_labels_do_not_fill_a_bucket(self):
        first = label_workflow.sample(corpus(), {}, policy(cap=10))["too_many_parameters"]
        rescanned = {"alpha": report([finding(i) for i in range(500, 525)], {"a.py": "python"})}
        topped = label_workflow.sample(rescanned, {"too_many_parameters": first}, policy(cap=10))
        self.assertEqual(len(topped["too_many_parameters"]), 10)


class VerdictTest(unittest.TestCase):
    def setUp(self):
        self.labels = label_workflow.sample(corpus(5), {}, policy(cap=30))
        self.first = self.labels["too_many_parameters"][0]

    def test_model_verdicts_are_pre_labels_and_never_replace_final(self):
        verdict = [{"id": self.first["id"], "verdict": "false_positive", "reason": "grouped params"}]
        self.assertEqual(label_workflow.apply_verdicts(self.labels, verdict, "model:free-route")[0], 1)
        self.assertEqual(self.first["status"], "pre_label")
        label_workflow.apply_verdicts(self.labels, [{**verdict[0], "verdict": "true_positive"}], "human:reviewer")
        self.assertEqual((self.first["status"], self.first["verdict"]), ("final", "true_positive"))
        applied, skipped = label_workflow.apply_verdicts(self.labels, verdict, "model:free-route")
        self.assertEqual((applied, self.first["verdict"]), (0, "true_positive"))
        self.assertIn("already final", skipped[0])

    def test_confirm_promotes_only_reviewed_pre_labels_for_humans(self):
        second = self.labels["too_many_parameters"][1]
        for entry in (self.first, second):
            label_workflow.apply_verdicts(self.labels, [{"id": entry["id"], "verdict": "true_positive", "reason": "r"}],
                                  "model:free-route")
        with self.assertRaises(ValueError):
            label_workflow.confirm(self.labels, "model:free-route")
        self.assertEqual(label_workflow.confirm(self.labels, "human:reviewer", ids={self.first["id"]}), 1)
        self.assertEqual((self.first["status"], self.first["labeller"]), ("final", "human:reviewer"))
        self.assertEqual(second["status"], "pre_label")
        self.assertEqual(label_workflow.confirm(self.labels, "human:reviewer"), 1)

    def test_rejects_unknown_ids_and_missing_reasons(self):
        applied, skipped = label_workflow.apply_verdicts(
            self.labels, [{"id": "EA-ffffffffffffffff", "verdict": "unclear", "reason": "x"},
                          {"id": self.first["id"], "verdict": "true_positive", "reason": " "}], "human:reviewer")
        self.assertEqual(applied, 0)
        self.assertEqual(len(skipped), 2)

    def test_written_files_match_the_schema_and_rules(self):
        label_workflow.apply_verdicts(self.labels, [{"id": self.first["id"], "verdict": "unclear", "reason": "r"}],
                              "model:free-route")
        with tempfile.TemporaryDirectory() as root:
            label_store.write_labels(self.labels, root)
            document = json.loads((Path(root) / "too_many_parameters.json").read_text(encoding="utf-8"))
            self.assertEqual(schema_errors(document, SCHEMA), [])
            self.assertEqual(label_store.label_errors(document), [])
            self.assertEqual(label_store.load_labels(root), {"too_many_parameters": document["labels"]})
            document["labels"][0] = labelled(document["labels"][0], "true_positive", "model:free-route")
            self.assertIn("only a human", label_store.label_errors(document)[0])

    def test_tracked_label_files_are_valid(self):
        for path in sorted(label_store.LABELS.glob("*.json")):
            document = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(schema_errors(document, SCHEMA) + label_store.label_errors(document), [], path.name)
            self.assertEqual(path.stem, document["detector"])


class RelocateTest(unittest.TestCase):
    def test_a_label_whose_finding_moved_returns_to_pending_at_the_new_site(self):
        reports = corpus(3)
        sampled = label_workflow.sample(reports, {}, policy(cap=30))
        entries = sampled["too_many_parameters"]
        label_workflow.apply_verdicts(sampled, [{"id": e["id"], "verdict": "true_positive", "reason": "r"}
                                                for e in entries], "model:free-route")
        moved = reports["alpha"]["findings"][0]
        moved["locations"][0]["line"] = 99
        reset = label_workflow.relocate(sampled, reports)
        self.assertEqual(reset, [moved["id"]])
        entry = next(e for e in entries if e["id"] == moved["id"])
        self.assertEqual((entry["status"], entry["line"], "verdict" in entry), ("pending", 99, False))
        self.assertEqual(label_workflow.relocate(sampled, reports), [])


class PrecisionTest(unittest.TestCase):
    def test_precision_counts_final_labels_and_applies_the_gating_bar(self):
        entries = label_workflow.sample(corpus(), {}, policy(cap=30))["too_many_parameters"]
        entries[:9] = [labelled(e, "true_positive") for e in entries[:9]]
        row = label_precision.precision({"d": entries})["d"]
        self.assertEqual((row["standing"], row["provisional"]), ("insufficient_sample", {"pending": 21}))
        entries[9:11] = [labelled(entries[9], "false_positive"), labelled(entries[10], "unclear")]
        row = label_precision.precision({"d": entries})["d"]
        self.assertEqual((row["true_positive"], row["false_positive"], row["unclear"]), (9, 1, 1))
        # 9/10 is 90%, but its lower confidence bound is about 60%.
        self.assertEqual((row["precision"], row["standing"]), (0.9, "insufficient_sample"))
        entries[11] = labelled(entries[11], "true_positive", "model:free-route", "pre_label")
        row = label_precision.precision({"d": entries})["d"]
        self.assertEqual((row["standing"], row["unverified"], row["provisional"]["true_positive"]),
                         ("insufficient_sample", 1, 1))
        entries[12] = labelled(entries[12], "false_positive")
        self.assertEqual(label_precision.precision({"d": entries})["d"]["standing"], "below_bar")
        stale = label_precision.precision({"d": entries}, live_ids={e["id"] for e in entries[1:]})["d"]
        self.assertEqual((stale["stale"], stale["true_positive"]), (1, 8))
        self.assertIn("| `d` | 9 | 2 | 1 | 82% | — | 0 | below_bar | 12 / 0 |",
                      label_precision.precision_markdown(label_precision.precision({"d": entries})))
        self.assertEqual(label_precision.precision({"d": entries})["d"]["provisional_precision"], 1.0)

    def test_standing_is_a_sound_lower_bound_over_unverified_labels(self):
        self.assertEqual(label_precision.standing(5, 0, 4)[1], "insufficient_sample")
        self.assertEqual(label_precision.standing(22, 0)[1], "meets_bar")          # lower bound 85.1%
        self.assertEqual(label_precision.standing(21, 0)[1], "insufficient_sample")  # lower bound 84.5%
        self.assertEqual(label_precision.standing(29, 1)[1], "insufficient_sample")  # one error needs ~40 labels
        self.assertEqual(label_precision.standing(39, 1)[1], "meets_bar")
        self.assertEqual(label_precision.standing(20, 0, 2)[1], "unverified")      # 22/22 would meet it
        self.assertEqual(label_precision.standing(5, 3, 20)[1], "below_bar")       # 25/28 < 90% at best
        self.assertEqual(label_precision.standing(0, 0, 30)[1], "unverified")
        self.assertEqual(label_precision.standing(5, 0, 0, accepted=40)[1], "meets_bar")
        self.assertAlmostEqual(label_precision.wilson_lower(10, 10), 0.722, places=3)

    def test_a_clean_agreement_audit_accepts_unlabelled_confirmed_findings(self):
        entries = [labelled(e, "true_positive", status="final")
                   for e in label_workflow.sample(corpus(), {}, policy(cap=30))["too_many_parameters"]]
        claims = {e["id"]: {"detector": "d", "claim": "confirmed", "language": "python"} for e in entries}
        claims.update({f"EA-extra{i}": {"detector": "d", "claim": "confirmed", "language": "go"} for i in range(10)})
        claims["EA-refuted"] = {"detector": "d", "claim": "refuted", "language": "go"}
        row = label_precision.precision({"d": entries}, claims=claims)["d"]
        self.assertEqual((row["audit"], row["accepted"], row["by_language"]["go"]["accepted"]), ([30, 0], 10, 10))
        self.assertEqual(row["standing"], "meets_bar")
        entries[0] = labelled(entries[0], "false_positive")
        row = label_precision.precision({"d": entries}, claims=claims)["d"]
        self.assertEqual((row["audit"], row["accepted"]), ([29, 1], 0))
        live = {e["id"] for e in entries}
        self.assertEqual(label_precision.precision({"d": entries[1:]}, live, claims)["d"]["accepted"], 0)


class CheckTest(unittest.TestCase):
    def setUp(self):
        self.labels = label_workflow.sample(corpus(), {}, policy(cap=30))
        self.entries = self.labels["too_many_parameters"]
        label_workflow.apply_verdicts(self.labels, [{"id": e["id"], "verdict": "true_positive", "reason": "r"}
                                            for e in self.entries[:12]], "model:primary")
        label_workflow.apply_verdicts(self.labels, [{"id": self.entries[12]["id"], "verdict": "false_positive",
                                             "reason": "r"}], "model:primary")

    def oracle(self, claims, source="oracle:lizard@1.0"):
        return label_workflow.apply_checks(self.labels, [{"id": self.entries[i]["id"], "claim": claim, "detail": "d"}
                                                 for i, claim in claims.items()], source)

    def test_oracle_claims_corroborate_or_dispute_by_verdict(self):
        self.assertEqual(self.oracle({0: "confirmed", 1: "refuted", 12: "confirmed"})[0], 3)
        statuses = [self.entries[i]["status"] for i in (0, 1, 12)]
        # A confirmed claim is neutral toward a (scope) false positive.
        self.assertEqual(statuses, ["corroborated", "pre_label", "pre_label"])
        self.assertTrue(label_store.is_disputed(self.entries[1]))
        self.oracle({12: "refuted"}, "oracle:ruff@1")
        self.assertEqual(self.entries[12]["status"], "corroborated")

    def test_a_source_run_replaces_its_earlier_checks_across_versions(self):
        self.oracle({0: "confirmed"})
        self.oracle({1: "confirmed"}, "oracle:lizard@2.0")
        self.assertNotIn("checks", self.entries[0])
        self.assertEqual(self.entries[0]["status"], "pre_label")
        self.assertEqual(self.entries[1]["checks"][0]["source"], "oracle:lizard@2.0")

    def test_a_detector_scoped_run_keeps_other_detectors_checks(self):
        self.oracle({0: "confirmed"})
        check = {"id": self.entries[1]["id"], "claim": "confirmed", "detail": "d"}
        applied, skipped = label_workflow.apply_checks(self.labels, [check], "oracle:lizard@2.0", ["other"])
        self.assertEqual((applied, self.entries[0]["status"]), (0, "corroborated"))
        self.assertIn("outside the selected detectors", skipped[0])
        applied, _ = label_workflow.apply_checks(self.labels, [check], "oracle:lizard@2.0", ["too_many_parameters"])
        self.assertEqual((applied, self.entries[0]["status"]), (1, "pre_label"))

    def test_second_model_must_be_independent_and_can_dispute(self):
        applied, skipped = label_workflow.apply_checks(
            self.labels, [{"id": self.entries[0]["id"], "verdict": "true_positive", "reason": "r"}], "model:primary")
        self.assertEqual(applied, 0)
        self.assertIn("different model", skipped[0])
        label_workflow.apply_checks(self.labels, [{"id": self.entries[0]["id"], "verdict": "true_positive", "reason": "r"},
                                          {"id": self.entries[1]["id"], "verdict": "false_positive", "reason": "r"}],
                            "model:second")
        self.assertEqual((self.entries[0]["status"], self.entries[1]["status"]), ("corroborated", "pre_label"))
        self.assertTrue(label_store.is_disputed(self.entries[1]))
        # Relabelling by the checking model drops its own check.
        label_workflow.apply_verdicts(self.labels, [{"id": self.entries[0]["id"], "verdict": "true_positive",
                                             "reason": "r"}], "model:second")
        self.assertEqual(self.entries[0]["status"], "pre_label")

    def test_corroborated_labels_count_and_files_stay_valid(self):
        self.oracle({i: "confirmed" for i in range(10)})
        self.oracle({10: "refuted"}, "oracle:ruff@1")
        row = label_precision.precision(self.labels)["too_many_parameters"]
        self.assertEqual((row["true_positive"], row["corroborated"], row["disputed"]), (10, 10, 1))
        self.assertEqual((row["unverified"], row["agreement"]["oracle"]), (3, [10, 11]))
        self.assertEqual((row["standing"], row["by_language"]["python"]["standing"]),
                         ("insufficient_sample", "insufficient_sample"))
        self.assertEqual(row["by_language"]["python"]["unverified"], row["unverified"])
        self.assertEqual(label_workflow.confirm(self.labels, "human:reviewer", ids={self.entries[0]["id"]}), 1)
        with tempfile.TemporaryDirectory() as root:
            label_store.write_labels(self.labels, root)
            document = json.loads((Path(root) / "too_many_parameters.json").read_text(encoding="utf-8"))
            self.assertEqual(schema_errors(document, SCHEMA) + label_store.label_errors(document), [])
            unchecked = next(e for e in document["labels"] if e["status"] == "pre_label" and "checks" not in e)
            unchecked["status"] = "corroborated"
            self.assertTrue(any("does not match" in error for error in label_store.label_errors(document)))


class ContextTest(unittest.TestCase):
    def test_context_includes_finding_fields_and_marked_excerpt(self):
        with tempfile.TemporaryDirectory() as cache:
            source = Path(cache) / "repos" / "alpha" / "a.py"
            source.parent.mkdir(parents=True)
            source.write_text("".join(f"line {n}\n" for n in range(1, 31)))
            reports = {"alpha": report([finding(1, line=10)], {"a.py": "python"})}
            sampled = label_workflow.sample(reports, {}, policy(cap=30))
            reports["alpha"]["findings"][0]["message"] = "rescanned"
            previous = os.environ.get("EA_ECOSYSTEM_CACHE")
            os.environ["EA_ECOSYSTEM_CACHE"] = cache
            try:
                [item] = label_workflow.context(sampled, reports)
            finally:
                if previous is None:
                    del os.environ["EA_ECOSYSTEM_CACHE"]
                else:
                    os.environ["EA_ECOSYSTEM_CACHE"] = previous
            self.assertEqual((item["title"], item["recommendation"], item["message"]), ("t", "r", "rescanned"))
            self.assertIn("   10> line 10", item["excerpt"])
            self.assertTrue(item["excerpt"].startswith("    5  line 5"))
            self.assertEqual(label_workflow.context(sampled, reports, ("final",)), [])
            self.assertNotIn("current", label_workflow.context(sampled, reports, blind=True)[0])

    def test_function_findings_show_the_whole_function(self):
        java = ["// c", "int f(int x) {", *[f"  if (x == {n}) {{ x++; }}" for n in range(30)], "}", "int g() { return 1; }"]
        self.assertEqual(label_excerpt.function_end(java, 1), 33)
        python = ["@cache", "def f(x,", "      y):", *["    x += 1"] * 25, "", "    return x", "def g():", "    pass"]
        self.assertEqual(label_excerpt.function_end(python, 0), 30)
        endless = ["void f() {"] + ["  x();"] * 500
        self.assertEqual(label_excerpt.function_end(endless, 0), label_excerpt.FUNCTION_EXCERPT_MAX)


if __name__ == "__main__":
    unittest.main()

"""Behavior tests for the ecosystem runner (no network)."""

import subprocess
import tempfile
import unittest
from pathlib import Path

from tooling.ecosystem import run


def finding(finding_id, detector, path, line=1, message="m"):
    return {"id": finding_id, "detector": detector, "message": message,
            "locations": [{"path": path, "line": line}]}


def report(findings, languages):
    return {"findings": findings,
            "files": [{"path": path, "catalog_language": language} for path, language in languages.items()]}


REPOSITORY = {"id": "demo", "language": "python", "commit": "0" * 40}


class SummaryTest(unittest.TestCase):
    def test_counts_per_detector_language_and_repository(self):
        scanned = report(
            [finding("EA-1", "too_many_parameters", "a.py"),
             finding("EA-2", "too_many_parameters", "web/b.js"),
             finding("EA-3", "god_module", "pkg")],
            {"a.py": "python", "web/b.js": "javascript"},
        )
        summary = run.summarize({"demo": (REPOSITORY, scanned)})
        parameters = summary["by_detector"]["too_many_parameters"]
        self.assertEqual(parameters["total"], 2)
        self.assertEqual(parameters["by_language"], {"javascript": 1, "python": 1})
        # Findings without a matching file are repository-level.
        self.assertEqual(summary["by_detector"]["god_module"]["by_language"], {"repository": 1})
        self.assertEqual(summary["repositories"][0]["findings"], 3)
        self.assertIn("| `too_many_parameters` | 2 |", run.summary_markdown(summary, "x"))


class DiffTest(unittest.TestCase):
    def test_reports_new_resolved_and_unchanged_by_finding_id(self):
        before = {"demo": report([finding("EA-1", "a", "x.py"), finding("EA-2", "b", "y.py")], {})}
        after = {"demo": report([finding("EA-1", "a", "x.py"), finding("EA-3", "b", "z.py", 7)], {})}
        diff = run.diff_reports(before, after)
        self.assertEqual(diff["a"], {"new": [], "resolved": [], "rekeyed": [], "unchanged": 1})
        self.assertEqual([f["id"] for f in diff["b"]["new"]], ["EA-3"])
        self.assertEqual([f["id"] for f in diff["b"]["resolved"]], ["EA-2"])
        markdown = run.diff_markdown(diff, "old", "new")
        self.assertIn("New: 1. Resolved: 1. Re-keyed: 0.", markdown)
        self.assertIn("demo:z.py:7", markdown)
        self.assertNotIn("| `a` |", markdown)

    def test_same_site_id_change_is_rekeyed_not_new_plus_resolved(self):
        before = {"demo": report([finding("EA-old", "a", "x.go", 3), finding("EA-gone", "a", "x.go", 9)], {})}
        after = {"demo": report([finding("EA-new", "a", "x.go", 3)], {})}
        diff = run.diff_reports(before, after)["a"]
        self.assertEqual([(f["previous_id"], f["id"]) for f in diff["rekeyed"]], [("EA-old", "EA-new")])
        self.assertEqual([f["id"] for f in diff["resolved"]], ["EA-gone"])
        self.assertEqual(diff["new"], [])


class TargetedTest(unittest.TestCase):
    def test_application_reports_keep_only_their_detectors(self):
        findings = [finding("EA-a", "network_call_without_timeout", "a.py"), finding("EA-b", "magic_literal_cluster", "a.py")]
        application = {**REPOSITORY, "detectors": ["network_call_without_timeout"]}
        self.assertEqual([f["id"] for f in run.targeted(report(findings, {}), application)["findings"]], ["EA-a"])
        self.assertEqual(len(run.targeted(report(findings, {}), REPOSITORY)["findings"]), 2)


class FetchTest(unittest.TestCase):
    def test_fetches_pinned_commit_and_repairs_a_dirty_checkout(self):
        with tempfile.TemporaryDirectory() as scratch:
            origin = Path(scratch) / "origin"
            origin.mkdir()
            for args in (["init", "-q"], ["config", "user.email", "t@t"], ["config", "user.name", "t"],
                         ["config", "uploadpack.allowAnySHA1InWant", "true"]):
                subprocess.run(["git", *args], cwd=origin, check=True)
            (origin / "a.txt").write_text("one\n")
            subprocess.run(["git", "add", "."], cwd=origin, check=True)
            subprocess.run(["git", "commit", "-qm", "one"], cwd=origin, check=True)
            pinned = run.git(["rev-parse", "HEAD"], origin)
            (origin / "a.txt").write_text("two\n")
            subprocess.run(["git", "commit", "-qam", "two"], cwd=origin, check=True)

            checkout = Path(scratch) / "checkout"
            run.fetch_repository(origin.as_uri(), pinned, checkout)
            self.assertEqual((checkout / "a.txt").read_text(), "one\n")
            (checkout / "a.txt").write_text("edited\n")
            self.assertFalse(run.checkout_is_pinned(checkout, pinned))
            run.fetch_repository(origin.as_uri(), pinned, checkout)
            self.assertTrue(run.checkout_is_pinned(checkout, pinned))
            with self.assertRaises(RuntimeError):
                run.fetch_repository(origin.as_uri(), "f" * 40, Path(scratch) / "missing")


if __name__ == "__main__":
    unittest.main()

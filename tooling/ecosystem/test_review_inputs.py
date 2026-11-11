"""Behavior tests for review-input preparation (no network: a local
`file://` source and a stub auditor)."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tooling.ecosystem import review_inputs

DAY = 86_400
HEAD_TIME = 1_767_225_600  # 2026-01-01

STUB_AUDITOR = """#!/usr/bin/env python3
import json, subprocess, sys
args = sys.argv[1:]
value = lambda flag: args[args.index(flag) + 1]
root = value("--repo-root")
if args[0] == "scan":
    with open(value("--output"), "w") as out:
        json.dump({"findings": []}, out)
elif args[:2] == ["review", "history"]:
    count = subprocess.run(["git", "rev-list", "--count", "HEAD"], cwd=root,
                           capture_output=True, text=True, check=True).stdout.strip()
    with open(value("--output"), "w") as out:
        json.dump({"commits_fetched": int(count), "window_days": int(value("--window-days"))}, out)
else:
    sys.exit(2)
"""


def git(cwd, *args, when=None):
    environment = {**os.environ}
    if when is not None:
        environment.update(GIT_AUTHOR_DATE=f"@{when} +0000", GIT_COMMITTER_DATE=f"@{when} +0000")
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *args],
                   cwd=cwd, env=environment, check=True, capture_output=True)


class PrepareTest(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp())
        self.addCleanup(subprocess.run, ["rm", "-rf", str(self.temp)])
        source = self.temp / "source"
        source.mkdir()
        git(source, "init", "-q")
        # 400, 200, 100, and 10 days before HEAD, then HEAD.
        for days, name in [(400, "a"), (200, "b"), (100, "c"), (10, "d"), (0, "e")]:
            (source / f"{name}.py").write_text(f"{name} = 1\n")
            git(source, "add", "-A")
            git(source, "commit", "-qm", name, when=HEAD_TIME - days * DAY)
        commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source,
                                capture_output=True, text=True, check=True).stdout.strip()
        self.repository = {"id": "demo", "url": f"file://{source}", "commit": commit}
        self.auditor = self.temp / "auditor"
        self.auditor.write_text(STUB_AUDITOR)
        self.auditor.chmod(0o755)
        self.cache = self.temp / "cache"
        patcher = mock.patch.dict(os.environ, {"EA_ECOSYSTEM_CACHE": str(self.cache)})
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_inputs_keep_the_window_and_the_checkout_is_deleted(self):
        marker = f"{sys.executable} -c \"import os,sys; open(sys.argv[2],'w').write(str(os.path.isdir(sys.argv[1])))\" {{repo}} {{out}}/reviewed.txt"
        settings = review_inputs.Settings(self.auditor, 150, marker)
        status = review_inputs.prepare([self.repository], settings, "t", 2)
        self.assertEqual(status, 0)
        out = self.cache / "review-inputs" / "t" / "demo"
        history = json.loads((out / "history.json").read_text())
        # 150 + 31 days back from HEAD reaches the commits 100 and 10 days
        # old; the fetch stops there.
        self.assertEqual(history, {"commits_fetched": 3, "window_days": 150})
        self.assertTrue((out / "audit.json").exists())
        # The review command ran while the checkout existed.
        self.assertEqual((out / "reviewed.txt").read_text(), "True")
        self.assertFalse((self.cache / "review-checkouts").exists())
        inputs = json.loads((out / "inputs.json").read_text())
        self.assertEqual(inputs["commit"], self.repository["commit"])
        self.assertIn("scan_seconds", inputs)
        summary = json.loads((self.cache / "review-inputs" / "t" / "summary.json").read_text())
        self.assertEqual([entry["id"] for entry in summary["repositories"]], ["demo"])

    def test_each_review_run_gets_its_number(self):
        command = f"{sys.executable} -c \"import sys; open(sys.argv[1],'w').write('{{{{}}}}')\" {{out}}/review-{{run}}.json"
        settings = review_inputs.Settings(self.auditor, 150, command, 2)
        self.assertEqual(review_inputs.prepare([self.repository], settings, "runs", 1), 0)
        out = self.cache / "review-inputs" / "runs" / "demo"
        self.assertEqual(sorted(path.name for path in out.glob("review-*.json")),
                         ["review-1.json", "review-2.json"])
        self.assertEqual([review["run"] for review in json.loads((out / "inputs.json").read_text())["reviews"]],
                         [1, 2])

    def test_preparing_again_resumes_a_stopped_run_and_keeps_the_rest(self):
        # Run 1 stops unless resumed; every invocation is logged.
        script = self.temp / "review.py"
        script.write_text(
            "import json, sys\n"
            "record, log = sys.argv[1], sys.argv[2]\n"
            "open(log, 'a').write(' '.join(sys.argv[1:]) + '\\n')\n"
            "stops = record.endswith('review-1.json') and '--resume' not in sys.argv\n"
            "json.dump({'stopped': 'outage'} if stops else {'stopped': None}, open(record, 'w'))\n")
        command = f"{sys.executable} {script} {{out}}/review-{{run}}.json {self.temp}/calls.log"
        settings = review_inputs.Settings(self.auditor, 150, command, 2)
        self.assertEqual(review_inputs.prepare([self.repository], settings, "again", 1), 1)
        out = self.cache / "review-inputs" / "again" / "demo"
        summary = json.loads((out.parent / "summary.json").read_text())
        self.assertIn("stopped", summary["failures"][0]["error"])
        scanned = (out / "audit.json").stat().st_mtime_ns

        self.assertEqual(review_inputs.prepare([self.repository], settings, "again", 1), 0)
        calls = (self.temp / "calls.log").read_text().splitlines()
        self.assertEqual(len(calls), 3)
        self.assertTrue(calls[2].endswith(f"--resume {out / 'review-1.json'}"))
        self.assertEqual((out / "audit.json").stat().st_mtime_ns, scanned)
        reviews = json.loads((out / "inputs.json").read_text())["reviews"]
        self.assertEqual([(review["run"], review["resumed"]) for review in reviews],
                         [(1, False), (2, False), (1, True)])

        self.assertEqual(review_inputs.prepare([self.repository], settings, "again", 1), 0)
        self.assertEqual(len((self.temp / "calls.log").read_text().splitlines()), 3)
        self.assertFalse((self.cache / "review-checkouts").exists())

    def test_a_failure_is_recorded_and_still_deletes_the_checkout(self):
        missing = {**self.repository, "commit": "f" * 40}
        status = review_inputs.prepare([missing], review_inputs.Settings(self.auditor, 150), "bad", 1)
        self.assertEqual(status, 1)
        summary = json.loads((self.cache / "review-inputs" / "bad" / "summary.json").read_text())
        self.assertEqual(summary["failures"][0]["id"], "demo")
        self.assertFalse((self.cache / "review-checkouts").exists())


# Fails the citation gate of a run named review-bad.json; with
# sheets, three of four counted items are useful.
STUB_QUALIFY = """#!/usr/bin/env python3
import json, sys
args = sys.argv[1:]
assert args[:2] == ["review", "qualify"], args
reviews = [args[i + 1] for i, arg in enumerate(args) if arg == "--review"]
scored = "--scores" in args
bad = lambda review: review.endswith("review-bad.json")
passed = not any(map(bad, reviews))
result = {
    "passed": passed,
    "runs": [{"label": review, "gates": [{"gate": "citations", "passed": not bad(review)}]}
             for review in reviews],
    "stability": {"passed": True},
    "usefulness": {"useful": 3, "useful_beyond_report": 1, "counted": 4, "pending": 0,
                   "disputed": 0} if scored else None,
}
open(args[args.index("--output") + 1], "w").write(json.dumps(result))
print(json.dumps(result))
sys.exit(0 if passed else 1)
"""


class CorpusTest(unittest.TestCase):
    def test_the_corpus_binds_current_files_and_known_repositories(self):
        corpus = review_inputs.load_corpus()
        self.assertEqual(corpus["repositories"]["slice"], "applications")
        self.assertEqual(len(set(corpus["repositories"]["ids"])), len(corpus["repositories"]["ids"]))
        self.assertGreaterEqual(corpus["run_plan"]["runs_per_repository"], 2)
        for case in corpus["protocol_suite"]:
            generator = review_inputs.REPO_ROOT / case["generator"]
            self.assertEqual(review_inputs.file_digest(generator), case["generator_digest"], case["id"])

    def test_a_changed_binding_is_refused(self):
        corpus = json.loads(review_inputs.CORPUS.read_text())
        corpus["bindings"]["brief"]["digest"] = "sha256:" + "0" * 64
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "corpus.json"
            path.write_text(json.dumps(corpus))
            with self.assertRaises(SystemExit) as raised:
                review_inputs.load_corpus(path)
        self.assertIn("changed since the corpus bound it", str(raised.exception))


class QualifyTest(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp())
        self.addCleanup(subprocess.run, ["rm", "-rf", str(self.temp)])
        self.auditor = self.temp / "auditor"
        self.auditor.write_text(STUB_QUALIFY)
        self.auditor.chmod(0o755)
        patcher = mock.patch.dict(os.environ, {"EA_ECOSYSTEM_CACHE": str(self.temp / "cache")})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.corpus = json.loads(review_inputs.CORPUS.read_text())
        self.corpus["repositories"]["ids"] = ["one", "two"]
        self.root = self.temp / "cache" / "review-inputs" / "t"

    def record(self, repository, *names):
        out = self.root / repository
        out.mkdir(parents=True, exist_ok=True)
        for name in names:
            (out / name).write_text("{}")

    def summary(self):
        return json.loads((self.root / "qualification-mistral_free.json").read_text())

    def run_qualify(self):
        return review_inputs.qualify(self.corpus, "mistral_free", self.auditor, "t")

    def test_a_failed_gate_or_a_missing_run_fails_the_route(self):
        self.record("one", "review-1.json", "review-bad.json")
        self.record("two", "review-1.json")
        self.assertEqual(self.run_qualify(), 1)
        summary = self.summary()
        self.assertFalse(summary["gates_passed_everywhere"])
        self.assertEqual(summary["decision"], "fails the automatic gates")
        one, two = summary["repositories"]
        self.assertEqual(one["failed_gates"], [f"{self.root / 'one' / 'review-bad.json'}: citations"])
        self.assertFalse(one["passed"])
        self.assertIn("1 recorded runs, 2 wanted", two["error"])

    def test_the_decision_waits_for_a_threshold_then_compares_with_it(self):
        for repository in ["one", "two"]:
            self.record(repository, "review-1.json", "review-2.json", "usefulness-1.json")
        self.assertEqual(self.run_qualify(), 1)
        summary = self.summary()
        self.assertTrue(summary["gates_passed_everywhere"])
        self.assertEqual(summary["usefulness"], {
            "useful": 6, "useful_beyond_report": 2, "counted": 8, "share": 0.75,
            "beyond_report_share": 0.25, "threshold": None, "beyond_report_threshold": None})
        self.assertEqual(summary["decision"], "gates pass; the usefulness bar is not set yet")
        self.assertTrue((self.root / "one" / "qualification-mistral_free.json").exists())
        bar = self.corpus["usefulness"]
        bar["threshold"] = 0.7
        self.assertEqual(self.run_qualify(), 1)
        self.assertEqual(self.summary()["decision"], "gates pass; the usefulness bar is not set yet")
        bar["beyond_report_threshold"] = 0.25
        self.assertEqual(self.run_qualify(), 0)
        self.assertEqual(self.summary()["decision"], "qualifies")
        for useful, beyond in [(0.8, 0.25), (0.7, 0.3)]:
            bar["threshold"], bar["beyond_report_threshold"] = useful, beyond
            self.assertEqual(self.run_qualify(), 1)
            self.assertEqual(self.summary()["decision"], "below the usefulness bar")


if __name__ == "__main__":
    unittest.main()

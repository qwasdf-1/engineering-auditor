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

    def test_a_failure_is_recorded_and_still_deletes_the_checkout(self):
        missing = {**self.repository, "commit": "f" * 40}
        status = review_inputs.prepare([missing], review_inputs.Settings(self.auditor, 150), "bad", 1)
        self.assertEqual(status, 1)
        summary = json.loads((self.cache / "review-inputs" / "bad" / "summary.json").read_text())
        self.assertEqual(summary["failures"][0]["id"], "demo")
        self.assertFalse((self.cache / "review-checkouts").exists())


if __name__ == "__main__":
    unittest.main()

"""Behavior tests for the ecosystem syntax-coverage check."""

import unittest

from tooling.ecosystem import syntax_coverage


def report(files):
    return {"files": [{"path": path, "catalog_language": language, **({"syntax": syntax} if syntax else {})}
                      for path, language, syntax in files]}


class SyntaxCoverageTest(unittest.TestCase):
    def test_counts_recovered_and_unparsed_files_per_language(self):
        summary = syntax_coverage.syntax_summary([
            report([("a.c", "c", "recovered"), ("b.c", "c", "unparsed"), ("c.c", "c", None)]),
            report([("d.rs", "rust", None), ("e.cs", "csharp", "unparsed")]),
        ])
        self.assertEqual(summary, {
            "c": {"files": 3, "recovered": 1, "unparsed": 1},
            "csharp": {"files": 1, "recovered": 0, "unparsed": 1},
        })

    def test_more_unparsed_files_than_the_baseline_is_a_regression(self):
        baseline = {"c": {"files": 3, "recovered": 1, "unparsed": 2}}
        same = {"c": {"files": 3, "recovered": 1, "unparsed": 2}}
        worse = {"c": {"files": 3, "recovered": 0, "unparsed": 3},
                 "java": {"files": 9, "recovered": 0, "unparsed": 1}}
        better = {"c": {"files": 3, "recovered": 2, "unparsed": 1}}
        self.assertEqual(syntax_coverage.regressions(same, baseline), [])
        self.assertEqual(syntax_coverage.regressions(worse, baseline), [
            "c: 3 unparsed files (baseline 2)",
            "java: 1 unparsed files (baseline 0)",
        ])
        self.assertEqual(syntax_coverage.regressions(better, baseline), [])
        self.assertEqual(syntax_coverage.improvements(better, baseline), ["c: 1 unparsed files (baseline 2)"])


if __name__ == "__main__":
    unittest.main()

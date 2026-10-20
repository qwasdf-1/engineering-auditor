import unittest

from tooling.recall.cases import SHAPES, Case, all_cases
from tooling.recall.run import improvements, outcome, regressions


class RecallRunTest(unittest.TestCase):
    def test_outcomes_depend_on_shape(self) -> None:
        positive = Case("d", "rust", "helper", {})
        negative = Case("d", "rust", "negative", {})
        self.assertEqual(outcome(positive, {"d"}), "detected")
        self.assertEqual(outcome(positive, {"other"}), "missed")
        self.assertEqual(outcome(negative, {"d"}), "fired")
        self.assertEqual(outcome(negative, set()), "silent")

    def test_baseline_only_ratchets_up(self) -> None:
        baseline = {"a": "detected", "b": "missed", "c": "silent", "gone": "detected"}
        results = {"a": "missed", "b": "detected", "c": "fired", "new": "detected"}
        self.assertEqual(
            regressions(baseline, results),
            ["a: detected -> missed", "c: silent -> fired", "gone: no longer measured"],
        )
        self.assertEqual(improvements(baseline, results), ["b: missed -> detected", "new: new -> detected"])

    def test_every_case_group_has_all_shapes(self) -> None:
        cases = all_cases()
        groups = {(case.detector, case.language) for case in cases}
        self.assertEqual(len(cases), len(groups) * len(SHAPES))
        self.assertEqual(len({case.id for case in cases}), len(cases))
        for case in cases:
            self.assertTrue(case.files, case.id)


if __name__ == "__main__":
    unittest.main()

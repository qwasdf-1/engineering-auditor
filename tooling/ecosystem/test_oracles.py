"""Behavior tests for corpus oracle judging and model-review parsing (no tools, no network)."""

import json
import tempfile
import unittest
from pathlib import Path

from tooling.ecosystem import model_review, oracle_judges, oracle_overlap, oracle_tools, oracles


def entry(line=10, path="a.py", language="python"):
    return {"id": "EA-0000000000000001", "repository": "r", "language": language, "path": path, "line": line}


def finding(**evidence):
    return {"evidence": evidence}


def function(name, start, end, **metrics):
    return {"name": name, "start": start, "end": end, "metrics": metrics}


def diagnostic(rule, line, message="", value=None, name=""):
    return {"rule": rule, "line": line, "message": message, "value": value, "name": name}


class MatchTest(unittest.TestCase):
    def test_matches_by_short_name_nearest_line_and_anonymous_by_start(self):
        facts = [function("Server::handle", 12, 40), function("handle", 90, 99), function("<anon>", 50, 60)]
        self.assertEqual(oracle_judges.match_function(facts, "(*Server)::handle", 10)["start"], 12)
        self.assertIsNone(oracle_judges.match_function(facts, "handle", 70))
        self.assertEqual(oracle_judges.match_function(facts, "", 50)["start"], 50)
        self.assertIsNone(oracle_judges.match_function(facts, "", 52))
        # A tool without lines (complexipy) matches only a unique name.
        unplaced = [function("A::run", None, None), function("B::run", None, None), function("go", None, None)]
        self.assertIsNone(oracle_judges.match_function(unplaced, "run", 5))
        self.assertEqual(oracle_judges.match_function(unplaced, "go", 5)["name"], "go")
        # An arrow-function constant EA names by its binding; the tool calls it anonymous.
        arrows = [function("<anonymous>", 30, 60), function("<anonymous>", 31, 31)]
        self.assertEqual(oracle_judges.match_function(arrows, "tupleProcessor", 30)["end"], 60)
        self.assertIsNone(oracle_judges.match_function(arrows, "tupleProcessor", 29))


class MetricJudgeTest(unittest.TestCase):
    def test_metric_confirms_over_budget_refutes_clearly_under_and_abstains_between(self):
        judge = oracle_judges.metric_judge("rca", "cognitive", oracle_judges.COGNITIVE)
        for value, claim in ((16, "confirmed"), (12, "refuted"), (14, None)):
            verdict = judge(entry(), finding(function="f", cognitive_complexity=18),
                            [function("f", 10, 30, cognitive=value)])
            self.assertEqual(verdict and verdict[0], claim, value)
        self.assertIn("(EA 18)", judge(entry(), finding(function="f", cognitive_complexity=18),
                                       [function("f", 10, 30, cognitive=16)])[1])

    def test_nested_decisions_sum_closures_inside_the_span(self):
        facts = [function("outer", 10, 50, ccn=4), function("(anonymous)", 20, 30, ccn=6),
                 function("(anonymous)", 35, 45, ccn=4), function("other", 60, 70, ccn=20)]
        judge = oracle_judges.metric_judge("lizard", "decisions", oracle_judges.BRANCHES, oracle_judges.nested_decisions)
        claim, detail = judge(entry(), finding(function="outer", branch_count=11), facts)
        self.assertEqual(claim, "confirmed")
        self.assertIn("decisions 11", detail)

    def test_halstead_uses_the_detectors_rule_with_a_refutation_margin(self):
        def judged(volume, difficulty):
            facts = [function("f", 10, 99, halstead_volume=volume, halstead_difficulty=difficulty)]
            verdict = oracle_judges.halstead_judge(entry(), finding(function="f", halstead_volume=2000,
                                                              halstead_difficulty=20.0), facts)
            return verdict and verdict[0]
        self.assertEqual(judged(1600, 16), "confirmed")
        self.assertEqual(judged(400, 31), "confirmed")
        self.assertEqual(judged(800, 10), "refuted")
        self.assertIsNone(judged(1400, 13))

    def test_valued_diagnostic_abstains_on_silence_and_matches_pmd_by_name(self):
        judge = oracle_judges.valued_diagnostic_judge("pmd", "CognitiveComplexity", oracle_judges.COGNITIVE, by_name=True)
        facts = [diagnostic("CognitiveComplexity", 95, value=1, name="isLiteral"),
                 diagnostic("CognitiveComplexity", 125, value=24, name="next")]
        self.assertEqual(judge(entry(94), finding(function="isLiteral"), facts)[0], "refuted")
        self.assertEqual(judge(entry(120), finding(function="next"), facts)[0], "confirmed")
        self.assertIsNone(judge(entry(300), finding(function="missing"), facts))


class DiagnosticJudgeTest(unittest.TestCase):
    def test_presence_confirms_and_silence_refutes_when_the_rule_covers_the_claim(self):
        judge = oracle_judges.diagnostic_judge("ruff", ("PLC0415",), lines_from="lines")
        facts = [diagnostic("PLC0415", 19), diagnostic("S101", 30)]
        self.assertEqual(judge(entry(18), finding(lines=["19"]), facts)[0], "confirmed")
        self.assertEqual(judge(entry(29), finding(lines=["30"]), facts)[0], "refuted")
        confirm_only = oracle_judges.diagnostic_judge("pmd", ("FieldNamingConventions",), refutes=False)
        self.assertIsNone(confirm_only(entry(5), finding(), facts))

    def test_clone_needs_a_jscpd_pair_over_two_occurrences_and_never_refutes(self):
        occurrences = finding(occurrences=["a.rs:23:1-34:2", "b.rs:99:1-110:2"])
        pairs = [(("a.rs", 21, 36), ("b.rs", 97, 112))]
        self.assertEqual(oracle_judges.clone_judge(entry(23), occurrences, pairs)[0], "confirmed")
        self.assertIsNone(oracle_judges.clone_judge(entry(23), occurrences, [(("a.rs", 21, 36), ("a.rs", 200, 215))]))
        self.assertIsNone(oracle_judges.clone_judge(entry(23), occurrences, []))

    def test_goconst_silence_is_no_refutation(self):
        facts = [diagnostic("goconst", 142, "article")]
        self.assertEqual(oracle_judges.goconst_judge(entry(142), finding(literal="article"), facts)[0], "confirmed")
        self.assertIsNone(oracle_judges.goconst_judge(entry(142), finding(literal="true"), facts))

    def test_large_file_recount(self):
        facts = [{"lines": 1305}]
        self.assertEqual(oracle_judges.large_file_judge(entry(None), finding(advisory_threshold=1000), facts)[0],
                         "confirmed")
        self.assertEqual(oracle_judges.large_file_judge(entry(None), finding(advisory_threshold=2000), facts)[0],
                         "refuted")


class RuleTableTest(unittest.TestCase):
    def test_every_rule_names_a_collector_with_a_pinned_version(self):
        versions = oracle_tools.versions()
        for rule in oracles.rules():
            self.assertIn(rule.tool, oracle_tools.COLLECTORS, rule.detector)
            self.assertIn(rule.tool, versions, rule.detector)
            self.assertTrue(rule.languages, rule.detector)

    def test_one_oracle_per_detector_and_language(self):
        seen = set()
        for rule in oracles.rules():
            for language in rule.languages:
                self.assertNotIn((rule.detector, language), seen)
                seen.add((rule.detector, language))


class OverlapTest(unittest.TestCase):
    def test_overlap_counts_every_live_finding_and_records_judged_claims(self):
        reports = {"r": {"files": [{"path": "a.py", "catalog_language": "python"}], "findings": [
            {"id": f"EA-{index}", "detector": "assert_in_production", "message": "m",
             "locations": [{"path": "a.py", "line": index}]} for index in (1, 2, 3)]}}
        entries = oracle_overlap.live_entries(reports)
        rule = next(rule for rule in oracles.rules() if rule.detector == "assert_in_production")
        planned = [(rule, item, {}) for item in entries["assert_in_production"]]
        checks = {"oracle:ruff@1": [{"id": "EA-1", "claim": "confirmed", "detail": "d"},
                                    {"id": "EA-2", "claim": "refuted", "detail": "d"}]}
        labels = {"EA-1": {"verdict": "true_positive", "status": "final"},
                  "EA-2": {"verdict": "false_positive", "status": "corroborated"},
                  "EA-3": {"verdict": "true_positive", "status": "pre_label"}}
        rows, claims = oracle_overlap.overlap(entries, planned, checks, labels)
        self.assertEqual(rows[("assert_in_production", "python")],
                         {"findings": 3, "covered": 3, "confirmed": 1, "refuted": 1, "unjudged": 1,
                          "labelled": 2, "confirmed_tp": 1, "refuted_fp": 1})
        self.assertEqual((claims["EA-2"]["claim"], claims["EA-2"]["line"]), ("refuted", 2))
        self.assertNotIn("EA-3", claims)

    def test_stratified_precision_weights_strata_and_accepts_a_clean_agreement_audit(self):
        from collections import Counter
        counts = Counter(findings=100, confirmed=80, refuted=10, confirmed_tp=30,
                         refuted_tp=2, refuted_fp=8, other_tp=9, other_fp=1)
        estimate, sample = oracle_overlap.stratified(counts)
        self.assertAlmostEqual(estimate, 0.8 * 1.0 + 0.1 * 0.2 + 0.1 * 0.9)
        self.assertEqual(sample, 80 + 10 + 10)
        counts["confirmed_fp"] = 1
        self.assertEqual(oracle_overlap.stratified(counts)[1], 31 + 10 + 10)
        counts.update(confirmed_tp=-11, confirmed_fp=-1)
        self.assertEqual(oracle_overlap.stratified(counts)[1], 19 + 10 + 10)
        self.assertEqual(oracle_overlap.stratified(Counter(findings=5, confirmed=5)), (None, None))
        self.assertAlmostEqual(oracle_overlap.wilson_lower(10, 10), 0.722, places=3)


class ModelReviewTest(unittest.TestCase):
    def test_parse_answer_keeps_valid_verdicts_for_known_ids(self):
        text = ('Here you go:\n[{"id": "EA-1", "verdict": "true_positive", "reason": "counts match"},'
                '{"id": "EA-2", "verdict": "maybe", "reason": "x"}, {"id": "EA-9", "verdict": "unclear",'
                ' "reason": "y"}, {"id": "EA-3", "verdict": "false_positive", "reason": " "}]')
        self.assertEqual(model_review.parse_answer(text, {"EA-1", "EA-2", "EA-3"}),
                         [{"id": "EA-1", "verdict": "true_positive", "reason": "counts match"}])
        self.assertEqual(model_review.parse_answer("no json", {"EA-1"}), [])
        keyed = '{"EA-1": {"verdict": "false_positive", "reason": "lazy import"}}'
        self.assertEqual(model_review.parse_answer(keyed, {"EA-1"}),
                         [{"id": "EA-1", "verdict": "false_positive", "reason": "lazy import"}])
        wrapped = '{"findings": [{"id": "EA-1", "verdict": "unclear", "reason": "r"}]}'
        self.assertEqual(len(model_review.parse_answer(wrapped, {"EA-1"})), 1)

    def test_review_is_blind_batched_and_cached(self):
        items = [{"id": f"EA-{n}", "detector": "d", "excerpt": "x", "message": "m"} for n in range(3)]
        prompts = []

        def ask(model, text):
            prompts.append(text)
            ids = [item["id"] for item in items if f'"{item["id"]}"' in text]
            return json.dumps([{"id": i, "verdict": "unclear", "reason": "r"} for i in ids])

        with tempfile.TemporaryDirectory() as cache:
            first = model_review.review(items, model_review.Route("m/x", ask), Path(cache), batch=2)
            again = model_review.review(items, model_review.Route("m/x", ask), Path(cache), batch=2)
        self.assertEqual((len(first), len(prompts)), (3, 2))
        self.assertEqual(first, again)
        self.assertNotIn("verdict\": \"true_positive", prompts[0].split("Findings:")[1])

    def test_earlier_verdicts_of_the_model_are_carried_over(self):
        labels = {"d": [{"id": "EA-1", "checks": [{"source": "model:m/x", "verdict": "true_positive", "reason": "r"}]},
                        {"id": "EA-2", "checks": [{"source": "model:m/y", "verdict": "true_positive", "reason": "r"}]},
                        {"id": "EA-3"}]}
        self.assertEqual(model_review.earlier_verdicts(labels, "model:m/x"),
                         {"EA-1": {"id": "EA-1", "verdict": "true_positive", "reason": "r"}})


if __name__ == "__main__":
    unittest.main()

import unittest

from tooling.live_provider.compare_prompt_candidate import (
    load_manifest,
    resumable_runs,
    route_binding,
    seeded_runs,
    split_cases,
    summarize,
)
from tooling.live_provider.lifecycle_eval_gate import MECHANICAL_CHECKS, SEMANTIC_CHECKS
from tooling.live_provider.lifecycle_eval_support import EvalError, sha256_file
from tooling.live_provider.run_lifecycle_v2_eval import REPO_ROOT

MANIFEST = REPO_ROOT / "eval/review-improvement/intervening-mutation-v1/manifest.json"


def scored_run(case_id: str, answer: str, **metrics: object) -> dict:
    return {
        "case_id": case_id,
        "run_index": 0,
        "slot_answer": answer,
        "metrics": {
            **dict.fromkeys([*MECHANICAL_CHECKS, *SEMANTIC_CHECKS], True),
            "qualified": True,
            "execution_succeeded": True,
            "finding_immutability_valid": True,
            "forbidden_outputs": 0,
            "unsafe_recommendations": 0,
            "raw_trace_or_secret_leaks": 0,
            **metrics,
        },
    }


class CompareCandidateTest(unittest.TestCase):
    def test_held_out_cases_require_a_frozen_candidate(self) -> None:
        manifest = load_manifest(MANIFEST)
        manifest["candidate"]["frozen"] = False
        with self.assertRaisesRegex(EvalError, "frozen"):
            split_cases(manifest, "held_out")
        manifest["candidate"]["frozen"] = True
        self.assertEqual(len(split_cases(manifest, "held_out")), 4)

    def test_training_cases_resolve_from_the_bound_release_corpus(self) -> None:
        manifest = load_manifest(MANIFEST)
        ids = [case["id"] for case in split_cases(manifest, "training")]
        self.assertEqual(ids, manifest["training_case_ids"])
        self.assertTrue(set(ids).isdisjoint(case["id"] for case in manifest["held_out_cases"]))

    def test_manifest_bindings_and_held_out_sources_are_current(self) -> None:
        manifest = load_manifest(MANIFEST)
        for binding in (
            manifest["baseline"]["case_templates"],
            manifest["candidate"]["case_templates"],
            manifest["routes"],
            *manifest["check_routes"].values(),
            manifest["release_corpus"],
        ):
            self.assertEqual(sha256_file(REPO_ROOT / binding["path"]), binding["digest"])
        for case in manifest["held_out_cases"]:
            self.assertEqual(sha256_file(REPO_ROOT / case["generator"]), case["generator_digest"])

    def test_summary_excludes_errored_failed_and_unqualified_runs(self) -> None:
        case = {"id": "c", "expectations": {"probe_answers": {"intervening_mutation": ["no"]}}}
        runs = [
            scored_run("c", "no"),
            scored_run("c", "unknown", forbidden_outputs=1),
            {"case_id": "c", "run_index": 2, "error": "provider timeout"},
            scored_run("c", "unknown", execution_succeeded=False),
            scored_run("c", "unknown", qualified=False),
        ]
        summary = summarize([case], runs, "intervening_mutation")["c"]
        self.assertEqual(summary["scored_runs"], 2)
        self.assertEqual(summary["unscored_runs"], 3)
        self.assertEqual(summary["slot_hits"], 1)
        self.assertEqual(summary["slot_answers"], {"no": 1, "unknown": 1})
        self.assertEqual(summary["forbidden_outputs"], 1)
        self.assertEqual(summary["exact_passes"], 1)

    def test_gate_and_check_routes_resolve_by_name(self) -> None:
        manifest = load_manifest(MANIFEST)
        self.assertEqual(route_binding(manifest, "gate"), manifest["routes"])
        self.assertEqual(route_binding(manifest, "minimax"), manifest["check_routes"]["minimax"])
        with self.assertRaisesRegex(EvalError, "unknown route set"):
            route_binding(manifest, "absent")

    def test_resume_reuses_scored_runs_only_from_a_matching_report(self) -> None:
        identity = {"arm": "baseline", "routes_digest": "sha256:a"}
        previous = {
            **identity,
            "runs": [scored_run("c", "no"), scored_run("c", "unknown", execution_succeeded=False)],
        }
        self.assertEqual(len(resumable_runs(previous, identity)), 1)
        self.assertEqual(resumable_runs({}, identity), [])
        with self.assertRaisesRegex(EvalError, "routes_digest"):
            resumable_runs(previous, {**identity, "routes_digest": "sha256:b"})

    def test_seeding_requires_the_same_arm_identity_and_rereads_the_slot(self) -> None:
        identity = {
            "arm": "baseline",
            "split": "training",
            "case_templates_digest": "sha256:t",
            "routes_digest": "sha256:r",
        }
        source = {**identity, "improvement_id": "other", "runs": [scored_run("c", "no")]}
        with self.assertRaisesRegex(EvalError, "routes_digest"):
            seeded_runs({**source, "routes_digest": "sha256:x"}, identity, {"c"}, "slot")
        self.assertEqual(seeded_runs(source, identity, {"absent"}, "slot"), [])


if __name__ == "__main__":
    unittest.main()

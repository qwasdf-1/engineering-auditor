import argparse
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tooling.live_provider.run_lifecycle_v2_eval import (
    CheckedCommand,
    EVALUATOR_CONTRACT_VERSION,
    EvalError,
    REPO_ROOT,
    aggregate_metric_vector,
    bound_pending_runs,
    citation_valid,
    conclusion_signature,
    evaluator_identity,
    evaluation_identity,
    evaluator_source_digest,
    evaluate_run,
    exact_metric_failures,
    execution_succeeded,
    lifecycle_action_complete,
    load_json,
    numeric_benefit_challenged,
    unsupported_numeric_benefit_removed,
    pending_run_indexes,
    prepare_run_directory,
    provider_record_leak_count,
    qualified,
    release_run_counts,
    consent_flags,
    resolve_binding,
    resolve_routes,
    run_checked,
    select_cases,
    sha256_file,
    source_tree_digest,
    validate_resumed_runs,
    validate_live_identity,
    write_json_atomic,
)


class LifecycleV2EvalTests(unittest.TestCase):
    def setUp(self) -> None:
        self.corpus_path = REPO_ROOT / "config/auditor/review-eval-corpus-v2.json"
        self.corpus = json.loads(self.corpus_path.read_text())

    def test_public_corpus_has_exact_bound_inputs_and_reproducible_sources(self) -> None:
        self.assertEqual(self.corpus["schema_version"], 2)
        release_counts = release_run_counts(self.corpus, self.corpus["cases"])
        self.assertEqual(sum(release_counts.values()), 16)
        self.assertEqual(sorted(release_counts.values()), [1, 1, 1, 1, 3, 3, 3, 3])
        self.assertEqual(len(self.corpus["cases"]), 8)
        for name in ("case_templates", "prompt_bundle", "routes"):
            path = resolve_binding(self.corpus, name)
            self.assertEqual(sha256_file(path), self.corpus["bindings"][name]["digest"])
        for case in self.corpus["cases"]:
            self.assertTrue(case["expectations"]["allowed_case_conclusions"])
            self.assertNotIn("required_claim_slots", case["expectations"])
            conclusion = case["expectations"]["allowed_case_conclusions"]
            required_families = case["expectations"][
                "required_recommendation_families"
            ]
            expected_families = {
                "lifecycle_mismatch_substantiated": ["lifecycle_reownership"],
                "repetition_justified": ["explicit_retention_avoidance"],
                "impact_immaterial": [],
                "insufficient_evidence": ["measurement_before_change"],
            }
            self.assertEqual(len(conclusion), 1)
            self.assertEqual(required_families, expected_families[conclusion[0]])
        generator = REPO_ROOT / self.corpus["cases"][0]["generator"]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for case in self.corpus["cases"]:
                self.assertEqual(sha256_file(generator), case["generator_digest"])
                output = root / case["fixture_kind"]
                completed = subprocess.run(
                    [str(generator), str(output), case["fixture_kind"]],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(source_tree_digest(output), case["source_digest"])
                if case["fixture_kind"] == "missing_evidence":
                    source = output.joinpath("src/lib.rs").read_text()
                    self.assertIn("refresh: impl FnMut(&mut Vec<i32>)", source)
                    self.assertIn("consume: impl FnMut(&[i32])", source)
                    self.assertNotIn("std::env::var", source)

    def test_empty_case_selection_runs_the_complete_corpus(self) -> None:
        self.assertEqual(select_cases(self.corpus, []), self.corpus["cases"])
        self.assertEqual(
            [case["id"] for case in select_cases(self.corpus, [self.corpus["cases"][0]["id"]])],
            [self.corpus["cases"][0]["id"]],
        )
        with self.assertRaises(EvalError):
            select_cases(self.corpus, ["not-a-corpus-case"])

    def test_resume_schedules_only_missing_contiguous_run_indexes(self) -> None:
        cases = self.corpus["cases"][:2]
        runs = [{"case_id": cases[0]["id"], "run_index": 0}]
        required_counts = {cases[0]["id"]: 3, cases[1]["id"]: 1}
        pending = pending_run_indexes(cases, runs, required_counts)
        self.assertEqual(
            [(case["id"], index) for case, index in pending],
            [
                (cases[0]["id"], 1),
                (cases[0]["id"], 2),
                (cases[1]["id"], 0),
            ],
        )
        with self.assertRaisesRegex(EvalError, "already exceeds"):
            pending_run_indexes(
                cases,
                [
                    {"case_id": cases[0]["id"], "run_index": index}
                    for index in range(4)
                ],
                required_counts,
            )

    def test_pending_run_bound_preserves_order_for_resumable_checkpoints(self) -> None:
        cases = self.corpus["cases"][:2]
        pending = [(cases[0], 1), (cases[0], 2), (cases[1], 0)]
        self.assertEqual(bound_pending_runs(pending, None), pending)
        self.assertEqual(bound_pending_runs(pending, 1), pending[:1])
        self.assertEqual(bound_pending_runs(pending, 2), pending[:2])

    def test_checkpoint_write_is_atomic_and_incomplete_run_is_quarantined(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            checkpoint = root / "checkpoint.json"
            write_json_atomic(checkpoint, {"status": "in_progress", "runs": [1]})
            self.assertEqual(load_json(checkpoint)["runs"], [1])
            self.assertEqual(list(root.glob(f".{checkpoint.name}.*.tmp")), [])

            run_dir = root / "case" / "run-1"
            run_dir.mkdir(parents=True)
            (run_dir / "partial.txt").write_text("retained")
            prepare_run_directory(run_dir)

            self.assertTrue(run_dir.is_dir())
            self.assertEqual(list(run_dir.iterdir()), [])
            self.assertEqual(
                (root / "case" / "run-1.incomplete-1" / "partial.txt").read_text(),
                "retained",
            )

    def test_resume_rejects_identity_drift_before_reusing_artifacts(self) -> None:
        expected = {
            "corpus_id": self.corpus["corpus_id"],
            "corpus_digest": "sha256:expected",
            "evaluator": evaluator_identity(),
            "bindings": {},
            "binaries": {},
            "live_anonymous": True,
            "review_process_timeout_seconds": None,
        }
        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / "prior.json"
            report.write_text(
                json.dumps(
                    {
                        "schema_version": 2,
                        **expected,
                        "corpus_digest": "sha256:changed",
                        "runs": [],
                    }
                )
            )
            with self.assertRaisesRegex(EvalError, "corpus_digest"):
                validate_resumed_runs(
                    report,
                    expected,
                    self.corpus["cases"],
                )

    def test_resume_evaluator_change_override_is_explicit_and_narrow(self) -> None:
        expected = {
            "corpus_id": self.corpus["corpus_id"],
            "corpus_digest": "sha256:stable",
            "evaluator": evaluator_identity(),
            "bindings": {},
            "binaries": {},
            "live_anonymous": True,
            "review_process_timeout_seconds": None,
        }
        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / "prior.json"
            report.write_text(
                json.dumps(
                    {
                        "schema_version": 2,
                        **expected,
                        "evaluator": {"digest": "sha256:prior"},
                        "runs": [],
                    }
                )
            )
            with self.assertRaisesRegex(EvalError, "evaluator"):
                validate_resumed_runs(report, expected, self.corpus["cases"])
            self.assertEqual(
                validate_resumed_runs(
                    report,
                    expected,
                    self.corpus["cases"],
                    allow_evaluator_change=True,
                ),
                [],
            )

    def test_live_identity_rejects_binary_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corpus_path = root / "corpus.json"
            corpus_path.write_text(json.dumps(self.corpus))
            auditor = root / "auditor"
            runner = root / "review-v2-case"
            auditor.write_bytes(b"auditor-v1")
            runner.write_bytes(b"runner-v1")
            args = argparse.Namespace(
                corpus=corpus_path,
                auditor=auditor,
                runner=runner,
                no_review_timeout=True,
                timeout_seconds=900,
                route_set="anonymous",
            )
            bindings = {
                name: resolve_binding(self.corpus, name)
                for name in ("case_templates", "prompt_bundle", "routes")
            }
            expected = evaluation_identity(args, self.corpus, bindings)
            validate_live_identity(expected, args, self.corpus, bindings)
            auditor.write_bytes(b"auditor-v2")
            with self.assertRaisesRegex(EvalError, "binaries"):
                validate_live_identity(expected, args, self.corpus, bindings)

    def test_qualification_citation_and_numeric_challenge_checks_are_typed(self) -> None:
        artifact = {
            "qualification_records": [
                {
                    "schema_version": 2,
                    "binding": {"role": role},
                    "transport_valid": True,
                    "native_protocol_valid": True,
                    "semantic_output_valid": True,
                    "semantic_cases_passed": [
                        "positive",
                        "negative",
                        "unknown",
                        "conflicting_evidence",
                        "forbidden_redesign",
                    ],
                    "failure_class": None,
                }
                for role in ("single_reviewer", "challenger")
            ],
            "probe_results": [{"probe_id": "probe:one"}],
            "claims": [
                {
                    "claim_id": "claim:one",
                    "supporting_probe_ids": ["probe:one"],
                    "counter_probe_ids": [],
                }
            ],
            "recommendations": [
                {
                    "recommendation_id": "recommendation:one",
                    "premise_claim_ids": ["claim:one"],
                    "verification_steps": [{"evidence_probe_ids": ["probe:one"]}],
                }
            ],
            "reviewer_runs": [
                {
                    "recommendations": [
                        {
                            "recommendation_id": "recommendation:one",
                            "expected_benefits": [
                                {
                                    "numeric_value": "70",
                                    "evidence_mode": "qualitative_only",
                                }
                            ],
                        }
                    ]
                }
            ],
            "challenges": [
                {
                    "challenge": {
                        "objections": [
                            {
                                "target_id": "recommendation:one",
                                "reason": "overclaimed_benefit",
                            }
                        ]
                    }
                }
            ],
        }
        self.assertTrue(qualified(artifact))
        self.assertFalse(execution_succeeded(artifact))
        self.assertTrue(citation_valid(artifact))
        self.assertTrue(numeric_benefit_challenged(artifact))
        artifact["execution_truth"] = [{"accepted_recommendations": []}]
        artifact["execution_truth"][0]["accepted_recommendations"] = [
            {
                "expected_benefits": [
                    {
                        "description": "Reduce repeated sorting work.",
                        "numeric_value": None,
                        "evidence_mode": "structural_inference",
                    }
                ]
            }
        ]
        self.assertTrue(unsupported_numeric_benefit_removed(artifact))
        artifact["execution_truth"][0]["accepted_recommendations"][0][
            "expected_benefits"
        ][0]["description"] = "Expect a 70% latency reduction."
        self.assertFalse(unsupported_numeric_benefit_removed(artifact))
        artifact["claims"].append(
            {
                "claim_id": "claim:aggregate",
                "supporting_probe_ids": [],
                "counter_probe_ids": [],
                "dependency_claim_ids": ["claim:one"],
            }
        )
        self.assertTrue(citation_valid(artifact))
        artifact["claims"][-1]["dependency_claim_ids"] = ["claim:missing"]
        self.assertFalse(citation_valid(artifact))
        artifact["claims"].pop()
        artifact["qualification_records"][1]["native_protocol_valid"] = False
        self.assertFalse(qualified(artifact))
        artifact["qualification_records"][1]["native_protocol_valid"] = True
        artifact["qualification_records"].append(
            {
                "schema_version": 2,
                "binding": {"role": "reviewer_a"},
                "transport_valid": False,
                "native_protocol_valid": False,
                "semantic_output_valid": False,
                "semantic_cases_passed": [],
                "failure_class": "invalid_output",
            }
        )
        self.assertTrue(qualified(artifact))
        artifact["claims"][0]["supporting_probe_ids"] = ["probe:missing"]
        self.assertFalse(citation_valid(artifact))
        artifact["execution_truth"] = [{"closure": {"reason": "resolved"}}]
        self.assertTrue(execution_succeeded(artifact))
        artifact["execution_truth"][0]["closure"]["reason"] = "execution_failed"
        self.assertFalse(execution_succeeded(artifact))

    def test_narrowed_lifecycle_action_follows_challenged_recommendation_lineage(
        self,
    ) -> None:
        accepted_variant = {
            "recommendation_id": "recommendation:variant",
            "premise_claim_ids": ["claim:one"],
            "risks": ["bounded risk"],
            "alternatives": [{"summary": "alternative"}],
            "verification_steps": [{"method": "test"}],
        }
        artifact = {
            "challenges": [
                {
                    "challenge": {
                        "objections": [{"target_id": "recommendation:original"}]
                    }
                }
            ],
            "challenge_target_dispositions": [
                {
                    "target_id": "recommendation:original",
                    "target_type": "recommendation",
                    "disposition": {"disposition": "cleared"},
                }
            ],
            "execution_truth": [
                {
                    "recommendation_dispositions": [
                        {
                            "recommendation_id": "recommendation:original",
                            "disposition": "narrowed",
                            "accepted_variant_id": "recommendation:variant",
                        }
                    ]
                }
            ],
        }
        self.assertTrue(lifecycle_action_complete(accepted_variant, artifact))
        artifact["execution_truth"][0]["recommendation_dispositions"][0][
            "accepted_variant_id"
        ] = "recommendation:other"
        self.assertFalse(lifecycle_action_complete(accepted_variant, artifact))

    def test_cleared_lifecycle_action_has_a_complete_challenge_disposition(self) -> None:
        recommendation = {
            "recommendation_id": "recommendation:accepted",
            "premise_claim_ids": ["claim:one"],
            "risks": ["bounded risk"],
            "alternatives": [{"summary": "alternative"}],
            "verification_steps": [{"method": "test"}],
        }
        artifact = {
            "challenge_target_dispositions": [
                {
                    "target_id": "recommendation:accepted",
                    "target_type": "recommendation",
                    "disposition": {"disposition": "cleared"},
                }
            ],
            "execution_truth": [],
        }
        self.assertTrue(lifecycle_action_complete(recommendation, artifact))
        artifact["challenge_target_dispositions"][0]["disposition"] = {
            "disposition": "unknown"
        }
        self.assertFalse(lifecycle_action_complete(recommendation, artifact))

    def test_provider_record_leak_check_reads_nested_call_digests(self) -> None:
        artifact = {
            "provider_records": [
                {
                    "call": {
                        "context_digest": "a" * 64,
                        "output_digest": "sha256:" + ("b" * 64),
                    }
                }
            ]
        }
        self.assertEqual(provider_record_leak_count(artifact), 0)
        artifact["provider_records"][0]["call"]["context_digest"] = "raw prompt"
        self.assertEqual(provider_record_leak_count(artifact), 1)

    def test_execution_failures_do_not_contaminate_semantic_metrics(self) -> None:
        failed_metrics = {
            "qualified": True,
            "execution_succeeded": False,
            "schema_valid": True,
            "binding_valid": True,
            "citation_valid": True,
            "finding_immutability_valid": True,
            "candidate_count_valid": True,
            "closure_valid": False,
            "case_conclusion_valid": False,
            "probe_answers_valid": False,
            "required_recommendations_valid": False,
            "abstention_correct": False,
            "unsupported_numeric_benefit_removed": False,
            "lifecycle_action_records_complete": True,
            "forbidden_outputs": 0,
            "unsafe_recommendations": 0,
            "raw_trace_or_secret_leaks": 0,
            "required_hits": 0,
            "required_total": 9,
            "useful_novel_action": False,
            "signature": "execution-failed",
        }
        runs = [{"case_id": "case", "metrics": failed_metrics}]
        aggregate = aggregate_metric_vector(
            [{"id": "case", "expectations": {"requires_abstention": True}}],
            runs,
        )
        self.assertEqual(aggregate["qualified_run_count"], 1)
        self.assertEqual(aggregate["completed_run_count"], 0)
        self.assertEqual(aggregate["required_outcome_recall"], 1.0)
        self.assertEqual(aggregate["unknown_abstention_accuracy"], 1.0)
        failures = exact_metric_failures(runs)
        self.assertIn(
            "one or more lifecycle executions failed before semantic closure",
            failures,
        )
        self.assertNotIn(
            "one or more cases violated its exact allowed closure",
            failures,
        )

    def test_typed_case_conclusion_is_scored_independently_from_optional_claims(self) -> None:
        case = next(
            case
            for case in self.corpus["cases"]
            if case["id"] == "lifecycle-intervening-mutation"
        )
        expected_probes = case["expectations"]["probe_answers"]
        definitions = [
            {"probe_id": f"probe:{slot}", "slot": slot}
            for slot in expected_probes
        ]
        artifact = {
            "schema_version": 2,
            "verification": {"verified": True},
            "probe_definitions": definitions,
            "probe_results": [
                {"probe_id": f"probe:{slot}", "answer": allowed[0]}
                for slot, allowed in expected_probes.items()
            ],
            "claims": [],
            "recommendations": [],
            "reviewer_runs": [{"case_conclusion": "repetition_justified"}],
            "execution_truth": [
                {
                    "closure": {"reason": "not_substantiated"},
                    "recommendation_dispositions": [],
                    "accepted_recommendations": [
                        {"family": "explicit_retention_avoidance"}
                    ],
                }
            ],
        }
        metrics = evaluate_run(case, {"findings": []}, {"findings": []}, artifact, {"valid": True})
        self.assertTrue(metrics["case_conclusion_valid"])
        self.assertTrue(metrics["probe_answers_valid"])
        self.assertTrue(metrics["required_recommendations_valid"])
        self.assertEqual(metrics["required_hits"], metrics["required_total"])
        original_answer = artifact["probe_results"][0]["answer"]
        artifact["probe_results"][0]["answer"] = "conflicting_evidence"
        missed_probe = evaluate_run(
            case,
            {"findings": []},
            {"findings": []},
            artifact,
            {"valid": True},
        )
        self.assertFalse(missed_probe["probe_answers_valid"])
        missed_probe["qualified"] = True
        missed_probe["execution_succeeded"] = True
        missed_probe["candidate_count_valid"] = True
        self.assertIn(
            "one or more admitted probe answers missed the exact corpus boundary",
            exact_metric_failures([{"case_id": case["id"], "metrics": missed_probe}]),
        )
        artifact["probe_results"][0]["answer"] = original_answer
        artifact["reviewer_runs"][0]["case_conclusion"] = "lifecycle_mismatch_substantiated"
        self.assertFalse(
            evaluate_run(
                case,
                {"findings": []},
                {"findings": []},
                artifact,
                {"valid": True},
            )["case_conclusion_valid"]
        )

    def test_material_signature_uses_typed_conclusion_and_accepted_action_only(self) -> None:
        artifact = {
            "claims": [{"slot": "repetition", "status": "substantiated"}],
            "reviewer_runs": [
                {"case_conclusion": "lifecycle_mismatch_substantiated"}
            ],
            "execution_truth": [
                {
                    "closure": {"reason": "resolved"},
                    "accepted_recommendations": [
                        {"family": "lifecycle_reownership"}
                    ],
                }
            ],
        }
        stable = conclusion_signature(artifact)
        artifact["claims"] = [
            {"slot": "wider_owner", "status": "partially_substantiated"}
        ]
        artifact["execution_truth"][0]["closure"]["reason"] = (
            "resolved_with_uncertainty"
        )
        self.assertEqual(conclusion_signature(artifact), stable)

        artifact["reviewer_runs"][0]["case_conclusion"] = "repetition_justified"
        self.assertNotEqual(conclusion_signature(artifact), stable)
        artifact["reviewer_runs"][0]["case_conclusion"] = (
            "lifecycle_mismatch_substantiated"
        )
        artifact["execution_truth"][0]["accepted_recommendations"][0][
            "family"
        ] = "explicit_retention_avoidance"
        self.assertNotEqual(conclusion_signature(artifact), stable)

    def test_evaluator_identity_binds_metric_contract_and_source(self) -> None:
        identity = evaluator_identity()
        self.assertEqual(identity["contract_version"], EVALUATOR_CONTRACT_VERSION)
        self.assertEqual(identity["digest"], evaluator_source_digest())
        self.assertEqual(len(identity["source_paths"]), 5)
        self.assertTrue(all(Path(path).is_file() for path in identity["source_paths"]))

    def test_timeout_retains_partial_subprocess_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout_path = root / "stdout.txt"
            stderr_path = root / "stderr.txt"
            with self.assertRaisesRegex(EvalError, "command timed out after 1s"):
                run_checked(
                    CheckedCommand(
                        [
                            sys.executable,
                            "-c",
                            (
                                "import sys,time;"
                                "print('stdout-before-timeout',flush=True);"
                                "print('stderr-before-timeout',file=sys.stderr,flush=True);"
                                "time.sleep(2)"
                            ),
                        ],
                        cwd=REPO_ROOT,
                        stdout_path=stdout_path,
                        stderr_path=stderr_path,
                        timeout=1,
                    )
                )
            self.assertEqual(stdout_path.read_text(), "stdout-before-timeout\n")
            self.assertEqual(stderr_path.read_text(), "stderr-before-timeout\n")

    def test_no_outer_timeout_still_captures_completed_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout_path = root / "stdout.txt"
            stderr_path = root / "stderr.txt"
            run_checked(
                CheckedCommand(
                    [
                        sys.executable,
                        "-c",
                        "import sys;print('complete');print('clean',file=sys.stderr)",
                    ],
                    cwd=REPO_ROOT,
                    stdout_path=stdout_path,
                    stderr_path=stderr_path,
                    timeout=None,
                )
            )
            self.assertEqual(stdout_path.read_text(), "complete\n")
            self.assertEqual(stderr_path.read_text(), "clean\n")

    def test_run_checked_can_enable_transcript_capture_for_only_the_review(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout_path = root / "stdout.txt"
            stderr_path = root / "stderr.txt"
            run_checked(
                CheckedCommand(
                    [
                        sys.executable,
                        "-c",
                        "import os;print(os.environ.get('AUDITOR_REVIEW_TRANSCRIPT','missing'))",
                    ],
                    cwd=REPO_ROOT,
                    stdout_path=stdout_path,
                    stderr_path=stderr_path,
                    timeout=5,
                    env_overrides={"AUDITOR_REVIEW_TRANSCRIPT": "1"},
                )
            )
            self.assertEqual(stdout_path.read_text(), "1\n")
            self.assertEqual(stderr_path.read_text(), "")


if __name__ == "__main__":
    unittest.main()

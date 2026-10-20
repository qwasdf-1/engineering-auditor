import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from detector_gate_contract import CatalogEntry, full_profile_languages, validate_precision  # noqa: E402


def entry(detector_id: str, profile: str) -> CatalogEntry:
    return CatalogEntry(detector_id=detector_id, category="c", profile=profile, layer="L2", owner="O")


def row(standing: str, **languages: str) -> dict:
    return {"standing": standing, "by_language": {name: {"standing": value} for name, value in languages.items()}}


class PrecisionGateTests(unittest.TestCase):
    def test_gating_detectors_below_the_bar_are_rejected(self) -> None:
        entries = [entry("noisy", "gating"), entry("noisy_rust", "gating"), entry("advisory", "full"),
                   entry("unsure", "gating")]
        table = {"noisy": row("below_bar"), "noisy_rust": row("unverified", rust="below_bar", python="meets_bar"),
                 "advisory": row("below_bar"), "unsure": row("unverified", go="insufficient_sample")}
        errors = validate_precision(entries, table)
        self.assertEqual(len(errors), 2)
        self.assertIn("noisy is below the precision bar (overall)", errors[0])
        self.assertIn("noisy_rust is below the precision bar (rust)", errors[1])

    def test_languages_scoped_to_full_are_exempt_and_the_rest_rejudged(self) -> None:
        def cell(tp: int, fp: int) -> dict:
            return {"true_positive": tp, "false_positive": fp, "unverified": 0, "accepted": 0,
                    "standing": "below_bar" if fp * 2 > tp else "meets_bar"}
        noisy_java = {"standing": "below_bar", "by_language": {"java": cell(5, 7), "go": cell(40, 0)}}
        scoped = CatalogEntry("scoped", "c", "gating", "L2", "O", ("java",))
        self.assertEqual(validate_precision([scoped], {"scoped": noisy_java}), [])
        unscoped = CatalogEntry("scoped", "c", "gating", "L2", "O")
        self.assertIn("(java)", validate_precision([unscoped], {"scoped": noisy_java})[0])
        small = {"true_positive": 2, "false_positive": 3, "unverified": 0, "accepted": 0,
                 "standing": "insufficient_sample"}
        still_noisy = {"standing": "below_bar", "by_language": {"java": cell(0, 3), "go": small, "rust": small}}
        self.assertIn("(overall)", validate_precision([scoped], {"scoped": still_noisy})[0])

    def test_insufficient_samples_gate_only_without_live_false_positives(self) -> None:
        def cell(fp: int) -> dict:
            return {"true_positive": 4, "false_positive": fp, "unverified": 0, "accepted": 0,
                    "standing": "insufficient_sample"}
        clean = {"standing": "insufficient_sample", "false_positive": 0, "by_language": {"go": cell(0)}}
        noisy = {"standing": "insufficient_sample", "false_positive": 2, "by_language": {"go": cell(2)}}
        self.assertEqual(validate_precision([entry("rare", "gating")], {"rare": clean}), [])
        self.assertEqual(validate_precision([entry("rare", "full")], {"rare": noisy}), [])
        self.assertIn("2 live false positive(s)", validate_precision([entry("rare", "gating")], {"rare": noisy})[0])
        split = {"standing": "insufficient_sample", "false_positive": 2,
                 "by_language": {"java": cell(2), "go": cell(0)}}
        scoped = CatalogEntry("rare", "c", "gating", "L2", "O", ("java",))
        self.assertEqual(validate_precision([scoped], {"rare": split}), [])

    def test_full_profile_languages_are_parsed_from_the_spec_chain(self) -> None:
        self.assertEqual(full_profile_languages('\n    .full_profile_in(&["java", "python"]),'), ("java", "python"))
        self.assertEqual(full_profile_languages(",\n    spec!("), ())
        chained = '\n    .with_roles(RoleApplicability::Production)\n    .full_profile_in(&["c"]),'
        self.assertEqual(full_profile_languages(chained), ("c",))

    def test_snapshot_must_name_catalog_detectors(self) -> None:
        self.assertEqual(validate_precision([], {"gone": row("meets_bar")}),
                         ["precision snapshot names unknown detector: gone"])


if __name__ == "__main__":
    unittest.main()

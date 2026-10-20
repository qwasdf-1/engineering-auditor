import tempfile
import unittest
from pathlib import Path

from tooling.lint.detector_arch import parse_registry, parse_source_emissions


class DetectorArchitectureParserTests(unittest.TestCase):
    def write_temp(self, source: str) -> Path:
        handle = tempfile.NamedTemporaryFile(mode="w", suffix=".rs", delete=False)
        self.addCleanup(Path(handle.name).unlink, missing_ok=True)
        with handle:
            handle.write(source)
        return Path(handle.name)

    def test_registry_parser_reads_spec_macro(self) -> None:
        path = self.write_temp('spec!("stable_id", "correctness", High, Gating, "L1", Owner);')
        self.assertEqual(parse_registry(path), {"stable_id": "correctness"})

    def test_emission_parser_ignores_inline_test_module(self) -> None:
        path = self.write_temp(
            'Finding::new("runtime", "correctness", title);\n'
            '#[cfg(test)]\nmod tests {\n'
            '    Finding::new("test_only", "correctness", title);\n}\n'
        )
        self.assertEqual(set(parse_source_emissions(path)), {"runtime"})

    def test_emission_parser_reads_structural_finding_text(self) -> None:
        path = self.write_temp(
            'const SPEC: FindingText = FindingText {\n    detector: "typed_text",\n    title: "T",\n};\n'
        )
        self.assertEqual(parse_source_emissions(path)["typed_text"][0]["category"], "structural")

    def test_emission_parser_reads_typed_rule_declaration(self) -> None:
        path = self.write_temp(
            'const TYPED_RULE: Rule = Rule {\n    id: "typed_rule",\n    title: "Rule",\n};\n'
        )
        self.assertEqual(set(parse_source_emissions(path)), {"typed_rule"})

    def test_emission_parser_reads_typed_graph_constructor(self) -> None:
        path = self.write_temp(
            'finding("graph_rule", "architecture", title, message, path, recommendation);\n'
        )
        emissions = parse_source_emissions(path)
        self.assertEqual(set(emissions), {"graph_rule"})
        self.assertEqual(emissions["graph_rule"][0]["category"], "architecture")

    def test_emission_parser_reads_typed_systems_effect_constructor(self) -> None:
        path = self.write_temp(
            'effect_finding(effect, "systems_rule", "Title", behavior, recommendation);\n'
        )
        emissions = parse_source_emissions(path)
        self.assertEqual(set(emissions), {"systems_rule"})
        self.assertEqual(
            emissions["systems_rule"][0]["category"], "systems_economics"
        )

    def test_emission_parser_reads_typed_grouped_systems_constructor(self) -> None:
        path = self.write_temp(
            'grouped_finding("grouped_systems_rule", "Title", behavior, evidence);\n'
        )
        emissions = parse_source_emissions(path)
        self.assertEqual(set(emissions), {"grouped_systems_rule"})
        self.assertEqual(
            emissions["grouped_systems_rule"][0]["category"], "systems_economics"
        )

    def test_emission_parser_reads_typed_counted_algorithmic_constructor(self) -> None:
        path = self.write_temp(
            'counted_finding("repeated_work", fact, SPEC, count, message);\n'
        )
        emissions = parse_source_emissions(path)
        self.assertEqual(set(emissions), {"repeated_work"})
        self.assertEqual(emissions["repeated_work"][0]["category"], "algorithmic")


if __name__ == "__main__":
    unittest.main()

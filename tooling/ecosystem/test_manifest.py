"""Validate the ecosystem corpus manifest against its schema and tier rule."""

import json
import re
import sys
import unittest
from collections import Counter
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))
from tooling.ecosystem import run  # noqa: E402
SCHEMA = REPO_ROOT / "contracts" / "ecosystem-corpus.schema.json"
MANIFEST = REPO_ROOT / "eval" / "ecosystem" / "manifest.json"
TIER_1 = {"rust", "python", "typescript", "javascript", "go", "java"}
TIER_2 = {"kotlin", "csharp", "c", "cpp"}


def schema_errors(value, schema, path="$", root=None):
    """Check the closed subset of JSON Schema the corpus schema uses."""
    root = root or schema
    if "$ref" in schema:
        schema = root["$defs"][schema["$ref"].removeprefix("#/$defs/")]
    errors = []
    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: expected {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} not in enum")
    kind = schema.get("type")
    if kind == "object":
        if not isinstance(value, dict):
            return [f"{path}: expected object"]
        properties = schema.get("properties", {})
        errors += [f"{path}: missing {key}" for key in schema.get("required", []) if key not in value]
        if schema.get("additionalProperties") is False:
            errors += [f"{path}: unexpected {key}" for key in value if key not in properties]
        for key, item in value.items():
            if key in properties:
                errors += schema_errors(item, properties[key], f"{path}.{key}", root)
    elif kind == "array":
        if not isinstance(value, list):
            return [f"{path}: expected array"]
        if len(value) < schema.get("minItems", 0):
            errors.append(f"{path}: too few items")
        for index, item in enumerate(value):
            errors += schema_errors(item, schema["items"], f"{path}[{index}]", root)
    elif kind == "string":
        if not isinstance(value, str):
            return [f"{path}: expected string"]
        if len(value) < schema.get("minLength", 0):
            errors.append(f"{path}: too short")
        if "pattern" in schema and not re.search(schema["pattern"], value):
            errors.append(f"{path}: {value!r} does not match {schema['pattern']}")
    elif kind == "integer":
        if not isinstance(value, int) or isinstance(value, bool):
            return [f"{path}: expected integer"]
        if value < schema.get("minimum", value):
            errors.append(f"{path}: below minimum")
    return errors


class EcosystemManifestTest(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        self.manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))

    def test_manifest_matches_schema(self):
        self.assertEqual(schema_errors(self.manifest, self.schema), [])

    def test_ids_and_commits_are_unique(self):
        repositories = self.manifest["repositories"] + self.manifest.get("applications", [])
        for field in ("id", "commit", "url"):
            values = [repository[field] for repository in repositories]
            self.assertEqual(len(values), len(set(values)), field)

    def test_every_tier_1_and_tier_2_language_has_four_repositories(self):
        counts = Counter(repository["language"] for repository in self.manifest["repositories"])
        self.assertEqual({language: counts[language] >= 4 for language in TIER_1 | TIER_2},
                         {language: True for language in TIER_1 | TIER_2})

    def test_only_application_repositories_name_detectors(self):
        self.assertTrue(all("detectors" not in repository for repository in self.manifest["repositories"]))
        self.assertTrue(all(repository.get("detectors") for repository in self.manifest.get("applications", [])))

    def test_the_seeded_rule_alone_picks_the_held_out_repositories(self):
        marked = {repository["id"] for repository in self.manifest["repositories"] + self.manifest["applications"]
                  if repository.get("holdout")}
        self.assertEqual(marked, run.holdout_choice(self.manifest))
        self.assertEqual(marked, run.holdout_ids())
        held_languages = {repository["language"] for repository in self.manifest["repositories"]
                          if repository.get("holdout")}
        self.assertTrue(TIER_1 | TIER_2 <= held_languages)

    def test_validator_rejects_malformed_entries(self):
        broken = json.loads(json.dumps(self.manifest))
        broken["repositories"][0]["commit"] = "main"
        broken["repositories"][0]["extra"] = True
        errors = schema_errors(broken, self.schema)
        self.assertTrue(any("commit" in error for error in errors))
        self.assertTrue(any("unexpected extra" in error for error in errors))


if __name__ == "__main__":
    unittest.main()

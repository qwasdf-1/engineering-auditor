import unittest

from tooling.release.changelog_section import section

CHANGELOG = """# Changelog

## Unreleased

- pending

## 1.0.0 - First Stable Release

### Added

- stable surfaces

## 0.9.19 - Stable Finding Identity

- identity v3
"""


class ChangelogSectionTest(unittest.TestCase):
    def test_extracts_the_titled_section_up_to_the_next_heading(self) -> None:
        self.assertEqual(section(CHANGELOG, "1.0.0"), "### Added\n\n- stable surfaces\n")
        self.assertEqual(section(CHANGELOG, "0.9.19"), "- identity v3\n")

    def test_version_prefixes_do_not_match_other_versions(self) -> None:
        with self.assertRaisesRegex(LookupError, "no `## 1.0` section"):
            section(CHANGELOG, "1.0")

    def test_empty_sections_are_errors(self) -> None:
        with self.assertRaisesRegex(LookupError, "is empty"):
            section("## 2.0.0\n\n## 1.0.0\n- x\n", "2.0.0")


if __name__ == "__main__":
    unittest.main()

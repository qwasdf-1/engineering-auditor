import tempfile
import unittest
from pathlib import Path

from tooling.release.third_party_licenses import license_files, render, shipped_packages


def package(directory: Path, name: str, license: str = "MIT", **extra) -> dict:
    return {
        "id": name,
        "name": name,
        "version": "1.0.0",
        "license": license,
        "manifest_path": str(directory / "Cargo.toml"),
        **extra,
    }


class ThirdPartyLicensesTest(unittest.TestCase):
    def test_walks_normal_and_build_deps_but_not_dev_deps(self) -> None:
        meta = {
            "workspace_members": ["cli"],
            "packages": [
                {"id": "cli", "name": "auditor-cli", "version": "1.0.0"},
                {"id": "a", "name": "a", "version": "1.0.0"},
                {"id": "b", "name": "b", "version": "1.0.0"},
                {"id": "dev", "name": "dev", "version": "1.0.0"},
            ],
            "resolve": {
                "nodes": [
                    {"id": "cli", "deps": [
                        {"pkg": "a", "dep_kinds": [{"kind": None}]},
                        {"pkg": "dev", "dep_kinds": [{"kind": "dev"}]},
                    ]},
                    {"id": "a", "deps": [{"pkg": "b", "dep_kinds": [{"kind": "build"}]}]},
                    {"id": "b", "deps": []},
                    {"id": "dev", "deps": []},
                ]
            },
        }
        self.assertEqual([p["name"] for p in shipped_packages(meta)], ["a", "b"])

    def test_groups_identical_texts_and_lists_crates_without_files(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            first, second, bare = (Path(root) / name for name in ("first", "second", "bare"))
            for directory in (first, second, bare):
                directory.mkdir()
            (first / "LICENSE-MIT").write_text("MIT text\n")
            (second / "license.txt").write_text("MIT text\n")
            (second / "README.md").write_text("not a license\n")
            packages = [package(first, "first"), package(second, "second"), package(bare, "bare", "Zlib")]
            self.assertEqual([p.name for p in license_files(packages[1])], ["license.txt"])
            text = render(packages)
            self.assertEqual(text.count("MIT text"), 1)
            self.assertIn("  first 1.0.0 (MIT)\n  second 1.0.0 (MIT)\n", text)
            self.assertIn("ship no license file", text)
            self.assertIn("  bare 1.0.0 (Zlib)\n", text)


if __name__ == "__main__":
    unittest.main()

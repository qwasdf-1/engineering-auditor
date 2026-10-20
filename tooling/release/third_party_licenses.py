#!/usr/bin/env python3
"""Write the license notices of every crate compiled into `auditor`.

Usage: third_party_licenses.py [--target <triple>] [--output <path>]

Reads `cargo metadata` for the locked workspace, walks the normal and build
dependencies of `auditor-cli` (for one target when `--target` is given), and
writes each package's license files. Packages that share an identical
license text are listed together under one copy of it. A package with no
license file in its crate is listed with its SPDX expression.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT_PACKAGE = "auditor-cli"
LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "unlicense")
HEADER = (
    "Third-party licenses for Engineering Auditor\n"
    "============================================\n\n"
    "The `auditor` binary includes the Rust crates listed below. Each\n"
    "license text is followed by the crates distributed under it.\n"
)


def metadata(target: str | None) -> dict:
    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if target:
        command += ["--filter-platform", target]
    return json.loads(subprocess.run(command, check=True, capture_output=True, text=True).stdout)


def shipped_packages(meta: dict) -> list[dict]:
    """Non-workspace packages reachable from the CLI through normal or build deps."""
    packages = {package["id"]: package for package in meta["packages"]}
    nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
    workspace = set(meta["workspace_members"])
    root = next(i for i in workspace if packages[i]["name"] == ROOT_PACKAGE)
    seen, pending = set(), [root]
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        for dep in nodes[package_id]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    shipped = [packages[i] for i in seen - workspace]
    return sorted(shipped, key=lambda package: (package["name"], package["version"]))


def license_files(package: dict) -> list[Path]:
    directory = Path(package["manifest_path"]).parent
    found = {
        path
        for path in directory.iterdir()
        if path.is_file() and path.name.lower().startswith(LICENSE_PREFIXES)
    }
    if package.get("license_file"):
        declared = directory / package["license_file"]
        if declared.is_file():
            found.add(declared)
    return sorted(found)


def render(packages: list[dict]) -> str:
    groups: dict[str, list[str]] = {}
    unfiled: list[str] = []
    for package in packages:
        label = f"{package['name']} {package['version']} ({package.get('license') or 'no SPDX expression'})"
        texts = [path.read_text(encoding="utf-8", errors="replace").strip() for path in license_files(package)]
        if not texts:
            unfiled.append(label)
        for text in texts:
            groups.setdefault(text, []).append(label)
    sections = [HEADER]
    for text, labels in sorted(groups.items(), key=lambda item: item[1][0]):
        sections.append("-" * 72 + "\n\n" + text + "\n\nUsed by:\n" + "".join(f"  {label}\n" for label in labels))
    if unfiled:
        sections.append(
            "-" * 72
            + "\n\nThese crates ship no license file; their SPDX expression is shown:\n"
            + "".join(f"  {label}\n" for label in unfiled)
        )
    return "\n".join(sections)


def main() -> int:
    parser = argparse.ArgumentParser(description="Write the license notices of every crate compiled into auditor.")
    parser.add_argument("--target")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    text = render(shipped_packages(metadata(args.target)))
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

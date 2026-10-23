"""The held-out corpus repositories: one per library language and
HOLDOUT_APPLICATIONS applications, chosen by seeded hash so no one picks
them. Detector fixes never read their findings or labels; their standing
measures whether a fix generalizes beyond the repositories that motivated it.
"""

import hashlib
import json
from collections import defaultdict
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[2] / "eval" / "ecosystem" / "manifest.json"
HOLDOUT_SEED = "ea-holdout-v1"
HOLDOUT_APPLICATIONS = 6


def holdout_rank(repository_id) -> str:
    return hashlib.sha256(f"{HOLDOUT_SEED}:{repository_id}".encode()).hexdigest()


def holdout_choice(manifest) -> set:
    """The ids the seeded rule holds out of `manifest`."""
    by_language = defaultdict(list)
    for repository in manifest["repositories"]:
        by_language[repository["language"]].append(repository["id"])
    chosen = {min(ids, key=holdout_rank) for ids in by_language.values()}
    applications = sorted((repository["id"] for repository in manifest.get("applications", [])), key=holdout_rank)
    return chosen | set(applications[:HOLDOUT_APPLICATIONS])


def holdout_ids() -> set:
    """The ids the manifest marks `holdout`."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    return {repository["id"] for slice_name in ("repositories", "applications")
            for repository in manifest.get(slice_name, []) if repository.get("holdout")}

"""Shared contracts for the lifecycle evaluator and its focused owners."""

from __future__ import annotations

import json
import hashlib
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeVar

QUALIFICATION_CASES = {
    "positive",
    "negative",
    "unknown",
    "conflicting_evidence",
    "forbidden_redesign",
}


class EvalError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


ErrorType = TypeVar("ErrorType", bound=RuntimeError)


def load_json_object(
    path: Path, error_type: type[ErrorType], artifact_name: str
) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise error_type(f"failed to load {artifact_name} {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise error_type(f"{artifact_name} is not an object: {path}")
    return value


def load_json(path: Path) -> dict[str, Any]:
    return load_json_object(path, EvalError, "JSON")


def artifact_for(reviewed_report: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    reviewed = load_json(reviewed_report)
    envelope = reviewed.get("engineering_review") or {}
    relative = envelope.get("path")
    if not isinstance(relative, str) or not relative:
        raise EvalError("reviewed report has no engineering-review artifact path")
    return reviewed, load_json(reviewed_report.parent / relative)


@dataclass(frozen=True)
class CheckedCommand:
    argv: list[str]
    cwd: Path
    stdout_path: Path
    stderr_path: Path
    timeout: int | None
    env_overrides: dict[str, str] | None = None


def run_checked(command: CheckedCommand) -> None:
    try:
        completed = subprocess.run(
            command.argv,
            cwd=command.cwd,
            capture_output=True,
            text=True,
            check=False,
            timeout=command.timeout,
            env=(
                {**os.environ, **command.env_overrides}
                if command.env_overrides is not None
                else None
            ),
        )
    except subprocess.TimeoutExpired as exc:
        command.stdout_path.write_text(_output_text(exc.stdout))
        command.stderr_path.write_text(_output_text(exc.stderr))
        raise EvalError(
            f"command timed out after {command.timeout}s: {command.argv[:3]}"
        ) from exc
    command.stdout_path.write_text(completed.stdout)
    command.stderr_path.write_text(completed.stderr)
    if completed.returncode:
        detail = (completed.stderr or completed.stdout).strip()[-800:]
        raise EvalError(
            f"command failed ({completed.returncode}): {command.argv[:3]}: {detail}"
        )


def _output_text(output: bytes | str | None) -> str:
    if isinstance(output, bytes):
        return output.decode(errors="replace")
    return output or ""

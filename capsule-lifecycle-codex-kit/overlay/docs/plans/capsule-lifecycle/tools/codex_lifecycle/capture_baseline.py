#!/usr/bin/env python3
"""Capture a non-destructive Capsule repository baseline as JSON.

This script uses only the Python standard library. It does not fetch dependencies,
modify the checkout, or run generated builders. Run it from the repository root:

    python docs/plans/capsule-lifecycle/tools/codex_lifecycle/capture_baseline.py \
        --output docs/plans/capsule-lifecycle/evidence/M00/baseline.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from typing import Any, Sequence


REQUIRED_PATHS = (
    "AGENTS.md",
    "CONTRIBUTING.md",
    "README.md",
    "format",
    "tools/capsule.py",
    "tools/capsule_author.py",
    "tools/capsule_conformance.py",
    "tools/capsule_signatures.py",
    "native/Cargo.toml",
    "native/crates/capsule-core",
    "native/crates/capsule-crypto",
    "native/crates/capsule-lifecycle",
    "native/crates/capsule-runtime",
    "native/desktop/src-tauri",
    "native/desktop/ui",
    "plugins/capsule-creator",
    "examples/diagram-studio",
    "tests",
)

KEY_FILES = (
    "AGENTS.md",
    "CONTRIBUTING.md",
    "README.md",
    "pyproject.toml",
    "package.json",
    "format/capsule-v0.2.sql",
    "format/capsule-signed-app-v0.2.sql",
    "native/Cargo.toml",
    "native/Cargo.lock",
    "native/README.md",
    "native/desktop/ui/index.html",
    "native/desktop/ui/app.js",
    "native/crates/capsule-core/src/lib.rs",
    "native/crates/capsule-crypto/src/lib.rs",
    "native/crates/capsule-lifecycle/src/lib.rs",
)


class BaselineError(RuntimeError):
    """Raised for an invalid repository root or failed required command."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(
    argv: Sequence[str],
    *,
    cwd: Path,
    required: bool = False,
    timeout: int = 30,
) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            list(argv),
            cwd=cwd,
            check=False,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired) as exc:
        if required:
            raise BaselineError(f"required command failed: {argv!r}: {exc}") from exc
        return {
            "argv": list(argv),
            "available": False,
            "exit_code": None,
            "stdout": "",
            "stderr": str(exc),
        }

    result = {
        "argv": list(argv),
        "available": True,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }
    if required and completed.returncode != 0:
        raise BaselineError(
            f"required command returned {completed.returncode}: {argv!r}\n"
            f"{completed.stderr.strip()}"
        )
    return result


def tool_version(command: str, args: Sequence[str]) -> dict[str, Any]:
    executable = shutil.which(command)
    if executable is None:
        return {"available": False, "path": None, "result": None}
    return {
        "available": True,
        "path": executable,
        "result": run([executable, *args], cwd=Path.cwd(), timeout=20),
    }


def inventory_path(repo: Path, relative: str) -> dict[str, Any]:
    path = repo / relative
    record: dict[str, Any] = {
        "path": relative,
        "exists": path.exists(),
        "kind": None,
    }
    if path.is_file():
        record.update(
            kind="file",
            size_bytes=path.stat().st_size,
            sha256=sha256_file(path),
        )
    elif path.is_dir():
        files = sum(1 for item in path.rglob("*") if item.is_file())
        record.update(kind="directory", file_count=files)
    return record


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument(
        "--output",
        type=Path,
        help="Output JSON path. Relative paths are resolved under --repo.",
    )
    args = parser.parse_args()

    repo = args.repo.expanduser().resolve()
    missing = [relative for relative in REQUIRED_PATHS if not (repo / relative).exists()]
    if missing:
        raise BaselineError(
            f"{repo} is not a compatible Capsule checkout; missing: {', '.join(missing)}"
        )

    git_root = run(
        ["git", "rev-parse", "--show-toplevel"], cwd=repo, required=True
    )["stdout"]
    if Path(git_root).resolve() != repo:
        raise BaselineError(
            f"--repo must be the checkout root; git reports {git_root!r}"
        )

    now = datetime.now(timezone.utc).replace(microsecond=0)
    output = args.output
    if output is None:
        output = Path(
            "docs/plans/capsule-lifecycle/evidence/M00/"
            f"baseline-{now.strftime('%Y%m%dT%H%M%SZ')}.json"
        )
    if not output.is_absolute():
        output = repo / output
    output = output.resolve()
    try:
        output.relative_to(repo)
    except ValueError as exc:
        raise BaselineError("--output must remain inside the repository") from exc

    status = run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=repo,
        required=True,
    )
    branch = run(["git", "branch", "--show-current"], cwd=repo, required=True)
    commit = run(["git", "rev-parse", "HEAD"], cwd=repo, required=True)
    commit_time = run(
        ["git", "show", "-s", "--format=%cI", "HEAD"], cwd=repo, required=True
    )
    remote = run(["git", "remote", "-v"], cwd=repo, required=False)

    key_inventory = [
        inventory_path(repo, relative)
        for relative in KEY_FILES
        if (repo / relative).exists()
    ]
    required_inventory = [inventory_path(repo, relative) for relative in REQUIRED_PATHS]

    cwd_before = Path.cwd()
    try:
        os.chdir(repo)
        tools = {
            "python": {
                "available": True,
                "path": sys.executable,
                "version": sys.version.replace("\n", " "),
            },
            "git": tool_version("git", ["--version"]),
            "node": tool_version("node", ["--version"]),
            "npm": tool_version("npm", ["--version"]),
            "rustc": tool_version("rustc", ["--version", "--verbose"]),
            "cargo": tool_version("cargo", ["--version", "--verbose"]),
            "sqlite3_cli": tool_version("sqlite3", ["--version"]),
        }
    finally:
        os.chdir(cwd_before)

    payload: dict[str, Any] = {
        "profile": "org.sqlite-capsule.lifecycle-baseline/1",
        "captured_at": now.isoformat().replace("+00:00", "Z"),
        "repository": {
            "root": str(repo),
            "commit": commit["stdout"],
            "commit_time": commit_time["stdout"],
            "branch": branch["stdout"],
            "dirty": bool(status["stdout"]),
            "status_porcelain": status["stdout"].splitlines(),
            "remotes": remote["stdout"].splitlines(),
        },
        "host": {
            "platform": platform.platform(),
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python_implementation": platform.python_implementation(),
        },
        "tools": tools,
        "required_paths": required_inventory,
        "key_files": key_inventory,
        "notes": [
            "No tests or builders were run by this script.",
            "Paths and remotes may be sensitive; retain evidence according to repository policy.",
        ],
    }

    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    temporary.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    os.replace(temporary, output)
    print(output.relative_to(repo).as_posix())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BaselineError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

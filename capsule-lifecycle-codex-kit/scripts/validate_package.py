#!/usr/bin/env python3
"""Validate the distributable Capsule lifecycle Codex kit."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import py_compile
import sqlite3
import sys
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "PACKAGE_MANIFEST.json"
REQUIRED = (
    "README.md",
    "INSTALL.md",
    "START_PROMPT.md",
    "PACKAGE_NOTES.md",
    "PACKAGE_INFO.json",
    "VALIDATION_REPORT.md",
    "overlay/CODEX_LIFECYCLE_START.md",
    "overlay/.agents/skills/capsule-lifecycle-program/SKILL.md",
    "overlay/docs/plans/capsule-lifecycle/PROGRAM_STATUS.json",
    "overlay/docs/plans/capsule-lifecycle/contracts/capsule-v0.3-draft.sql",
    "overlay/docs/plans/capsule-lifecycle/contracts/capsule-signed-app-v0.3-draft.sql",
    "scripts/install.py",
    "scripts/validate_package.py",
)


class PackageError(RuntimeError):
    pass


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise PackageError(f"{path.relative_to(ROOT)}: {exc}") from exc


def validate_manifest(require: bool) -> int:
    if not MANIFEST.exists():
        if require:
            raise PackageError("PACKAGE_MANIFEST.json is missing")
        print("manifest: not present (allowed before packaging)")
        return 0
    data = load_json(MANIFEST)
    entries = data.get("files")
    if not isinstance(entries, list):
        raise PackageError("manifest files must be an array")
    manifest_paths: set[str] = set()
    for entry in entries:
        relative = entry.get("path")
        if not isinstance(relative, str) or relative in manifest_paths:
            raise PackageError(f"invalid/duplicate manifest path: {relative!r}")
        manifest_paths.add(relative)
        path = ROOT / relative
        if not path.is_file():
            raise PackageError(f"manifest file missing: {relative}")
        if path.stat().st_size != entry.get("size_bytes"):
            raise PackageError(f"manifest size mismatch: {relative}")
        if digest(path) != entry.get("sha256"):
            raise PackageError(f"manifest digest mismatch: {relative}")
    actual = {
        path.relative_to(ROOT).as_posix()
        for path in ROOT.rglob("*")
        if path.is_file()
        and path != MANIFEST
        and "__pycache__" not in path.parts
        and not path.name.endswith(".pyc")
    }
    if actual != manifest_paths:
        missing = sorted(actual - manifest_paths)
        extra = sorted(manifest_paths - actual)
        raise PackageError(f"manifest inventory mismatch; missing={missing}, extra={extra}")
    print(f"manifest: {len(entries)} files verified")
    return len(entries)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--require-manifest", action="store_true")
    args = parser.parse_args()

    for relative in REQUIRED:
        if not (ROOT / relative).is_file():
            raise PackageError(f"required file missing: {relative}")

    symlinks = [path for path in ROOT.rglob("*") if path.is_symlink()]
    if symlinks:
        raise PackageError(f"symlinks are not permitted: {symlinks!r}")

    json_count = 0
    for path in sorted(ROOT.rglob("*.json")):
        if path == MANIFEST:
            continue
        load_json(path)
        json_count += 1
    print(f"json: {json_count} files parse")

    python_count = 0
    with tempfile.TemporaryDirectory(prefix="capsule-kit-pyc-") as temporary:
        compile_root = Path(temporary)
        for path in sorted(ROOT.rglob("*.py")):
            relative = path.relative_to(ROOT).as_posix()
            target = compile_root / f"{hashlib.sha256(relative.encode('utf-8')).hexdigest()}.pyc"
            py_compile.compile(str(path), cfile=str(target), doraise=True)
            python_count += 1
    print(f"python: {python_count} files compile")

    base = ROOT / "overlay/docs/plans/capsule-lifecycle/contracts/capsule-v0.3-draft.sql"
    signed = ROOT / "overlay/docs/plans/capsule-lifecycle/contracts/capsule-signed-app-v0.3-draft.sql"
    connection = sqlite3.connect(":memory:")
    try:
        connection.executescript(base.read_text(encoding="utf-8"))
        connection.executescript(signed.read_text(encoding="utf-8"))
        result = connection.execute("PRAGMA quick_check").fetchone()
        if result != ("ok",):
            raise PackageError(f"SQL quick_check failed: {result!r}")
    except sqlite3.Error as exc:
        raise PackageError(f"SQL draft compilation failed: {exc}") from exc
    finally:
        connection.close()
    print("sql: v0.3 base and signed extension compile")

    status = load_json(
        ROOT / "overlay/docs/plans/capsule-lifecycle/PROGRAM_STATUS.json"
    )
    milestones = status.get("milestones", [])
    if [item.get("id") for item in milestones] != [
        f"M{index:02d}" for index in range(10)
    ]:
        raise PackageError("status milestone IDs/order are invalid")
    for item in milestones:
        directory = (
            ROOT
            / "overlay/docs/plans/capsule-lifecycle/milestones"
            / f"{item['id']}-{item['slug']}"
        )
        for filename in ("EXECPLAN.md", "PROMPT.md", "ACCEPTANCE.md", "RESULT.md"):
            if not (directory / filename).is_file():
                raise PackageError(f"{item['id']}: missing {filename}")
    print("milestones: ten complete planning bundles")

    validate_manifest(args.require_manifest)
    print("package: ok")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (PackageError, py_compile.PyCompileError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

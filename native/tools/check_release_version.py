#!/usr/bin/env python3
"""Verify that all repository package versions match an optional release tag."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
VERSION_RE = re.compile(r"\d+\.\d+\.\d+")
TAG_RE = re.compile(r"v(\d+\.\d+\.\d+)")


class ReleaseVersionError(ValueError):
    pass


def read_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def repository_versions() -> dict[str, str]:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (ROOT / "package-lock.json").read_text(encoding="utf-8")
    )
    pyproject = read_toml(ROOT / "pyproject.toml")
    tauri = json.loads(
        (ROOT / "native/desktop/src-tauri/tauri.conf.json").read_text(
            encoding="utf-8"
        )
    )
    versions = {
        "package.json": package["version"],
        "package-lock.json": package_lock["version"],
        "package-lock.json packages root": package_lock["packages"][""]["version"],
        "pyproject.toml": pyproject["project"]["version"],
        "native desktop Cargo.toml": read_toml(
            ROOT / "native/desktop/src-tauri/Cargo.toml"
        )["package"]["version"],
        "tauri.conf.json": tauri["version"],
    }
    for path in sorted((ROOT / "native/crates").glob("*/Cargo.toml")):
        versions[f"native crate {path.parent.name}"] = read_toml(path)["package"][
            "version"
        ]
    return versions


def verify_release_version(tag: str | None = None) -> tuple[str, dict[str, str]]:
    versions = repository_versions()
    invalid = {
        name: version
        for name, version in versions.items()
        if not VERSION_RE.fullmatch(version)
    }
    if invalid:
        raise ReleaseVersionError(f"invalid package versions: {invalid}")
    distinct = sorted(set(versions.values()))
    if len(distinct) != 1:
        raise ReleaseVersionError(f"repository package versions do not match: {versions}")
    version = distinct[0]
    if tag is not None:
        match = TAG_RE.fullmatch(tag)
        if match is None:
            raise ReleaseVersionError("release tags must use vMAJOR.MINOR.PATCH")
        if match.group(1) != version:
            raise ReleaseVersionError(
                f"release tag {tag} does not match repository version {version}"
            )
    return version, versions


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="optional vMAJOR.MINOR.PATCH release tag")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        version, versions = verify_release_version(args.tag)
        print(
            json.dumps(
                {"ok": True, "version": version, "tag": args.tag, "sources": versions},
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (KeyError, OSError, json.JSONDecodeError, tomllib.TOMLDecodeError, ReleaseVersionError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, indent=2), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

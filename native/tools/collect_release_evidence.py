#!/usr/bin/env python3
"""Collect deterministic evidence for native host bundle artifacts.

This collector deliberately supports only development-unsigned output. Signed
release evidence must be produced by platform-specific verification jobs rather
than inferred from a file extension or from Tauri updater sidecars.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path
from typing import Any


NATIVE_ROOT = Path(__file__).resolve().parents[1]
FORMAT = "org.sqlite-capsule.host-build-evidence/0.2"
TREE_CONTEXT = b"SQLite Capsule bundle tree v1\0"
REVISION_RE = re.compile(r"[0-9a-f]{40}(?:[0-9a-f]{24})?\Z")
MAX_ARTIFACTS = 128


class EvidenceError(ValueError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def regular_file(path: Path, label: str) -> None:
    metadata = path.lstat()
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise EvidenceError(f"{label} must be a regular non-symlink file")


def artifact_kind(path: Path) -> str | None:
    name = path.name
    lower = name.lower()
    if lower.endswith(".msi"):
        return "windows-msi"
    if lower.endswith(".exe"):
        return "windows-nsis"
    if lower.endswith(".dmg"):
        return "macos-dmg"
    if name.endswith(".AppImage"):
        return "linux-appimage"
    if lower.endswith(".deb"):
        return "linux-deb"
    if lower.endswith(".rpm"):
        return "linux-rpm"
    if lower.endswith(".tar.gz"):
        return "updater-archive"
    if lower.endswith(".zip"):
        return "updater-archive"
    if lower.endswith(".sig"):
        return "updater-signature"
    return None


def directory_artifact(path: Path, root: Path) -> dict[str, Any]:
    entries: list[tuple[str, Path]] = []
    total = 0
    for current, directory_names, file_names in os.walk(path):
        current_path = Path(current)
        for name in directory_names:
            child = current_path / name
            if child.is_symlink():
                raise EvidenceError("bundle directories must not contain symlinks")
        for name in file_names:
            child = current_path / name
            regular_file(child, "bundle member")
            relative = child.relative_to(path).as_posix()
            entries.append((relative, child))
    entries.sort(key=lambda entry: entry[0].encode("utf-8"))
    if not entries:
        raise EvidenceError("application bundle directory is empty")
    digest = hashlib.sha256(TREE_CONTEXT)
    for relative, child in entries:
        encoded = relative.encode("utf-8")
        size = child.stat().st_size
        total += size
        digest.update(len(encoded).to_bytes(4, "big"))
        digest.update(encoded)
        digest.update(size.to_bytes(8, "big"))
        digest.update(bytes.fromhex(sha256_file(child)))
    return {
        "path": path.relative_to(root).as_posix(),
        "kind": "macos-app-bundle",
        "bytes": total,
        "sha256": digest.hexdigest(),
        "tree_entries": len(entries),
    }


def discover_artifacts(root: Path) -> list[dict[str, Any]]:
    if root.is_symlink() or not root.is_dir():
        raise EvidenceError("bundle root must be a regular non-symlink directory")
    artifacts: list[dict[str, Any]] = []
    app_roots: list[Path] = []
    for current, directory_names, file_names in os.walk(root):
        current_path = Path(current)
        kept_directories: list[str] = []
        for name in directory_names:
            child = current_path / name
            if child.is_symlink():
                raise EvidenceError("bundle tree must not contain symlinks")
            if name.lower().endswith(".app"):
                app_roots.append(child)
            else:
                kept_directories.append(name)
        directory_names[:] = kept_directories
        for name in file_names:
            child = current_path / name
            regular_file(child, "bundle artifact")
            kind = artifact_kind(child)
            if kind is None:
                continue
            artifacts.append(
                {
                    "path": child.relative_to(root).as_posix(),
                    "kind": kind,
                    "bytes": child.stat().st_size,
                    "sha256": sha256_file(child),
                }
            )
    artifacts.extend(directory_artifact(path, root) for path in app_roots)
    artifacts.sort(key=lambda entry: entry["path"].encode("utf-8"))
    if not artifacts:
        raise EvidenceError("bundle root contains no recognised release artifacts")
    if len(artifacts) > MAX_ARTIFACTS:
        raise EvidenceError("bundle artifact count exceeds policy")
    return artifacts


def require_platform_artifact(platform: str, artifacts: list[dict[str, Any]]) -> None:
    expected = {
        "windows": {"windows-msi", "windows-nsis"},
        "macos": {"macos-dmg", "macos-app-bundle"},
        "linux": {"linux-appimage"},
    }[platform]
    if not any(entry["kind"] in expected for entry in artifacts):
        raise EvidenceError(f"bundle does not contain a recognised {platform} artifact")


def supply_chain_record(path: Path, label: str, root: Path) -> dict[str, Any]:
    regular_file(path, label)
    try:
        relative = path.relative_to(root).as_posix()
    except ValueError:
        relative = path.name
    return {
        "path": relative,
        "bytes": path.stat().st_size,
        "sha256": sha256_file(path),
    }


def build_evidence(
    *,
    bundle_root: Path,
    platform: str,
    target: str,
    source_revision: str,
    tauri_config: Path,
    sbom: Path,
    licenses: Path,
    development_unsigned: bool,
    source_dirty: bool = False,
) -> dict[str, Any]:
    if not development_unsigned:
        raise EvidenceError(
            "signed release evidence requires platform-specific signature verification"
        )
    if not REVISION_RE.fullmatch(source_revision):
        raise EvidenceError("source revision must be a lowercase 40- or 64-digit hex digest")
    if not target or len(target) > 128 or not re.fullmatch(r"[a-z0-9_.-]+", target):
        raise EvidenceError("target triple is invalid")
    regular_file(tauri_config, "Tauri configuration")
    configuration = json.loads(tauri_config.read_text(encoding="utf-8"))
    artifacts = discover_artifacts(bundle_root)
    require_platform_artifact(platform, artifacts)
    return {
        "format": FORMAT,
        "host": {
            "identifier": configuration["identifier"],
            "product_name": configuration["productName"],
            "version": configuration["version"],
        },
        "build": {
            "platform": platform,
            "target": target,
            "source_revision": source_revision,
            "source_dirty": source_dirty,
            "development_unsigned": True,
            "platform_signing": "not_claimed",
        },
        "supply_chain": {
            "cyclonedx_sbom": supply_chain_record(sbom, "CycloneDX SBOM", NATIVE_ROOT),
            "third_party_licenses": supply_chain_record(
                licenses, "third-party license inventory", NATIVE_ROOT
            ),
        },
        "artifacts": artifacts,
    }


def encoded_evidence(evidence: dict[str, Any]) -> bytes:
    return (json.dumps(evidence, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", choices=("windows", "macos", "linux"), required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument(
        "--source-dirty",
        action="store_true",
        help="record that the bundle was built from an uncommitted working tree",
    )
    parser.add_argument("--development-unsigned", action="store_true")
    parser.add_argument(
        "--tauri-config",
        type=Path,
        default=NATIVE_ROOT / "desktop/src-tauri/tauri.conf.json",
    )
    parser.add_argument("--sbom", type=Path, default=NATIVE_ROOT / "sbom.cdx.json")
    parser.add_argument(
        "--licenses", type=Path, default=NATIVE_ROOT / "THIRD_PARTY_LICENSES.md"
    )
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        evidence = build_evidence(
            bundle_root=args.bundle_root.resolve(strict=True),
            platform=args.platform,
            target=args.target,
            source_revision=args.source_revision,
            tauri_config=args.tauri_config.resolve(strict=True),
            sbom=args.sbom.resolve(strict=True),
            licenses=args.licenses.resolve(strict=True),
            development_unsigned=args.development_unsigned,
            source_dirty=args.source_dirty,
        )
        encoded = encoded_evidence(evidence)
        if args.check:
            if not args.output.is_file() or args.output.read_bytes() != encoded:
                raise EvidenceError("release evidence is missing or stale")
        else:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with args.output.open("xb") as output:
                output.write(encoded)
        print(
            json.dumps(
                {
                    "ok": True,
                    "output": str(args.output.resolve()),
                    "artifacts": len(evidence["artifacts"]),
                    "development_unsigned": True,
                },
                indent=2,
            )
        )
        return 0
    except (EvidenceError, OSError, KeyError, json.JSONDecodeError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, indent=2), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

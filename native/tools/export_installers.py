#!/usr/bin/env python3
"""Export selected Windows host installers to the capsules directory.

Tauri owns the versioned files below its target bundle tree. This script keeps
that internal layout out of the user-facing workflow by copying exactly one
NSIS setup executable and one MSI package to stable names next to the capsule
artifacts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import sys
import tempfile
from pathlib import Path
from typing import NamedTuple


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_TARGET = "x86_64-pc-windows-msvc"
DEFAULT_BUNDLE_ROOT = (
    REPOSITORY_ROOT / "native/target" / DEFAULT_TARGET / "release/bundle"
)
DEFAULT_OUTPUT_DIRECTORY = REPOSITORY_ROOT / "capsules"


class InstallerSpec(NamedTuple):
    kind: str
    directory: str
    pattern: str
    output_name: str


INSTALLERS = (
    InstallerSpec(
        kind="windows-nsis",
        directory="nsis",
        pattern="*-setup.exe",
        output_name="sqlite-capsule-host-setup.exe",
    ),
    InstallerSpec(
        kind="windows-msi",
        directory="msi",
        pattern="*.msi",
        output_name="sqlite-capsule-host.msi",
    ),
)
DEFAULT_BUNDLES = ("nsis",)


class ExportError(ValueError):
    pass


def require_regular_file(path: Path, label: str) -> None:
    metadata = path.lstat()
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise ExportError(f"{label} must be a regular non-symlink file: {path}")


def require_directory(path: Path, label: str) -> None:
    if path.is_symlink() or not path.is_dir():
        raise ExportError(f"{label} must be a regular non-symlink directory: {path}")


def select_installers(bundles: tuple[str, ...]) -> tuple[InstallerSpec, ...]:
    requested = set(bundles)
    supported = {spec.directory for spec in INSTALLERS}
    unknown = requested - supported
    if unknown:
        raise ExportError(f"unsupported installer bundle: {sorted(unknown)[0]}")
    return tuple(spec for spec in INSTALLERS if spec.directory in requested)


def discover_installers(
    bundle_root: Path, bundles: tuple[str, ...] = DEFAULT_BUNDLES
) -> list[tuple[InstallerSpec, Path]]:
    require_directory(bundle_root, "bundle root")
    discovered: list[tuple[InstallerSpec, Path]] = []
    for spec in select_installers(bundles):
        source_directory = bundle_root / spec.directory
        require_directory(source_directory, f"{spec.kind} bundle directory")
        candidates = sorted(
            source_directory.glob(spec.pattern), key=lambda path: path.name.casefold()
        )
        if len(candidates) != 1:
            raise ExportError(
                f"expected exactly one {spec.kind} installer matching "
                f"{spec.directory}/{spec.pattern}, found {len(candidates)}"
            )
        require_regular_file(candidates[0], f"{spec.kind} installer")
        discovered.append((spec, candidates[0]))
    return discovered


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_copy(source: Path, destination: Path) -> None:
    if destination.exists() or destination.is_symlink():
        require_regular_file(destination, "existing exported installer")
    temporary_name: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb",
            prefix=f".{destination.name}.",
            suffix=".tmp",
            dir=destination.parent,
            delete=False,
        ) as temporary:
            temporary_name = temporary.name
            with source.open("rb") as installer:
                shutil.copyfileobj(installer, temporary, length=1024 * 1024)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_name, destination)
        temporary_name = None
    finally:
        if temporary_name is not None:
            Path(temporary_name).unlink(missing_ok=True)


def export_installers(
    bundle_root: Path,
    output_directory: Path,
    bundles: tuple[str, ...] = DEFAULT_BUNDLES,
) -> list[dict[str, object]]:
    discovered = discover_installers(bundle_root, bundles)
    output_directory.mkdir(parents=True, exist_ok=True)
    require_directory(output_directory, "output directory")

    destinations: list[tuple[InstallerSpec, Path, Path]] = []
    for spec, source in discovered:
        destination = output_directory / spec.output_name
        if destination.exists() or destination.is_symlink():
            require_regular_file(destination, "existing exported installer")
        destinations.append((spec, source, destination))

    exported: list[dict[str, object]] = []
    for spec, source, destination in destinations:
        atomic_copy(source, destination)
        exported.append(
            {
                "kind": spec.kind,
                "source": str(source.resolve()),
                "output": str(destination.resolve()),
                "bytes": destination.stat().st_size,
                "sha256": sha256_file(destination),
            }
        )
    return exported


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle-root", type=Path, default=DEFAULT_BUNDLE_ROOT)
    parser.add_argument("--output-directory", type=Path, default=DEFAULT_OUTPUT_DIRECTORY)
    parser.add_argument(
        "--bundles",
        default=",".join(DEFAULT_BUNDLES),
        help="comma-separated bundle types to export (default: nsis)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    bundles = tuple(part.strip() for part in args.bundles.split(",") if part.strip())
    if not bundles:
        print(
            json.dumps({"ok": False, "error": "at least one bundle is required"}),
            file=sys.stderr,
        )
        return 2
    try:
        exported = export_installers(
            args.bundle_root.resolve(strict=True),
            args.output_directory.resolve(),
            bundles,
        )
        print(
            json.dumps(
                {
                    "ok": True,
                    "development_unsigned": True,
                    "installers": exported,
                },
                indent=2,
            )
        )
        return 0
    except (ExportError, OSError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, indent=2), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

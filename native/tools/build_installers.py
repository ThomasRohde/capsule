#!/usr/bin/env python3
"""Build and export the pinned Windows NSIS installer (and optional MSI).

The Tauri CLI is a separate Cargo executable rather than a workspace
dependency. This wrapper removes reliance on an ambient ``cargo tauri``
installation by accepting only the pinned CLI version and bootstrapping it
into an ignored repository-local tool directory when necessary.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
NATIVE_ROOT = REPOSITORY_ROOT / "native"
TAURI_CONFIG = NATIVE_ROOT / "desktop" / "src-tauri" / "tauri.conf.json"
EXPORT_SCRIPT = NATIVE_ROOT / "tools" / "export_installers.py"
PINNED_TAURI_CLI_VERSION = "2.11.4"
DEFAULT_TARGET = "x86_64-pc-windows-msvc"
DEFAULT_BUNDLES = "nsis"
TOOL_ROOT = NATIVE_ROOT / ".tools" / f"tauri-cli-{PINNED_TAURI_CLI_VERSION}"
BUNDLE_PATTERNS = {"msi": "*.msi", "nsis": "*-setup.exe"}
VERSION_PATTERN = re.compile(r"^tauri-cli\s+(\d+\.\d+\.\d+)$")


def local_cli_path() -> Path:
    executable = "cargo-tauri.exe" if os.name == "nt" else "cargo-tauri"
    return TOOL_ROOT / "bin" / executable


def cli_version(executable: Path) -> str | None:
    try:
        result = subprocess.run(
            [str(executable), "--version"],
            cwd=NATIVE_ROOT,
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None
    match = VERSION_PATTERN.fullmatch(result.stdout.strip())
    return match.group(1) if match else None


def find_pinned_cli() -> Path | None:
    candidates = [local_cli_path()]
    ambient = shutil.which("cargo-tauri")
    if ambient:
        candidates.append(Path(ambient))
    for candidate in candidates:
        if candidate.is_file() and cli_version(candidate) == PINNED_TAURI_CLI_VERSION:
            return candidate.resolve()
    return None


def ensure_pinned_cli() -> Path:
    existing = find_pinned_cli()
    if existing is not None:
        return existing
    TOOL_ROOT.mkdir(parents=True, exist_ok=True)
    command = [
        "cargo",
        "install",
        "tauri-cli",
        "--version",
        f"={PINNED_TAURI_CLI_VERSION}",
        "--locked",
        "--root",
        str(TOOL_ROOT),
    ]
    print(
        f"Pinned tauri-cli {PINNED_TAURI_CLI_VERSION} is missing; "
        f"bootstrapping it into {TOOL_ROOT}",
        flush=True,
    )
    try:
        subprocess.run(command, cwd=NATIVE_ROOT, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise RuntimeError(
            "Unable to bootstrap the pinned Tauri CLI. The first build requires "
            "Cargo registry access; rerun this command with network permission."
        ) from error
    installed = local_cli_path()
    if cli_version(installed) != PINNED_TAURI_CLI_VERSION:
        raise RuntimeError(
            f"Bootstrapped Tauri CLI does not report {PINNED_TAURI_CLI_VERSION}"
        )
    return installed.resolve()


def bundle_root(target: str) -> Path:
    return NATIVE_ROOT / "target" / target / "release" / "bundle"


def clean_generated_installers(root: Path, bundles: tuple[str, ...]) -> list[Path]:
    removed: list[Path] = []
    resolved_root = root.resolve()
    for bundle in bundles:
        pattern = BUNDLE_PATTERNS.get(bundle)
        if pattern is None:
            raise ValueError(f"unsupported installer bundle: {bundle}")
        directory = root / bundle
        if not directory.exists():
            continue
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"bundle directory must be a regular directory: {directory}")
        for candidate in directory.glob(pattern):
            resolved = candidate.resolve()
            if resolved_root not in resolved.parents:
                raise ValueError(f"installer candidate escapes bundle root: {candidate}")
            metadata = candidate.lstat()
            if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
                raise ValueError(f"installer candidate must be a regular file: {candidate}")
            candidate.unlink()
            removed.append(candidate)
    return removed


def build_command(cli: Path, target: str, bundles: tuple[str, ...]) -> list[str]:
    return [
        str(cli),
        "build",
        "--config",
        str(TAURI_CONFIG),
        "--target",
        target,
        "--bundles",
        ",".join(bundles),
    ]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=DEFAULT_TARGET)
    parser.add_argument("--bundles", default=DEFAULT_BUNDLES)
    parser.add_argument(
        "--no-export",
        action="store_true",
        help="Build bundles without copying the stable installer outputs.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    bundles = tuple(part.strip() for part in args.bundles.split(",") if part.strip())
    if not bundles:
        print("at least one bundle is required", file=sys.stderr)
        return 2
    try:
        cli = ensure_pinned_cli()
        root = bundle_root(args.target)
        removed = clean_generated_installers(root, bundles)
        for path in removed:
            print(f"Removed stale generated installer: {path}")
        print(f"Using tauri-cli {PINNED_TAURI_CLI_VERSION}: {cli}", flush=True)
        subprocess.run(build_command(cli, args.target, bundles), cwd=NATIVE_ROOT, check=True)
        if not args.no_export:
            subprocess.run(
                [
                    sys.executable,
                    str(EXPORT_SCRIPT),
                    "--bundle-root",
                    str(root),
                    "--bundles",
                    ",".join(bundles),
                ],
                cwd=REPOSITORY_ROOT,
                check=True,
            )
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"installer build failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

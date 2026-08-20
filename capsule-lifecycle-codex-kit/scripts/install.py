#!/usr/bin/env python3
r"""Install the Capsule lifecycle programme overlay into a Capsule checkout.

The installer is deliberately conservative:
- it copies only files under this package's `overlay/` directory;
- it never deletes files;
- it accepts an existing destination only when its bytes are identical;
- it refuses symlinks and path escapes;
- `--check` performs a dry run.

Examples:

    python scripts/install.py --repo C:\\src\\capsule --check
    python scripts/install.py --repo C:\src\capsule
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import sys
from typing import Iterable


PACKAGE_ROOT = Path(__file__).resolve().parent.parent
OVERLAY = PACKAGE_ROOT / "overlay"
REQUIRED_REPO_PATHS = (
    "AGENTS.md",
    "CONTRIBUTING.md",
    "format",
    "native/Cargo.toml",
    "native/desktop",
    "plugins/capsule-creator",
)


class InstallError(RuntimeError):
    pass


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def overlay_files() -> Iterable[tuple[Path, Path]]:
    if not OVERLAY.is_dir():
        raise InstallError(f"missing overlay directory: {OVERLAY}")
    for source in sorted(OVERLAY.rglob("*")):
        if source.is_symlink():
            raise InstallError(f"overlay contains a symlink: {source}")
        if source.is_file():
            relative = source.relative_to(OVERLAY)
            if relative.is_absolute() or ".." in relative.parts:
                raise InstallError(f"unsafe overlay path: {relative}")
            if "__pycache__" in relative.parts or source.suffix == ".pyc":
                continue
            yield source, relative


def validate_repo(repo: Path) -> None:
    missing = [relative for relative in REQUIRED_REPO_PATHS if not (repo / relative).exists()]
    if missing:
        raise InstallError(
            f"{repo} does not look like the Capsule repository; missing: "
            + ", ".join(missing)
        )
    if not (repo / ".git").exists():
        print(
            "warning: .git is absent; continuing because required repository paths exist",
            file=sys.stderr,
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument(
        "--check",
        action="store_true",
        help="Show the install plan without changing the repository.",
    )
    args = parser.parse_args()

    repo = args.repo.expanduser().resolve()
    validate_repo(repo)

    actions: list[tuple[str, Path, Path]] = []
    conflicts: list[tuple[Path, Path]] = []

    for source, relative in overlay_files():
        destination = repo / relative
        try:
            destination.resolve(strict=False).relative_to(repo)
        except ValueError as exc:
            raise InstallError(f"destination escapes repository: {destination}") from exc

        if destination.is_symlink():
            conflicts.append((source, destination))
        elif destination.exists():
            if not destination.is_file() or sha256(source) != sha256(destination):
                conflicts.append((source, destination))
            else:
                actions.append(("identical", source, destination))
        else:
            actions.append(("create", source, destination))

    for action, source, destination in actions:
        relative = destination.relative_to(repo).as_posix()
        print(f"{action:9} {relative}")

    if conflicts:
        print("\nConflicts:", file=sys.stderr)
        for source, destination in conflicts:
            print(
                f"  {destination.relative_to(repo).as_posix()} already exists "
                "with different content or type",
                file=sys.stderr,
            )
        raise InstallError(
            "installation refused; move/reconcile conflicting files manually"
        )

    creates = [entry for entry in actions if entry[0] == "create"]
    if args.check:
        print(
            f"\ncheck complete: {len(creates)} files would be created; "
            f"{len(actions) - len(creates)} are already identical"
        )
        return 0

    for _, source, destination in creates:
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(destination.name + ".lifecycle-kit.tmp")
        shutil.copyfile(source, temporary)
        os.replace(temporary, destination)

    print(
        f"\ninstalled: {len(creates)} files created; "
        f"{len(actions) - len(creates)} already identical"
    )
    print("next: open the repository in Codex and read CODEX_LIFECYCLE_START.md")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except InstallError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

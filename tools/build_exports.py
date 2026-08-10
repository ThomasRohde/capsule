#!/usr/bin/env python3
"""Build or deterministically check the repository's HTML export matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.capsule_html import PROFILES, SQLITE_VERSION, export_html, inspect_html, verify_html


DEFAULT_CAPSULE = ROOT / "capsules" / "diagram-studio.capsule.sqlite"
DEFAULT_OUTPUT = ROOT / "exports"
MANIFEST_NAME = "manifest.json"


def _json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def _manifest(capsule: Path, output: Path) -> dict[str, object]:
    source_sha256 = hashlib.sha256(capsule.read_bytes()).hexdigest()
    files = []
    created_at = None
    for profile in PROFILES:
        path = output / f"diagram-studio-{profile}.html"
        report = inspect_html(path)
        verified = verify_html(path)
        metadata = report["metadata"]
        created_at = created_at or metadata["created_at"]
        if metadata["source"]["sha256"] != source_sha256:
            raise RuntimeError(f"{path.name} does not derive from the requested capsule")
        files.append(
            {
                "path": path.name,
                "profile": profile,
                "bytes": report["bytes"],
                "sha256": report["sha256"],
                "database_sha256": metadata["current"]["database_sha256"],
                "revision": metadata["current"]["revision"],
                "verified": verified["ok"],
                "capsule_checks": len(verified["capsule"]["checks"]),
                "components": {
                    "database_uncompressed_bytes": metadata["current"]["uncompressed_bytes"],
                    "database_compressed_bytes": metadata["current"]["compressed_bytes"],
                    "loader_bytes": metadata["runtime"]["loader_bytes"],
                    "sqlite_js_bytes": metadata["runtime"]["sqlite_js_bytes"],
                    "sqlite_wasm_bytes": metadata["runtime"]["sqlite_wasm_bytes"],
                    "worker_bytes": metadata["runtime"]["worker_bytes"],
                    "notices_bytes": metadata["runtime"]["notices_bytes"],
                },
            }
        )
    return {
        "contract": "org.sqlite-capsule.html-export-release/0.2",
        "created_at": created_at,
        "source": str(capsule.resolve().relative_to(ROOT)).replace("\\", "/"),
        "source_sha256": source_sha256,
        "sqlite_wasm_version": SQLITE_VERSION,
        "files": files,
    }


def build_exports(capsule: Path = DEFAULT_CAPSULE, output: Path = DEFAULT_OUTPUT, *, check: bool = False) -> dict[str, object]:
    capsule = capsule.resolve()
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for profile in PROFILES:
        export_html(
            capsule,
            output / f"diagram-studio-{profile}.html",
            profile=profile,
            replace=not check,
            check=check,
        )
    manifest = _manifest(capsule, output)
    rendered = _json_bytes(manifest)
    manifest_path = output / MANIFEST_NAME
    if check:
        if not manifest_path.is_file() or manifest_path.read_bytes() != rendered:
            raise RuntimeError("HTML export release manifest is missing or stale")
    else:
        fd, name = tempfile.mkstemp(prefix=f".{MANIFEST_NAME}.", suffix=".tmp", dir=output)
        temporary = Path(name)
        try:
            with os.fdopen(fd, "wb") as stream:
                stream.write(rendered)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, manifest_path)
        finally:
            temporary.unlink(missing_ok=True)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capsule", type=Path, default=DEFAULT_CAPSULE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        result = build_exports(args.capsule, args.output, check=args.check)
    except (OSError, RuntimeError, ValueError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2, sort_keys=True))
        return 2
    print(json.dumps({"ok": True, **result}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

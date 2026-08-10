from __future__ import annotations

import hashlib
import json
import shutil
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase, MAX_ASSET_BYTES, MAX_CAPSULE_BYTES  # noqa: E402
from tools.capsule_html import export_html  # noqa: E402


def zero_sha256(size: int) -> str:
    digest = hashlib.sha256()
    block = b"\x00" * (1024 * 1024)
    remaining = size
    while remaining:
        chunk = block[: min(remaining, len(block))]
        digest.update(chunk)
        remaining -= len(chunk)
    return digest.hexdigest()


def insert_zero_asset(connection: sqlite3.Connection, index: int, size: int) -> None:
    if not 0 < size <= MAX_ASSET_BYTES:
        raise ValueError(f"Limit fixture asset size is invalid: {size}")
    connection.execute(
        "INSERT INTO capsule_asset "
        "(path, media_type, content, sha256, executable, cache_policy, description) "
        "VALUES (?, 'application/octet-stream', zeroblob(?), ?, 0, 'no-store', ?)",
        (
            f"limit/payload-{index}.bin",
            size,
            zero_sha256(size),
            "Deterministic zero-filled payload for the HTML export supported-limit lane.",
        ),
    )


def main() -> None:
    source = Path(sys.argv[1]).resolve(strict=True)
    capsule = Path(sys.argv[2]).resolve()
    output = Path(sys.argv[3]).resolve()
    capsule.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, capsule)
    target = MAX_CAPSULE_BYTES - (384 * 1024)
    connection = sqlite3.connect(capsule)
    try:
        connection.execute("PRAGMA journal_mode = DELETE")
        for index in range(4):
            insert_zero_asset(connection, index, 15 * 1024 * 1024)
        connection.commit()
        remaining = target - capsule.stat().st_size - (128 * 1024)
        if remaining > 0:
            insert_zero_asset(connection, 4, min(remaining, MAX_ASSET_BYTES))
        connection.commit()
    finally:
        connection.close()
    size = capsule.stat().st_size
    if not MAX_CAPSULE_BYTES - (2 * 1024 * 1024) <= size <= MAX_CAPSULE_BYTES:
        raise RuntimeError(f"Limit fixture is not near the supported boundary: {size} bytes")
    with CapsuleDatabase(capsule, read_only=True) as database:
        verification = database.verify()
    if not verification["ok"]:
        raise RuntimeError("Limit fixture is invalid: " + "; ".join(verification["errors"]))
    export = export_html(capsule, output, profile="editable", replace=True)
    print(json.dumps({"ok": True, "capsule_bytes": size, "export": export}, indent=2))


if __name__ == "__main__":
    main()

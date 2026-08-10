#!/usr/bin/env python3
"""Inspect capsule signature metadata without treating it as authentication.

Cryptographic verification is delegated to the independently built native CLI
when it is installed or supplied explicitly. Merely reading a capsule-provided
publisher name, key, or signature never establishes trust.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sqlite3
import subprocess
from contextlib import closing
from datetime import datetime
from pathlib import Path
from typing import Any


PROFILE = "org.sqlite-capsule.signed-app/0.2"
ALGORITHM = "ed25519"
MAX_SIGNATURE_ROWS = 1024

PUBLISHER_COLUMNS = (
    ("id", "INTEGER", False, 1),
    ("profile", "TEXT", True, 0),
    ("publisher_id", "TEXT", True, 0),
    ("publisher_name", "TEXT", True, 0),
)
SIGNATURE_COLUMNS = (
    ("key_id", "TEXT", True, 1),
    ("algorithm", "TEXT", True, 0),
    ("public_key", "BLOB", True, 0),
    ("application_digest", "BLOB", True, 0),
    ("signature", "BLOB", True, 0),
    ("signed_at", "TEXT", True, 0),
)


def _open_immutable(path: Path) -> sqlite3.Connection:
    uri = path.resolve().as_uri() + "?mode=ro&immutable=1"
    connection = sqlite3.connect(uri, uri=True)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA trusted_schema=OFF")
    connection.execute("PRAGMA query_only=ON")
    connection.execute("PRAGMA foreign_keys=ON")
    return connection


def _table_exists(connection: sqlite3.Connection, name: str) -> bool:
    return (
        connection.execute(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?",
            (name,),
        ).fetchone()
        is not None
    )


def _valid_table_shape(
    connection: sqlite3.Connection,
    table: str,
    expected: tuple[tuple[str, str, bool, int], ...],
) -> bool:
    rows = connection.execute(f'PRAGMA table_xinfo("{table}")').fetchall()
    actual = tuple(
        (
            str(row["name"]),
            str(row["type"]).upper(),
            bool(row["notnull"]),
            int(row["pk"]),
            int(row["hidden"]),
        )
        for row in rows
    )
    wanted = tuple((*column, 0) for column in expected)
    return actual == wanted


def _valid_signed_at(value: str) -> bool:
    if len(value) != 20 or not value.endswith("Z"):
        return False
    try:
        parsed = datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return parsed.strftime("%Y-%m-%dT%H:%M:%SZ") == value


def _native_verifier_path(explicit: Path | None) -> Path | None:
    if explicit is not None:
        candidate = explicit.expanduser().resolve()
        if not candidate.is_file():
            raise FileNotFoundError(f"native verifier not found: {candidate}")
        return candidate
    installed = shutil.which("capsule-native")
    return Path(installed).resolve() if installed else None


def _invoke_native_verifier(executable: Path, capsule: Path) -> dict[str, Any]:
    completed = subprocess.run(
        [str(executable), "verify-signature", str(capsule.resolve())],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    channel = completed.stdout if completed.returncode == 0 else completed.stderr
    try:
        payload = json.loads(channel)
    except json.JSONDecodeError as exc:
        raise RuntimeError("native verifier returned invalid JSON") from exc
    if completed.returncode != 0 or payload.get("ok") is not True:
        detail = payload.get("error", "native verifier rejected the capsule")
        raise RuntimeError(str(detail))
    return payload


def signature_inventory(
    capsule: Path,
    *,
    native_verifier: Path | None = None,
) -> dict[str, Any]:
    capsule = capsule.expanduser().resolve()
    errors: list[str] = []
    signatures: list[dict[str, Any]] = []
    publisher: dict[str, str] | None = None
    integrity_ok = False
    extension_present = False
    extension_valid = False

    with closing(_open_immutable(capsule)) as connection:
        integrity = [str(row[0]) for row in connection.execute("PRAGMA integrity_check")]
        integrity_ok = integrity == ["ok"]
        if not integrity_ok:
            errors.append("SQLite integrity_check failed: " + "; ".join(integrity[:10]))

        publisher_present = _table_exists(connection, "capsule_publisher")
        signature_present = _table_exists(connection, "capsule_signature")
        extension_present = publisher_present or signature_present
        if publisher_present != signature_present:
            errors.append("signed-app extension is partial")
        elif publisher_present:
            if not _valid_table_shape(connection, "capsule_publisher", PUBLISHER_COLUMNS):
                errors.append("capsule_publisher has an incompatible shape")
            if not _valid_table_shape(connection, "capsule_signature", SIGNATURE_COLUMNS):
                errors.append("capsule_signature has an incompatible shape")

            if not errors:
                publisher_rows = connection.execute(
                    "SELECT id, profile, publisher_id, publisher_name FROM capsule_publisher"
                ).fetchall()
                if len(publisher_rows) != 1:
                    errors.append("capsule_publisher must contain exactly one row")
                else:
                    row = publisher_rows[0]
                    if (
                        row["id"] != 1
                        or row["profile"] != PROFILE
                        or not isinstance(row["publisher_id"], str)
                        or not 1 <= len(row["publisher_id"]) <= 512
                        or not isinstance(row["publisher_name"], str)
                        or not 1 <= len(row["publisher_name"]) <= 512
                    ):
                        errors.append("capsule_publisher row is malformed")
                    else:
                        publisher = {
                            "id": row["publisher_id"],
                            "name": row["publisher_name"],
                        }

                rows = connection.execute(
                    "SELECT key_id, algorithm, public_key, application_digest, "
                    "signature, signed_at FROM capsule_signature "
                    "ORDER BY key_id COLLATE BINARY LIMIT ?",
                    (MAX_SIGNATURE_ROWS + 1,),
                ).fetchall()
                if len(rows) > MAX_SIGNATURE_ROWS:
                    errors.append(f"signature inventory exceeds {MAX_SIGNATURE_ROWS} rows")
                else:
                    for row in rows:
                        public_key = row["public_key"]
                        digest = row["application_digest"]
                        signature = row["signature"]
                        key_id = row["key_id"]
                        expected_key_id = (
                            "ed25519:sha256:" + hashlib.sha256(public_key).hexdigest()
                            if isinstance(public_key, bytes) and len(public_key) == 32
                            else None
                        )
                        valid_shape = (
                            isinstance(key_id, str)
                            and key_id == expected_key_id
                            and row["algorithm"] == ALGORITHM
                            and isinstance(digest, bytes)
                            and len(digest) == 32
                            and isinstance(signature, bytes)
                            and len(signature) == 64
                            and isinstance(row["signed_at"], str)
                            and _valid_signed_at(row["signed_at"])
                        )
                        if not valid_shape:
                            errors.append(f"malformed signature row: {key_id!r}")
                            continue
                        signatures.append(
                            {
                                "key_id": key_id,
                                "algorithm": ALGORITHM,
                                "public_key_hex": public_key.hex(),
                                "application_digest_sha256": digest.hex(),
                                "signed_at": row["signed_at"],
                                "cryptographically_valid": None,
                                "digest_matches": None,
                            }
                        )
            extension_valid = not errors

    report: dict[str, Any] = {
        "ok": integrity_ok and (not extension_present or extension_valid),
        "integrity_ok": integrity_ok,
        "signature_extension_present": extension_present,
        "signature_inventory_valid": extension_valid if extension_present else None,
        "signature_valid": None,
        "publisher_known": False,
        "publisher_trusted": False,
        "revocation_status": "not_checked",
        "executable_allowed": False,
        "publisher": publisher,
        "signatures": signatures,
        "errors": errors,
        "native_verifier": {"invoked": False, "path": None},
        "note": "Inventory alone does not authenticate a publisher or permit execution.",
    }

    verifier = _native_verifier_path(native_verifier)
    if verifier is not None and report["ok"]:
        native = _invoke_native_verifier(verifier, capsule)
        report["native_verifier"] = {"invoked": True, "path": str(verifier)}
        for field in (
            "signature_valid",
            "publisher_known",
            "publisher_trusted",
            "revocation_status",
            "executable_allowed",
            "application_digest_sha256",
        ):
            if field in native:
                report[field] = native[field]
        native_signatures = {
            item.get("key_id"): item for item in native.get("signatures", [])
        }
        for item in signatures:
            verified = native_signatures.get(item["key_id"], {})
            item["cryptographically_valid"] = verified.get("cryptographically_valid")
            item["digest_matches"] = verified.get("digest_matches")
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Inventory publisher signatures without inferring publisher trust"
    )
    parser.add_argument("capsule", type=Path)
    parser.add_argument(
        "--native-verifier",
        type=Path,
        help="explicit capsule-native executable for cryptographic verification",
    )
    arguments = parser.parse_args(argv)
    try:
        report = signature_inventory(
            arguments.capsule,
            native_verifier=arguments.native_verifier,
        )
    except (FileNotFoundError, OSError, RuntimeError, sqlite3.DatabaseError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2, sort_keys=True))
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

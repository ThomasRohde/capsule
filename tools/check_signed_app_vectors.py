#!/usr/bin/env python3
"""Independent standard-library checker for signed-app canonical vectors.

This implementation intentionally shares fixture SQL, but no digest code, with
the Rust implementation. The test-vector JSON values contain only integral
JSON numbers; production handling of all RFC 8785 numbers remains covered by
the pinned Rust JCS implementation and its upstream RFC corpus.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sqlite3
import struct
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
VECTOR_FILE = ROOT / "compatibility" / "signed-app-v0.2" / "vectors.json"
SIGNED_SCHEMA = ROOT / "format" / "capsule-signed-app-v0.2.sql"
STREAM_CONTEXT = b"SQLite Capsule signed-app canonical stream v1\0"
MAX_STREAM_BYTES = 512 * 1024 * 1024
MAX_JSON_BYTES = 1024 * 1024

SIGNED_TABLES = (
    "capsule_manifest",
    "capsule_asset",
    "capsule_command",
    "capsule_runbook",
    "capsule_doc",
    "capsule_endpoint",
    "capsule_endpoint_step",
    "capsule_check",
    "capsule_prompt",
    "capsule_publisher",
)
JSON_COLUMNS = {
    ("capsule_manifest", "permissions_json"),
    ("capsule_command", "argv_json"),
    ("capsule_endpoint", "parameters_json"),
    ("capsule_check", "expected_json"),
}


class VectorError(ValueError):
    pass


class CanonicalWriter:
    def __init__(self) -> None:
        self.output = bytearray()

    def raw(self, value: bytes) -> None:
        if len(self.output) + len(value) > MAX_STREAM_BYTES:
            raise VectorError("canonical stream limit exceeded")
        self.output.extend(value)

    def byte(self, value: int) -> None:
        self.raw(bytes((value,)))

    def sized(self, value: bytes) -> None:
        self.raw(struct.pack(">Q", len(value)))
        self.raw(value)

    def start_record(self, tag: int, name: str, field_count: int) -> None:
        self.byte(tag)
        self.sized(name.encode("utf-8"))
        self.raw(struct.pack(">I", field_count))

    def field(self, name: str, tag: int, value: bytes) -> None:
        self.sized(name.encode("utf-8"))
        self.byte(tag)
        self.sized(value)


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise VectorError(f"duplicate JSON object key: {key!r}")
        result[key] = value
    return result


def _utf16_sort_key(value: str) -> bytes:
    return value.encode("utf-16-be")


def _jcs_subset(value: Any) -> str:
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        raise VectorError("compatibility vectors do not use non-integral JSON numbers")
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    if isinstance(value, list):
        return "[" + ",".join(_jcs_subset(item) for item in value) + "]"
    if isinstance(value, dict):
        members = (
            _jcs_subset(key) + ":" + _jcs_subset(value[key])
            for key in sorted(value, key=_utf16_sort_key)
        )
        return "{" + ",".join(members) + "}"
    raise VectorError(f"unsupported JSON value: {type(value).__name__}")


def canonical_json(value: str) -> bytes:
    encoded = value.encode("utf-8")
    if len(encoded) > MAX_JSON_BYTES:
        raise VectorError("canonical JSON limit exceeded")
    try:
        decoded = json.loads(value, object_pairs_hook=_unique_object)
    except (json.JSONDecodeError, VectorError) as exc:
        raise VectorError("invalid or duplicated JSON") from exc
    return _jcs_subset(decoded).encode("utf-8")


def _identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def _sqlite_value(table: str, column: str, value: Any) -> tuple[int, bytes]:
    if value is None:
        return 0, b""
    if isinstance(value, int):
        if not -(2**63) <= value < 2**63:
            raise VectorError("SQLite integer outside signed 64-bit range")
        return 1, struct.pack(">q", value)
    if isinstance(value, float):
        if not math.isfinite(value):
            raise VectorError("non-finite SQLite real")
        return 2, struct.pack(">d", value)
    if isinstance(value, str):
        if (table, column) in JSON_COLUMNS:
            return 5, canonical_json(value)
        return 3, value.encode("utf-8")
    if isinstance(value, bytes):
        return 4, value
    raise VectorError(f"unsupported SQLite value: {type(value).__name__}")


def canonical_stream(connection: sqlite3.Connection) -> bytes:
    user_version = int(connection.execute("PRAGMA user_version").fetchone()[0])
    if user_version != 2:
        raise VectorError(f"unsupported user_version: {user_version}")
    signed_tables = SIGNED_TABLES

    publisher = connection.execute(
        "SELECT profile, publisher_id, publisher_name FROM capsule_publisher WHERE id = 1"
    ).fetchall()
    if (
        len(publisher) != 1
        or publisher[0][0] != "org.sqlite-capsule.signed-app/0.2"
        or not publisher[0][1]
        or not publisher[0][2]
    ):
        raise VectorError("malformed publisher identity")

    allowed = set(signed_tables) | {
        "capsule_grant",
        "capsule_change_log",
        "capsule_signature",
    }
    platform_tables = {
        str(row[0])
        for row in connection.execute(
            "SELECT name FROM sqlite_schema "
            "WHERE type = 'table' AND name GLOB 'capsule_*'"
        )
    }
    unexpected = sorted(platform_tables - allowed)
    if unexpected:
        raise VectorError(f"unexpected platform tables: {unexpected!r}")

    writer = CanonicalWriter()
    writer.raw(STREAM_CONTEXT)
    for object_type, name, table_name, sql in connection.execute(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema "
        "WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND sql IS NOT NULL "
        "ORDER BY type COLLATE BINARY, name COLLATE BINARY, tbl_name COLLATE BINARY"
    ):
        writer.start_record(1, f"schema/{object_type}/{name}", 4)
        for field_name, field_value in (
            ("type", object_type),
            ("name", name),
            ("table", table_name),
            ("sql", sql),
        ):
            writer.field(field_name, 3, str(field_value).encode("utf-8"))

    for table in signed_tables:
        columns = connection.execute(f"PRAGMA table_info({_identifier(table)})").fetchall()
        if not columns:
            raise VectorError(f"missing signed table: {table}")
        primary_key = sorted(
            ((int(row[5]), str(row[1])) for row in columns if int(row[5]) > 0),
            key=lambda item: item[0],
        )
        if not primary_key:
            raise VectorError(f"signed table has no primary key: {table}")
        column_names = [str(row[1]) for row in columns]
        select_columns = ", ".join(_identifier(name) for name in column_names)
        order_columns = ", ".join(
            f"{_identifier(name)} COLLATE BINARY" for _, name in primary_key
        )
        rows = connection.execute(
            f"SELECT {select_columns} FROM {_identifier(table)} ORDER BY {order_columns}"
        )
        for row in rows:
            writer.start_record(2, f"row/{table}", len(column_names))
            for column, value in zip(column_names, row, strict=True):
                tag, encoded = _sqlite_value(table, column, value)
                writer.field(column, tag, encoded)
    return bytes(writer.output)


def build_fixture(vector: dict[str, Any]) -> sqlite3.Connection:
    connection = sqlite3.connect(":memory:")
    connection.executescript((ROOT / vector["schema"]).read_text(encoding="utf-8"))
    connection.executescript(SIGNED_SCHEMA.read_text(encoding="utf-8"))
    connection.executescript((ROOT / vector["data"]).read_text(encoding="utf-8"))
    return connection


def _digest(connection: sqlite3.Connection) -> tuple[str, int]:
    stream = canonical_stream(connection)
    return hashlib.sha256(stream).hexdigest(), len(stream)


def _mutated_digest(
    vector: dict[str, Any],
    mutation: Callable[[sqlite3.Connection], None],
) -> str:
    connection = build_fixture(vector)
    try:
        mutation(connection)
        return _digest(connection)[0]
    finally:
        connection.close()


def assert_mutation_contract(vector: dict[str, Any], baseline: str) -> None:
    preserved = {
        "domain row": "UPDATE vector_domain SET note = 'changed' WHERE id = 'domain'",
        "grant row": "UPDATE capsule_grant SET reason = 'changed' WHERE capability = 'database.read'",
        "change-log row": "UPDATE capsule_change_log SET changed_rows = 9",
        "JSON property order": (
            "UPDATE capsule_manifest SET permissions_json = "
            "'{\"😀\":\"astral\",\"database.read\":{\"required\":true},\"\":\"bmp\",\"é\":\"café\",\"z\":0}'"
        ),
    }
    for name, sql in preserved.items():
        actual = _mutated_digest(vector, lambda connection, sql=sql: connection.execute(sql))
        if actual != baseline:
            raise AssertionError(f"{vector['name']}: {name} unexpectedly changed digest")

    def add_signature(connection: sqlite3.Connection) -> None:
        connection.execute(
            "INSERT INTO capsule_signature VALUES (?, 'ed25519', ?, ?, ?, ?)",
            (
                "ed25519:sha256:" + "0" * 64,
                bytes(32),
                bytes(32),
                bytes(64),
                "2026-08-08T12:34:56Z",
            ),
        )

    if _mutated_digest(vector, add_signature) != baseline:
        raise AssertionError(f"{vector['name']}: signature row changed digest")

    invalidating = {
        "manifest text": "UPDATE capsule_manifest SET title = title || '!'",
        "permission request": (
            "UPDATE capsule_manifest SET permissions_json = "
            "'{\"database.read\":{\"required\":false}}'"
        ),
        "asset blob": "UPDATE capsule_asset SET content = X'01' WHERE path = 'app/é.bin'",
        "command": "UPDATE capsule_command SET purpose = purpose || '!'",
        "runbook": "UPDATE capsule_runbook SET body_md = body_md || '!'",
        "document": "UPDATE capsule_doc SET content = content || '!'",
        "endpoint": "UPDATE capsule_endpoint SET sql_text = sql_text || ' '",
        "check": "UPDATE capsule_check SET description = description || '!'",
        "prompt": "UPDATE capsule_prompt SET prompt_text = prompt_text || '!'",
        "publisher": "UPDATE capsule_publisher SET publisher_name = publisher_name || '!'",
        "schema": "CREATE INDEX vector_note_idx ON vector_domain(note)",
    }
    if vector["name"] == "format-v0.2":
        invalidating.update(
            {
                "endpoint step": (
                    "UPDATE capsule_endpoint_step SET required_changes = 2 "
                    "WHERE endpoint_name = 'vector.write' AND sequence = 2"
                ),
            }
        )
    for name, sql in invalidating.items():
        actual = _mutated_digest(vector, lambda connection, sql=sql: connection.execute(sql))
        if actual == baseline:
            raise AssertionError(f"{vector['name']}: {name} failed to invalidate digest")


def computed_vectors() -> dict[str, Any]:
    declared = json.loads(VECTOR_FILE.read_text(encoding="utf-8"))
    output = {"profile": declared["profile"], "fixtures": []}
    for vector in declared["fixtures"]:
        connection = build_fixture(vector)
        try:
            digest, size = _digest(connection)
        finally:
            connection.close()
        output["fixtures"].append(
            {
                **vector,
                "application_digest_sha256": digest,
                "canonical_stream_bytes": size,
            }
        )
    return output


def verify_vectors() -> dict[str, Any]:
    declared = json.loads(VECTOR_FILE.read_text(encoding="utf-8"))
    computed = computed_vectors()
    for expected, actual in zip(declared["fixtures"], computed["fixtures"], strict=True):
        if expected["application_digest_sha256"] != actual["application_digest_sha256"]:
            raise AssertionError(
                f"{expected['name']}: digest {actual['application_digest_sha256']} != "
                f"{expected['application_digest_sha256']}"
            )
        if expected["canonical_stream_bytes"] != actual["canonical_stream_bytes"]:
            raise AssertionError(
                f"{expected['name']}: stream size {actual['canonical_stream_bytes']} != "
                f"{expected['canonical_stream_bytes']}"
            )
        assert_mutation_contract(expected, actual["application_digest_sha256"])
    return computed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--emit",
        action="store_true",
        help="print computed vector values without comparing them",
    )
    arguments = parser.parse_args(argv)
    try:
        result = computed_vectors() if arguments.emit else verify_vectors()
    except (AssertionError, OSError, sqlite3.DatabaseError, VectorError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2, sort_keys=True))
        return 1
    print(json.dumps({"ok": True, **result}, indent=2, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

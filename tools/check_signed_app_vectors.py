#!/usr/bin/env python3
"""Independent standard-library checker for signed-app canonical vectors.

This implementation intentionally shares fixture SQL, but no digest code, with
the Rust implementation. It independently implements the versioned framing and
RFC 8785 JSON rules used by both signed-application profiles.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sqlite3
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
VECTOR_FILE = ROOT / "compatibility" / "signed-app-v0.2" / "vectors.json"
V03_VECTOR_FILE = ROOT / "compatibility" / "signed-app-v0.3" / "vectors.json"
MAX_STREAM_BYTES = 512 * 1024 * 1024
MAX_JSON_BYTES = 1024 * 1024


@dataclass(frozen=True)
class CanonicalProfile:
    user_version: int
    profile: str
    stream_context: bytes
    signed_schema: Path
    signed_tables: tuple[str, ...]
    json_columns: frozenset[tuple[str, str]]
    excluded_platform_tables: frozenset[str]
    format_id: str
    format_version: str
    runtime_protocol: str
    minimum_host_profile: str | None = None


COMMON_SIGNED_TABLES = (
    "capsule_manifest", "capsule_asset", "capsule_command", "capsule_runbook",
    "capsule_doc", "capsule_endpoint", "capsule_endpoint_step", "capsule_check",
    "capsule_prompt",
)
COMMON_JSON_COLUMNS = frozenset(
    {
        ("capsule_manifest", "permissions_json"),
        ("capsule_command", "argv_json"),
        ("capsule_endpoint", "parameters_json"),
        ("capsule_check", "expected_json"),
    }
)
PROFILES = {
    2: CanonicalProfile(
        user_version=2,
        profile="org.sqlite-capsule.signed-app/0.2",
        stream_context=b"SQLite Capsule signed-app canonical stream v1\0",
        signed_schema=ROOT / "format" / "capsule-signed-app-v0.2.sql",
        signed_tables=COMMON_SIGNED_TABLES + ("capsule_publisher",),
        json_columns=COMMON_JSON_COLUMNS,
        excluded_platform_tables=frozenset(
            {"capsule_grant", "capsule_change_log", "capsule_signature"}
        ),
        format_id="org.sqlite-capsule",
        format_version="0.2",
        runtime_protocol="capsule-http/0.2",
    ),
    3: CanonicalProfile(
        user_version=3,
        profile="org.sqlite-capsule.signed-app/0.3",
        stream_context=b"SQLite Capsule signed-app canonical stream v2\0",
        signed_schema=ROOT / "format" / "capsule-signed-app-v0.3.sql",
        signed_tables=(
            "capsule_manifest", "capsule_application", "capsule_asset",
            "capsule_command", "capsule_runbook", "capsule_doc",
            "capsule_endpoint", "capsule_endpoint_step", "capsule_check",
            "capsule_prompt", "capsule_dataset", "capsule_dataset_table",
            "capsule_dataset_dependency", "capsule_migration",
            "capsule_migration_step", "capsule_migration_check",
            "capsule_publisher",
        ),
        json_columns=COMMON_JSON_COLUMNS
        | frozenset(
            {
                ("capsule_dataset_table", "primary_key_json"),
                ("capsule_dataset_table", "ignored_columns_json"),
                ("capsule_dataset_table", "immutable_columns_json"),
                ("capsule_migration_step", "definition_json"),
                ("capsule_migration_check", "definition_json"),
            }
        ),
        excluded_platform_tables=frozenset(
            {
                "capsule_instance", "capsule_instance_asset",
                "capsule_lineage_event", "capsule_lineage_parent",
                "capsule_grant", "capsule_change_log", "capsule_signature",
            }
        ),
        format_id="org.sqlite-capsule",
        format_version="0.3",
        runtime_protocol="capsule-http/0.2",
        minimum_host_profile="org.sqlite-capsule.host-profile/0.3",
    ),
}


def _profile_for_connection(connection: sqlite3.Connection) -> CanonicalProfile:
    application_id = int(connection.execute("PRAGMA application_id").fetchone()[0])
    user_version = int(connection.execute("PRAGMA user_version").fetchone()[0])
    profile = PROFILES.get(user_version)
    if application_id != 1129337676 or profile is None:
        raise VectorError("unsupported capsule format identity")
    columns = "format_id, format_version, runtime_protocol"
    if profile.minimum_host_profile is not None:
        columns += ", minimum_host_profile"
    rows = connection.execute(
        f"SELECT {columns} FROM capsule_manifest WHERE id = 1"
    ).fetchall()
    total = int(connection.execute("SELECT count(*) FROM capsule_manifest").fetchone()[0])
    if total != 1 or len(rows) != 1:
        raise VectorError("capsule manifest must contain exactly one id=1 row")
    if tuple(rows[0][:3]) != (
        profile.format_id,
        profile.format_version,
        profile.runtime_protocol,
    ):
        raise VectorError("unsupported capsule format tuple")
    if profile.minimum_host_profile is not None and rows[0][3] != profile.minimum_host_profile:
        raise VectorError("unsupported capsule minimum_host_profile")
    return profile


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


def _jcs_number(value: float) -> str:
    if not math.isfinite(value):
        raise VectorError("non-finite JSON number")
    if value == 0:
        return "0"

    sign = "-" if value < 0 else ""
    rendered = repr(abs(value)).lower()
    if "e" in rendered:
        mantissa, exponent_text = rendered.split("e", 1)
        exponent = int(exponent_text)
    else:
        mantissa, exponent = rendered, 0
    if "." in mantissa:
        integer, fraction = mantissa.split(".", 1)
    else:
        integer, fraction = mantissa, ""
    raw_digits = integer + fraction
    leading_zeroes = len(raw_digits) - len(raw_digits.lstrip("0"))
    digits = raw_digits.lstrip("0")
    decimal_position = len(integer) + exponent - leading_zeroes
    digits = digits.rstrip("0") or "0"

    if 0 < decimal_position <= 21:
        if len(digits) <= decimal_position:
            number = digits + ("0" * (decimal_position - len(digits)))
        else:
            number = digits[:decimal_position] + "." + digits[decimal_position:]
    elif -6 < decimal_position <= 0:
        number = "0." + ("0" * -decimal_position) + digits
    else:
        fraction_digits = digits[1:]
        number = digits[0]
        if fraction_digits:
            number += "." + fraction_digits
        scientific_exponent = decimal_position - 1
        number += "e" + ("+" if scientific_exponent >= 0 else "") + str(scientific_exponent)
    return sign + number


def _jcs(value: Any) -> str:
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return _jcs_number(value)
    if isinstance(value, str):
        value.encode("utf-8")
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    if isinstance(value, list):
        return "[" + ",".join(_jcs(item) for item in value) + "]"
    if isinstance(value, dict):
        members = (
            _jcs(key) + ":" + _jcs(value[key])
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
        output = _jcs(decoded).encode("utf-8")
    except (json.JSONDecodeError, UnicodeEncodeError, VectorError) as exc:
        raise VectorError("invalid or duplicated JSON") from exc
    return output


def _identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def _sqlite_value(
    profile: CanonicalProfile,
    table: str,
    column: str,
    value: Any,
) -> tuple[int, bytes]:
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
        if (table, column) in profile.json_columns:
            return 5, canonical_json(value)
        return 3, value.encode("utf-8")
    if isinstance(value, bytes):
        return 4, value
    raise VectorError(f"unsupported SQLite value: {type(value).__name__}")


def canonical_stream(connection: sqlite3.Connection) -> bytes:
    profile = _profile_for_connection(connection)

    publisher = connection.execute(
        "SELECT profile, publisher_id, publisher_name FROM capsule_publisher WHERE id = 1"
    ).fetchall()
    if (
        len(publisher) != 1
        or publisher[0][0] != profile.profile
        or not publisher[0][1]
        or not publisher[0][2]
    ):
        raise VectorError("malformed publisher identity")

    allowed = set(profile.signed_tables) | set(profile.excluded_platform_tables)
    platform_objects = {
        str(row[0])
        for row in connection.execute(
            "SELECT name FROM sqlite_schema WHERE name GLOB 'capsule_*'"
        )
    }
    unexpected = sorted(platform_objects - allowed)
    if unexpected:
        raise VectorError(f"unexpected platform tables: {unexpected!r}")

    writer = CanonicalWriter()
    writer.raw(profile.stream_context)
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

    for table in profile.signed_tables:
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
                tag, encoded = _sqlite_value(profile, table, column, value)
                writer.field(column, tag, encoded)
    return bytes(writer.output)


def build_fixture(vector: dict[str, Any]) -> sqlite3.Connection:
    connection = sqlite3.connect(":memory:")
    connection.executescript((ROOT / vector["schema"]).read_text(encoding="utf-8"))
    user_version = int(connection.execute("PRAGMA user_version").fetchone()[0])
    profile = PROFILES.get(user_version)
    if profile is None:
        raise VectorError(f"unsupported fixture user_version: {user_version}")
    connection.executescript(profile.signed_schema.read_text(encoding="utf-8"))
    connection.executescript((ROOT / vector["data"]).read_text(encoding="utf-8"))
    _profile_for_connection(connection)
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
    is_v03 = vector["name"] == "format-v0.3"
    preserved = {
        "domain row": "UPDATE vector_domain SET note = 'changed' WHERE id = 'domain'",
        "grant row": "UPDATE capsule_grant SET reason = 'changed' WHERE capability = 'database.read'",
        "change-log row": "UPDATE capsule_change_log SET changed_rows = 9",
        "JSON property order": (
            "UPDATE capsule_manifest SET permissions_json = "
            + (
                "'{\"😀\":\"astral\",\"network\":{\"value\":\"none\"},"
                "\"database.write\":{\"required\":true},\"database.read\":{\"required\":true},"
                "\"\":\"bmp\",\"é\":\"café\",\"z\":0}'"
                if is_v03
                else "'{\"😀\":\"astral\",\"database.read\":{\"required\":true},"
                "\"\":\"bmp\",\"é\":\"café\",\"z\":0}'"
            )
        ),
    }
    if is_v03:
        preserved.update(
            {
                "instance title": "UPDATE capsule_instance SET title = title || '!' WHERE id = 1",
                "instance tags": "UPDATE capsule_instance SET tags_json = '[\"changed\"]' WHERE id = 1",
                "instance icon": (
                    "UPDATE capsule_instance_asset SET description = description || '!' "
                    "WHERE id = 'instance-icon'"
                ),
                "lineage row": (
                    "UPDATE capsule_lineage_event SET details_json = '{\"changed\":true}' "
                    "WHERE sequence = 1"
                ),
                "lineage parent row": (
                    "INSERT INTO capsule_lineage_parent VALUES ("
                    "'33333333-3333-4333-8333-333333333333', 1, 'created-from', "
                    "NULL, NULL, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc')"
                ),
            }
        )
    for name, sql in preserved.items():
        actual = _mutated_digest(vector, lambda connection, sql=sql: connection.execute(sql))
        if actual != baseline:
            raise AssertionError(f"{vector['name']}: {name} unexpectedly changed digest")

    def add_signature(connection: sqlite3.Connection) -> None:
        if is_v03:
            connection.execute("UPDATE capsule_signature SET signature = zeroblob(64)")
            return
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
        "permission request": (
            "UPDATE capsule_manifest SET permissions_json = "
            "'{\"database.read\":{\"required\":false}}'"
        ),
        "command": "UPDATE capsule_command SET purpose = purpose || '!'",
        "runbook": "UPDATE capsule_runbook SET body_md = body_md || '!'",
        "document": "UPDATE capsule_doc SET content = content || '!'",
        "endpoint": "UPDATE capsule_endpoint SET sql_text = sql_text || ' '",
        "check": "UPDATE capsule_check SET description = description || '!'",
        "prompt": "UPDATE capsule_prompt SET prompt_text = prompt_text || '!'",
        "publisher": "UPDATE capsule_publisher SET publisher_name = publisher_name || '!'",
        "schema": "CREATE INDEX vector_note_idx ON vector_domain(note)",
    }
    if is_v03:
        invalidating.update(
            {
                "application id": "UPDATE capsule_manifest SET app_id = app_id || '.changed'",
                "application version": "UPDATE capsule_manifest SET app_version = app_version || '.1'",
                "data schema id": (
                    "UPDATE capsule_manifest SET data_schema_id = data_schema_id || '.changed'"
                ),
                "data schema version": (
                    "UPDATE capsule_manifest SET data_schema_version = data_schema_version + 1"
                ),
                "application profile": (
                    "UPDATE capsule_application SET description = description || '!'"
                ),
                "asset blob": (
                    "UPDATE capsule_asset SET content = X'01' WHERE path = 'app/index.html'"
                ),
                "endpoint step": (
                    "UPDATE capsule_endpoint_step SET required_changes = 2 "
                    "WHERE endpoint_name = 'vector.write' AND sequence = 2"
                ),
                "dataset": "UPDATE capsule_dataset SET description = description || '!'",
                "dataset table": (
                    "UPDATE capsule_dataset_table SET ignored_columns_json = '[\"note\"]' "
                    "WHERE table_name = 'vector_domain'"
                ),
                "dataset dependency": (
                    "UPDATE capsule_dataset_dependency SET reason = reason || '!'"
                ),
                "migration": "UPDATE capsule_migration SET description = description || '!'",
                "migration step": (
                    "UPDATE capsule_migration_step SET definition_json = "
                    "'{\"operation\":\"discard_dataset\",\"dataset_id\":\"content\"}'"
                ),
                "migration check": (
                    "UPDATE capsule_migration_check SET description = description || '!'"
                ),
            }
        )
    else:
        invalidating.update(
            {
                "manifest text": "UPDATE capsule_manifest SET title = title || '!'",
                "asset blob": "UPDATE capsule_asset SET content = X'01' WHERE path = 'app/é.bin'",
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

    if is_v03:
        for object_kind, sql in {
            "table": "CREATE TABLE capsule_unreviewed (id TEXT PRIMARY KEY)",
            "view": "CREATE VIEW capsule_unreviewed AS SELECT 1 AS value",
            "index": "CREATE INDEX capsule_unreviewed ON vector_domain(note)",
        }.items():
            connection = build_fixture(vector)
            try:
                connection.execute(sql)
                try:
                    _digest(connection)
                except VectorError:
                    pass
                else:
                    raise AssertionError(
                        f"{vector['name']}: unknown platform {object_kind} was accepted"
                    )
            finally:
                connection.close()


def computed_vectors(vector_file: Path = VECTOR_FILE) -> dict[str, Any]:
    declared = json.loads(vector_file.read_text(encoding="utf-8"))
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


def verify_vectors(vector_file: Path = VECTOR_FILE) -> dict[str, Any]:
    declared = json.loads(vector_file.read_text(encoding="utf-8"))
    computed = computed_vectors(vector_file)
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
        if "signature_hex" in expected:
            connection = build_fixture(expected)
            try:
                rows = connection.execute(
                    "SELECT key_id, hex(public_key), hex(application_digest), "
                    "hex(signature), signed_at FROM capsule_signature"
                ).fetchall()
            finally:
                connection.close()
            declared_envelope = (
                expected["key_id"],
                expected["public_key_hex"].upper(),
                expected["application_digest_sha256"].upper(),
                expected["signature_hex"].upper(),
                expected["signed_at"],
            )
            if rows != [declared_envelope]:
                raise AssertionError(f"{expected['name']}: stored signature vector differs")
        assert_mutation_contract(expected, actual["application_digest_sha256"])
    return computed


def computed_all_vectors() -> dict[str, Any]:
    return {
        "profiles": [computed_vectors(VECTOR_FILE), computed_vectors(V03_VECTOR_FILE)]
    }


def verify_all_vectors() -> dict[str, Any]:
    return {
        "profiles": [verify_vectors(VECTOR_FILE), verify_vectors(V03_VECTOR_FILE)]
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--emit",
        action="store_true",
        help="print computed vector values without comparing them",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="check both the immutable v0.2 baseline and the v0.3 vectors",
    )
    arguments = parser.parse_args(argv)
    try:
        if arguments.all:
            result = computed_all_vectors() if arguments.emit else verify_all_vectors()
        else:
            result = computed_vectors() if arguments.emit else verify_vectors()
    except (AssertionError, OSError, sqlite3.DatabaseError, VectorError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2, sort_keys=True))
        return 1
    print(json.dumps({"ok": True, **result}, indent=2, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

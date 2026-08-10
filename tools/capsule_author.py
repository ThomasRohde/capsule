#!/usr/bin/env python3
"""Deterministic, product-independent authoring tools for SQLite Capsules."""

from __future__ import annotations

import argparse
import base64
import collections
import hashlib
import json
import os
import shutil
import sqlite3
import tempfile
from pathlib import Path
from typing import Any, Iterable, Mapping

from runtime.capsule_host import CapsuleDatabase, connect_database

BUNDLE_FORMAT = "org.sqlite-capsule.authoring-bundle/0.2"
BUNDLE_FILE = "capsule-unpack.json"
MAX_DIFF_KEYS = 20


class AuthoringError(RuntimeError):
    """Raised for invalid or unsafe authoring operations."""


def _json_dump(value: Any, *, indent: int | None = None) -> str:
    separators = None if indent is not None else (",", ":")
    return json.dumps(
        value,
        ensure_ascii=False,
        indent=indent,
        sort_keys=True,
        separators=separators,
    )


def _sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _quote_identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def _encode_value(value: Any) -> Any:
    if isinstance(value, bytes):
        return {"$capsule_blob": base64.b64encode(value).decode("ascii")}
    if value is None or isinstance(value, (str, int, float)):
        return value
    raise AuthoringError(f"Unsupported SQLite value type: {type(value).__name__}")


def _decode_value(value: Any, bundle: Path) -> Any:
    if not isinstance(value, dict):
        return value
    if set(value) == {"$capsule_blob"} and isinstance(value["$capsule_blob"], str):
        try:
            return base64.b64decode(value["$capsule_blob"], validate=True)
        except ValueError as exc:
            raise AuthoringError("Invalid base64 blob in authoring bundle") from exc
    if set(value) == {"$capsule_file", "sha256", "bytes"}:
        relative = value["$capsule_file"]
        expected_hash = value["sha256"]
        expected_bytes = value["bytes"]
        if not isinstance(relative, str) or not isinstance(expected_hash, str):
            raise AuthoringError("Invalid file reference in authoring bundle")
        candidate = (bundle / relative).resolve()
        try:
            candidate.relative_to(bundle.resolve())
        except ValueError as exc:
            raise AuthoringError(f"File reference escapes the bundle: {relative!r}") from exc
        if not candidate.is_file():
            raise AuthoringError(f"Referenced bundle file does not exist: {relative!r}")
        content = candidate.read_bytes()
        if len(content) != expected_bytes or _sha256_bytes(content) != expected_hash:
            raise AuthoringError(f"Referenced bundle file failed integrity check: {relative!r}")
        return content
    raise AuthoringError("Unknown typed value wrapper in authoring bundle")


def _schema_objects(connection: sqlite3.Connection) -> list[dict[str, str]]:
    rows = connection.execute(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema "
        "WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL "
        "ORDER BY CASE type "
        "WHEN 'table' THEN 0 WHEN 'index' THEN 1 WHEN 'view' THEN 2 ELSE 3 END, name"
    ).fetchall()
    return [
        {
            "type": str(row["type"]),
            "name": str(row["name"]),
            "table": str(row["tbl_name"]),
            "sql": str(row["sql"]),
        }
        for row in rows
    ]


def _table_columns(connection: sqlite3.Connection, table: str) -> list[dict[str, Any]]:
    rows = connection.execute(f"PRAGMA table_info({_quote_identifier(table)})").fetchall()
    return [
        {
            "name": str(row["name"]),
            "type": str(row["type"]),
            "not_null": bool(row["notnull"]),
            "primary_key": int(row["pk"]),
        }
        for row in rows
    ]


def _read_table_rows(
    connection: sqlite3.Connection,
    table: str,
    columns: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    column_names = [column["name"] for column in columns]
    projection = ", ".join(_quote_identifier(name) for name in column_names)
    primary_key = [
        column["name"]
        for column in sorted(columns, key=lambda item: item["primary_key"] or 1_000_000)
        if column["primary_key"]
    ]
    order = ""
    if primary_key:
        order = " ORDER BY " + ", ".join(_quote_identifier(name) for name in primary_key)
    rows = connection.execute(
        f"SELECT {projection} FROM {_quote_identifier(table)}{order}"
    ).fetchall()
    result = [
        {name: _encode_value(row[name]) for name in column_names}
        for row in rows
    ]
    if not primary_key:
        result.sort(key=_json_dump)
    return result


def _externalise_asset_content(row: dict[str, Any], staging: Path) -> dict[str, Any]:
    content = row.get("content")
    if not isinstance(content, dict) or set(content) != {"$capsule_blob"}:
        return row
    raw = base64.b64decode(content["$capsule_blob"], validate=True)
    digest = _sha256_bytes(raw)
    relative = Path("assets") / "by-sha256" / digest
    target = staging / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() and target.read_bytes() != raw:
        raise AuthoringError(f"Content-addressed asset collision: {digest}")
    if not target.exists():
        target.write_bytes(raw)
    prepared = dict(row)
    prepared["content"] = {
        "$capsule_file": relative.as_posix(),
        "sha256": digest,
        "bytes": len(raw),
    }
    return prepared


def _manifest_identity(connection: sqlite3.Connection) -> dict[str, Any] | None:
    tables = {
        str(row[0])
        for row in connection.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'table'"
        ).fetchall()
    }
    if "capsule_manifest" not in tables:
        return None
    row = connection.execute(
        "SELECT format_id, format_version, capsule_id, app_id, app_version, title "
        "FROM capsule_manifest WHERE id = 1"
    ).fetchone()
    return dict(row) if row is not None else None


def unpack_capsule(capsule_path: Path, output_directory: Path) -> dict[str, Any]:
    """Unpack any capsule into a deterministic, reviewable authoring bundle."""

    capsule_path = capsule_path.expanduser().resolve()
    output_directory = output_directory.expanduser().resolve()
    if not capsule_path.is_file():
        raise AuthoringError(f"Capsule not found: {capsule_path}")
    if output_directory.exists():
        raise AuthoringError(f"Refusing to replace existing path: {output_directory}")
    output_directory.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{output_directory.name}.", dir=output_directory.parent)
    )
    try:
        connection = connect_database(capsule_path, read_only=True)
        try:
            schema = _schema_objects(connection)
            tables = [item["name"] for item in schema if item["type"] == "table"]
            table_specs: list[dict[str, Any]] = []
            for index, table in enumerate(tables):
                columns = _table_columns(connection, table)
                rows = _read_table_rows(connection, table, columns)
                if table == "capsule_asset":
                    rows = [_externalise_asset_content(row, staging) for row in rows]
                relative = Path("tables") / f"{index:04d}.jsonl"
                target = staging / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                body = "".join(_json_dump(row) + "\n" for row in rows)
                target.write_text(body, encoding="utf-8", newline="\n")
                table_specs.append(
                    {
                        "name": table,
                        "columns": columns,
                        "rows": len(rows),
                        "data": relative.as_posix(),
                    }
                )
            sequence_rows: list[dict[str, Any]] = []
            has_sequence = connection.execute(
                "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'sqlite_sequence'"
            ).fetchone()
            if has_sequence:
                sequence_rows = [
                    {"name": str(row["name"]), "seq": int(row["seq"])}
                    for row in connection.execute(
                        "SELECT name, seq FROM sqlite_sequence ORDER BY name"
                    ).fetchall()
                ]
            metadata = {
                "bundle_format": BUNDLE_FORMAT,
                "source": {
                    "filename": capsule_path.name,
                    "bytes": capsule_path.stat().st_size,
                    "sha256": _sha256_file(capsule_path),
                    "manifest": _manifest_identity(connection),
                },
                "pragmas": {
                    "application_id": int(
                        connection.execute("PRAGMA application_id").fetchone()[0]
                    ),
                    "user_version": int(
                        connection.execute("PRAGMA user_version").fetchone()[0]
                    ),
                    "page_size": int(connection.execute("PRAGMA page_size").fetchone()[0]),
                },
                "schema": schema,
                "tables": table_specs,
                "sqlite_sequence": sequence_rows,
            }
        finally:
            connection.close()
        (staging / BUNDLE_FILE).write_text(
            _json_dump(metadata, indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        staging.replace(output_directory)
        return {
            "ok": True,
            "capsule": str(capsule_path),
            "output": str(output_directory),
            "tables": len(metadata["tables"]),
            "schema_objects": len(metadata["schema"]),
            "source_sha256": metadata["source"]["sha256"],
        }
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def _load_bundle(directory: Path) -> dict[str, Any]:
    metadata_path = directory / BUNDLE_FILE
    if not metadata_path.is_file():
        raise AuthoringError(f"Authoring bundle is missing {BUNDLE_FILE}: {directory}")
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise AuthoringError(f"Could not read authoring bundle: {exc}") from exc
    if metadata.get("bundle_format") != BUNDLE_FORMAT:
        raise AuthoringError(
            f"Unsupported authoring bundle format: {metadata.get('bundle_format')!r}"
        )
    if not isinstance(metadata.get("schema"), list) or not isinstance(
        metadata.get("tables"), list
    ):
        raise AuthoringError("Authoring bundle schema and tables must be arrays")
    return metadata


def _validate_schema_object(item: Mapping[str, Any]) -> tuple[str, str]:
    kind = item.get("type")
    name = item.get("name")
    sql_text = item.get("sql")
    allowed_prefixes = {
        "table": ("CREATE TABLE", "CREATE VIRTUAL TABLE"),
        "index": ("CREATE INDEX", "CREATE UNIQUE INDEX"),
        "view": ("CREATE VIEW",),
        "trigger": ("CREATE TRIGGER",),
    }
    if kind not in allowed_prefixes or not isinstance(name, str) or not name:
        raise AuthoringError(f"Invalid schema object declaration: {item!r}")
    if not isinstance(sql_text, str) or not sql_text.lstrip().upper().startswith(
        allowed_prefixes[kind]
    ):
        raise AuthoringError(f"Schema SQL does not match declared type for {name!r}")
    return str(kind), name


def _read_jsonl(bundle: Path, relative: str, expected_rows: int) -> list[dict[str, Any]]:
    candidate = (bundle / relative).resolve()
    try:
        candidate.relative_to(bundle.resolve())
    except ValueError as exc:
        raise AuthoringError(f"Table data path escapes the bundle: {relative!r}") from exc
    if not candidate.is_file():
        raise AuthoringError(f"Missing table data file: {relative!r}")
    rows: list[dict[str, Any]] = []
    for line_number, line in enumerate(candidate.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            raise AuthoringError(f"Invalid JSON at {relative}:{line_number}: {exc}") from exc
        if not isinstance(row, dict):
            raise AuthoringError(f"Row at {relative}:{line_number} must be an object")
        rows.append(row)
    if len(rows) != expected_rows:
        raise AuthoringError(
            f"Row count mismatch for {relative!r}: expected {expected_rows}, found {len(rows)}"
        )
    return rows


def pack_capsule(
    input_directory: Path,
    output_path: Path,
    *,
    replace: bool = False,
) -> dict[str, Any]:
    """Pack a deterministic authoring bundle into a verified capsule."""

    bundle = input_directory.expanduser().resolve()
    output_path = output_path.expanduser().resolve()
    if not bundle.is_dir():
        raise AuthoringError(f"Authoring bundle not found: {bundle}")
    if output_path.exists() and not replace:
        raise AuthoringError(f"Refusing to replace existing capsule: {output_path}")
    metadata = _load_bundle(bundle)
    schema_items = metadata["schema"]
    declared_objects = [_validate_schema_object(item) for item in schema_items]
    if len(declared_objects) != len(set(declared_objects)):
        raise AuthoringError("Authoring bundle contains duplicate schema objects")
    table_schema = {
        item["name"]: item for item in schema_items if item.get("type") == "table"
    }
    table_specs = metadata["tables"]
    if {item.get("name") for item in table_specs} != set(table_schema):
        raise AuthoringError("Table data inventory does not match the schema inventory")

    pragmas = metadata.get("pragmas", {})
    try:
        application_id = int(pragmas["application_id"])
        user_version = int(pragmas["user_version"])
        page_size = int(pragmas["page_size"])
    except (KeyError, TypeError, ValueError) as exc:
        raise AuthoringError("Authoring bundle has invalid SQLite pragmas") from exc
    if not 512 <= page_size <= 65536 or page_size & (page_size - 1):
        raise AuthoringError(f"Invalid SQLite page size: {page_size}")

    output_path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{output_path.name}.", suffix=".tmp", dir=output_path.parent
    )
    os.close(descriptor)
    temporary = Path(temporary_name)
    temporary.unlink(missing_ok=True)
    try:
        connection = sqlite3.connect(temporary)
        try:
            connection.row_factory = sqlite3.Row
            connection.execute(f"PRAGMA page_size = {page_size}")
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute("PRAGMA foreign_keys = OFF")
            connection.execute(f"PRAGMA application_id = {application_id}")
            connection.execute(f"PRAGMA user_version = {user_version}")
            for item in schema_items:
                if item["type"] == "table":
                    connection.execute(item["sql"])
                    created = connection.execute(
                        "SELECT type FROM sqlite_schema WHERE name = ?", (item["name"],)
                    ).fetchone()
                    if created is None or created["type"] != "table":
                        raise AuthoringError(f"Schema did not create table {item['name']!r}")

            connection.execute("BEGIN")
            for spec in table_specs:
                table = spec.get("name")
                columns = spec.get("columns")
                relative = spec.get("data")
                expected_rows = spec.get("rows")
                if (
                    not isinstance(table, str)
                    or not isinstance(columns, list)
                    or not isinstance(relative, str)
                    or not isinstance(expected_rows, int)
                ):
                    raise AuthoringError(f"Invalid table inventory entry: {spec!r}")
                column_names = [item.get("name") for item in columns]
                if not column_names or not all(isinstance(name, str) for name in column_names):
                    raise AuthoringError(f"Invalid column inventory for table {table!r}")
                rows = _read_jsonl(bundle, relative, expected_rows)
                sql_text = (
                    f"INSERT INTO {_quote_identifier(table)} "
                    f"({', '.join(_quote_identifier(name) for name in column_names)}) "
                    f"VALUES ({', '.join('?' for _ in column_names)})"
                )
                for row in rows:
                    if set(row) != set(column_names):
                        raise AuthoringError(f"Row columns do not match table {table!r}")
                    values = [_decode_value(row[name], bundle) for name in column_names]
                    connection.execute(sql_text, values)

            sequence_rows = metadata.get("sqlite_sequence", [])
            has_sequence = connection.execute(
                "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'sqlite_sequence'"
            ).fetchone()
            if sequence_rows and not has_sequence:
                raise AuthoringError("Bundle declares sqlite_sequence without AUTOINCREMENT tables")
            if has_sequence:
                connection.execute("DELETE FROM sqlite_sequence")
                for row in sequence_rows:
                    connection.execute(
                        "INSERT INTO sqlite_sequence(name, seq) VALUES (?, ?)",
                        (str(row["name"]), int(row["seq"])),
                    )
            connection.commit()

            for item in schema_items:
                if item["type"] != "table":
                    connection.execute(item["sql"])
                    created = connection.execute(
                        "SELECT type FROM sqlite_schema WHERE name = ?", (item["name"],)
                    ).fetchone()
                    if created is None or created["type"] != item["type"]:
                        raise AuthoringError(
                            f"Schema did not create {item['type']} {item['name']!r}"
                        )
            connection.commit()
            connection.execute("PRAGMA foreign_keys = ON")
            foreign_key_errors = connection.execute("PRAGMA foreign_key_check").fetchall()
            if foreign_key_errors:
                raise AuthoringError(f"Packed capsule has foreign-key errors: {foreign_key_errors}")
            connection.execute("VACUUM")
        finally:
            connection.close()

        with CapsuleDatabase(temporary, read_only=True) as capsule:
            report = capsule.verify()
        if not report["ok"]:
            raise AuthoringError(
                "Packed capsule failed verification: " + "; ".join(report["errors"])
            )
        temporary.replace(output_path)
        return {
            "ok": True,
            "input": str(bundle),
            "output": str(output_path),
            "bytes": output_path.stat().st_size,
            "sha256": _sha256_file(output_path),
            "verification": report,
        }
    finally:
        temporary.unlink(missing_ok=True)


def _row_digest(row: Mapping[str, Any]) -> str:
    return _sha256_bytes(_json_dump(row).encode("utf-8"))


def _diff_key(value: tuple[Any, ...]) -> str:
    return _json_dump(list(value))


def _table_snapshot(
    connection: sqlite3.Connection, table: str
) -> tuple[list[str], dict[str, dict[str, Any]] | collections.Counter[str]]:
    columns = _table_columns(connection, table)
    rows = _read_table_rows(connection, table, columns)
    if table == "capsule_asset":
        for row in rows:
            content = row.get("content")
            if isinstance(content, dict) and "$capsule_blob" in content:
                raw = base64.b64decode(content["$capsule_blob"], validate=True)
                row["content"] = {"bytes": len(raw), "sha256": _sha256_bytes(raw)}
    primary_key = [
        column["name"]
        for column in sorted(columns, key=lambda item: item["primary_key"] or 1_000_000)
        if column["primary_key"]
    ]
    if primary_key:
        keyed = {
            _diff_key(tuple(row[name] for name in primary_key)): row
            for row in rows
        }
        return primary_key, keyed
    return [], collections.Counter(_row_digest(row) for row in rows)


def _identity_for_diff(path: Path, connection: sqlite3.Connection) -> dict[str, Any]:
    return {
        "path": str(path),
        "bytes": path.stat().st_size,
        "sha256": _sha256_file(path),
        "manifest": _manifest_identity(connection),
    }


def diff_capsules(left_path: Path, right_path: Path) -> dict[str, Any]:
    """Return a bounded semantic diff between two capsule databases."""

    left_path = left_path.expanduser().resolve()
    right_path = right_path.expanduser().resolve()
    if not left_path.is_file() or not right_path.is_file():
        raise AuthoringError("Both diff inputs must be existing capsule files")
    left = connect_database(left_path, read_only=True)
    right = connect_database(right_path, read_only=True)
    try:
        left_schema = {(item["type"], item["name"]): item["sql"] for item in _schema_objects(left)}
        right_schema = {
            (item["type"], item["name"]): item["sql"] for item in _schema_objects(right)
        }
        schema_added = sorted(right_schema.keys() - left_schema.keys())
        schema_removed = sorted(left_schema.keys() - right_schema.keys())
        schema_changed = sorted(
            key
            for key in left_schema.keys() & right_schema.keys()
            if left_schema[key] != right_schema[key]
        )
        left_tables = {name for kind, name in left_schema if kind == "table"}
        right_tables = {name for kind, name in right_schema if kind == "table"}
        table_changes: dict[str, Any] = {}
        for table in sorted(left_tables | right_tables):
            if table not in left_tables:
                table_changes[table] = {"status": "added"}
                continue
            if table not in right_tables:
                table_changes[table] = {"status": "removed"}
                continue
            left_primary_key, left_rows = _table_snapshot(left, table)
            right_primary_key, right_rows = _table_snapshot(right, table)
            if left_primary_key != right_primary_key:
                table_changes[table] = {"status": "primary-key-changed"}
                continue
            if left_primary_key:
                assert isinstance(left_rows, dict) and isinstance(right_rows, dict)
                added = sorted(right_rows.keys() - left_rows.keys())
                removed = sorted(left_rows.keys() - right_rows.keys())
                changed = sorted(
                    key
                    for key in left_rows.keys() & right_rows.keys()
                    if left_rows[key] != right_rows[key]
                )
                if added or removed or changed:
                    table_changes[table] = {
                        "status": "changed",
                        "primary_key": left_primary_key,
                        "left_rows": len(left_rows),
                        "right_rows": len(right_rows),
                        "added": added[:MAX_DIFF_KEYS],
                        "removed": removed[:MAX_DIFF_KEYS],
                        "changed": changed[:MAX_DIFF_KEYS],
                        "truncated": {
                            "added": max(0, len(added) - MAX_DIFF_KEYS),
                            "removed": max(0, len(removed) - MAX_DIFF_KEYS),
                            "changed": max(0, len(changed) - MAX_DIFF_KEYS),
                        },
                    }
            else:
                assert isinstance(left_rows, collections.Counter)
                assert isinstance(right_rows, collections.Counter)
                added_count = sum((right_rows - left_rows).values())
                removed_count = sum((left_rows - right_rows).values())
                if added_count or removed_count:
                    table_changes[table] = {
                        "status": "changed",
                        "primary_key": [],
                        "left_rows": sum(left_rows.values()),
                        "right_rows": sum(right_rows.values()),
                        "added_rows": added_count,
                        "removed_rows": removed_count,
                    }
        pragmas: dict[str, dict[str, int]] = {}
        for pragma in ("application_id", "user_version"):
            left_value = int(left.execute(f"PRAGMA {pragma}").fetchone()[0])
            right_value = int(right.execute(f"PRAGMA {pragma}").fetchone()[0])
            if left_value != right_value:
                pragmas[pragma] = {"left": left_value, "right": right_value}
        schema_report = {
            "added": [{"type": kind, "name": name} for kind, name in schema_added],
            "removed": [{"type": kind, "name": name} for kind, name in schema_removed],
            "changed": [{"type": kind, "name": name} for kind, name in schema_changed],
        }
        equal = not pragmas and not any(schema_report.values()) and not table_changes
        return {
            "ok": True,
            "equal": equal,
            "left": _identity_for_diff(left_path, left),
            "right": _identity_for_diff(right_path, right),
            "pragmas": pragmas,
            "schema": schema_report,
            "tables": table_changes,
        }
    finally:
        left.close()
        right.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    unpack_parser = subparsers.add_parser(
        "unpack", help="Unpack a capsule into a deterministic authoring bundle"
    )
    unpack_parser.add_argument("capsule", type=Path)
    unpack_parser.add_argument("output", type=Path)

    pack_parser = subparsers.add_parser(
        "pack", help="Pack and verify a deterministic authoring bundle"
    )
    pack_parser.add_argument("input", type=Path)
    pack_parser.add_argument("output", type=Path)
    pack_parser.add_argument(
        "--replace", action="store_true", help="Replace an existing output capsule"
    )

    diff_parser = subparsers.add_parser("diff", help="Compare two capsules semantically")
    diff_parser.add_argument("left", type=Path)
    diff_parser.add_argument("right", type=Path)

    return parser


def main(argv: Iterable[str] | None = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    try:
        if args.command == "unpack":
            result = unpack_capsule(args.capsule, args.output)
        elif args.command == "pack":
            result = pack_capsule(args.input, args.output, replace=args.replace)
        elif args.command == "diff":
            result = diff_capsules(args.left, args.right)
        else:  # pragma: no cover - argparse prevents this
            raise AuthoringError(f"Unknown authoring command: {args.command}")
    except (AuthoringError, OSError, sqlite3.DatabaseError) as exc:
        print(_json_dump({"ok": False, "error": str(exc)}, indent=2))
        return 2
    print(_json_dump(result, indent=2))
    if args.command == "diff" and not result["equal"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

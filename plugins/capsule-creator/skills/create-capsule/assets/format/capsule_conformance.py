#!/usr/bin/env python3
"""Independent SQLite Capsule v0.2 platform conformance checker.

This module deliberately uses only sqlite3 and the JSON conformance document. It
does not import the runtime verifier, so it provides an independent signal when
the host implementation and the format description accidentally drift apart.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path
from typing import Any, Mapping


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SPEC = ROOT / "format" / "capsule-v0.2.conformance.json"


class ConformanceError(Exception):
    """Raised for malformed conformance documents or inaccessible capsules."""


def _readonly_connection(path: Path) -> sqlite3.Connection:
    resolved = path.resolve()
    uri = f"file:{resolved.as_posix()}?mode=ro"
    connection = sqlite3.connect(uri, uri=True, timeout=5.0)
    connection.row_factory = sqlite3.Row
    return connection


def _identifier(value: str) -> str:
    if not value or any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_" for char in value):
        raise ConformanceError(f"Unsafe identifier in conformance document: {value!r}")
    return '"' + value.replace('"', '""') + '"'


def _load_spec(path: Path) -> Mapping[str, Any]:
    path = path.expanduser().resolve()
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ConformanceError(f"Could not load conformance document {path}: {exc}") from exc
    if not isinstance(document, dict):
        raise ConformanceError("Conformance document must be a JSON object")
    if document.get("description_format") != "org.sqlite-capsule.conformance/0.2":
        raise ConformanceError("Unsupported conformance document format")
    return document


def _object_names(connection: sqlite3.Connection) -> dict[str, str]:
    return {
        row["name"]: row["type"]
        for row in connection.execute(
            "SELECT name, type FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'"
        )
    }


def _check_columns(
    connection: sqlite3.Connection,
    table: str,
    expected: Mapping[str, Mapping[str, Any]],
    expected_primary_key: list[str],
    errors: list[str],
) -> None:
    try:
        rows = connection.execute(f"PRAGMA table_xinfo({_identifier(table)})").fetchall()
    except sqlite3.DatabaseError as exc:
        errors.append(f"{table}: unable to inspect columns: {exc}")
        return
    actual = {row["name"]: row for row in rows}
    for column, rules in expected.items():
        row = actual.get(column)
        if row is None:
            errors.append(f"{table}: missing required column {column!r}")
            continue
        expected_type = str(rules.get("type", "")).upper()
        actual_type = str(row["type"] or "").upper()
        if expected_type and actual_type != expected_type:
            errors.append(f"{table}.{column}: expected type {expected_type}, got {actual_type or '<empty>'}")
        if bool(rules.get("notnull")) != bool(row["notnull"]):
            errors.append(
                f"{table}.{column}: expected notnull={bool(rules.get('notnull'))}, got {bool(row['notnull'])}"
            )
        if "pk" in rules and int(rules["pk"]) != int(row["pk"]):
            errors.append(f"{table}.{column}: expected pk={rules['pk']}, got {row['pk']}")

    primary_key = [row["name"] for row in sorted(rows, key=lambda item: item["pk"]) if row["pk"]]
    if expected_primary_key and primary_key != expected_primary_key:
        errors.append(f"{table}: expected primary key {expected_primary_key}, got {primary_key}")


def _check_foreign_keys(
    connection: sqlite3.Connection,
    table: str,
    expected: list[Mapping[str, str]],
    errors: list[str],
) -> None:
    actual = {
        (row["from"], row["table"], row["to"])
        for row in connection.execute(f"PRAGMA foreign_key_list({_identifier(table)})")
    }
    required = {(item["from"], item["table"], item["to"]) for item in expected}
    for foreign_key in sorted(required - actual):
        errors.append(f"{table}: missing foreign key {foreign_key}")


def check_conformance(capsule: Path, spec_path: Path | None = None) -> dict[str, Any]:
    capsule = capsule.expanduser().resolve()
    if spec_path is None:
        spec_path = DEFAULT_SPEC
    spec_path = spec_path.expanduser().resolve()
    spec = _load_spec(spec_path)
    errors: list[str] = []
    identity = spec.get("identity", {})
    required = spec.get("required_objects", {})
    optional = spec.get("optional_objects", {})
    tables = required.get("tables", {})
    views = required.get("views", {})
    optional_tables = optional.get("tables", {}) if isinstance(optional, dict) else {}
    if not isinstance(tables, dict) or not isinstance(views, dict) or not isinstance(optional_tables, dict):
        raise ConformanceError("Conformance document has invalid required_objects")

    try:
        connection = _readonly_connection(capsule)
    except (OSError, sqlite3.DatabaseError) as exc:
        raise ConformanceError(f"Could not open capsule read-only: {exc}") from exc
    try:
        application_id = connection.execute("PRAGMA application_id").fetchone()[0]
        user_version = connection.execute("PRAGMA user_version").fetchone()[0]
        if application_id != identity.get("application_id"):
            errors.append(f"application_id: expected {identity.get('application_id')}, got {application_id}")
        if user_version != identity.get("user_version"):
            errors.append(f"user_version: expected {identity.get('user_version')}, got {user_version}")

        names = _object_names(connection)
        for table, declaration in tables.items():
            if names.get(table) != "table":
                errors.append(f"missing required table {table!r}")
                continue
            columns = dict(declaration.get("columns", {}))
            _check_columns(connection, table, columns, list(declaration.get("primary_key", [])), errors)
            _check_foreign_keys(connection, table, declaration.get("foreign_keys", []), errors)
            if table in set(spec.get("minimum_nonempty_tables", [])):
                count = connection.execute(f"SELECT COUNT(*) FROM {_identifier(table)}").fetchone()[0]
                if count < 1:
                    errors.append(f"{table}: required non-empty table is empty")

        for table, declaration in optional_tables.items():
            if names.get(table) != "table":
                continue
            _check_columns(
                connection,
                table,
                declaration.get("columns", {}),
                list(declaration.get("primary_key", [])),
                errors,
            )
            _check_foreign_keys(connection, table, declaration.get("foreign_keys", []), errors)

        for view, declaration in views.items():
            if names.get(view) != "view":
                errors.append(f"missing required view {view!r}")
                continue
            try:
                cursor = connection.execute(f"SELECT * FROM {_identifier(view)} LIMIT 0")
                columns = [item[0] for item in cursor.description or ()]
            except sqlite3.DatabaseError as exc:
                errors.append(f"{view}: unable to inspect result shape: {exc}")
                continue
            if columns != list(declaration.get("columns", [])):
                errors.append(f"{view}: expected columns {declaration.get('columns', [])}, got {columns}")

        manifest = connection.execute(
            "SELECT format_id, format_version, runtime_protocol FROM capsule_manifest WHERE id = 1"
        ).fetchone()
        if manifest is None:
            errors.append("capsule_manifest: required id=1 row is missing")
        else:
            for column in ("format_id", "format_version", "runtime_protocol"):
                expected = identity.get(column)
                if manifest[column] != expected:
                    errors.append(f"capsule_manifest.{column}: expected {expected!r}, got {manifest[column]!r}")

        return {
            "capsule": str(capsule.resolve()),
            "spec": str(spec_path.resolve()),
            "ok": not errors,
            "errors": errors,
            "identity": dict(identity),
        }
    finally:
        connection.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Check a capsule against its independent platform conformance document"
    )
    parser.add_argument("capsule", type=Path)
    parser.add_argument("--spec", type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        report = check_conformance(args.capsule, args.spec)
    except ConformanceError as exc:
        report = {"capsule": str(args.capsule), "spec": str(args.spec), "ok": False, "errors": [str(exc)]}
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

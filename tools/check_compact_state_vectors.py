#!/usr/bin/env python3
"""Independently verify Capsule compact-logical-state/1 vectors."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
import sqlite3
import struct
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
VECTOR_ROOT = ROOT / "compatibility" / "compact-logical-state-v1"
PROFILE = b"org.sqlite-capsule.compact-logical-state/1"
MAX_BYTES = 512 * 1024 * 1024
MAX_ROWS = 100_000
MAX_OBJECTS = 4_096
MAX_COLUMNS = 256


class VectorError(ValueError):
    pass


class Writer:
    def __init__(self, digest: Any | None = None) -> None:
        self.digest = hashlib.sha256() if digest is None else digest
        self.count = 0

    def fixed(self, value: bytes) -> None:
        self.count += len(value)
        if self.count > MAX_BYTES:
            raise VectorError("compact logical stream exceeded byte ceiling")
        self.digest.update(value)

    def frame(self, value: bytes | str) -> None:
        encoded = value.encode("utf-8") if isinstance(value, str) else value
        self.fixed(struct.pack(">Q", len(encoded)))
        self.fixed(encoded)

    def value(self, value: Any) -> None:
        if value is None:
            self.fixed(b"\x00")
        elif isinstance(value, int):
            self.fixed(b"\x01" + struct.pack(">q", value))
        elif isinstance(value, float):
            if not math.isfinite(value):
                raise VectorError("non-finite REAL")
            self.fixed(b"\x02" + struct.pack(">d", value))
        elif isinstance(value, str):
            self.fixed(b"\x03")
            self.frame(value)
        elif isinstance(value, bytes):
            self.fixed(b"\x04")
            self.frame(value)
        else:
            raise VectorError(f"unsupported SQLite value: {type(value).__name__}")


def quote(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def rowid_expression(connection: sqlite3.Connection, table: str) -> str | None:
    without_rowid = int(
        connection.execute(
            "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?",
            (table,),
        ).fetchone()[0]
    )
    if without_rowid == 1:
        return None
    if without_rowid != 0:
        raise VectorError(f"invalid table kind for {table}")
    columns = connection.execute(f"PRAGMA table_xinfo({quote(table)})").fetchall()
    names = {str(row[1]).lower() for row in columns}
    for candidate in ("_rowid_", "rowid", "oid"):
        if candidate not in names:
            return candidate
    return None


def logical_state(connection: sqlite3.Connection) -> dict[str, Any]:
    writer = Writer()
    writer.frame(PROFILE)
    for name in ("application_id", "user_version", "auto_vacuum", "default_cache_size"):
        value = int(connection.execute(f"PRAGMA {name}").fetchone()[0])
        writer.frame(name)
        writer.fixed(struct.pack(">q", value))
    encoding = str(connection.execute("PRAGMA encoding").fetchone()[0])
    writer.frame("encoding")
    writer.frame(encoding)
    page_size = int(connection.execute("PRAGMA page_size").fetchone()[0])

    schema = connection.execute(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema "
        "ORDER BY type COLLATE BINARY,name COLLATE BINARY,tbl_name COLLATE BINARY,sql COLLATE BINARY"
    ).fetchall()
    if len(schema) > MAX_OBJECTS:
        raise VectorError("too many schema objects")
    writer.fixed(struct.pack(">Q", len(schema)))
    table_sql: dict[str, str | None] = {}
    for object_type, name, table_name, sql in schema:
        writer.frame(str(object_type))
        writer.frame(str(name))
        writer.frame(str(table_name))
        if sql is None:
            writer.fixed(b"\x00")
        else:
            writer.fixed(b"\x01")
            writer.frame(str(sql))
        if object_type == "table":
            table_sql[str(name)] = None if sql is None else str(sql)

    tables = sorted(
        name
        for name in table_sql
        if not name.startswith("sqlite_")
        or name == "sqlite_sequence"
        or name.startswith("sqlite_stat")
    )
    writer.fixed(struct.pack(">Q", len(tables)))
    total_rows = 0
    for table in tables:
        if table.startswith("sqlite_stat") and table not in {
            "sqlite_stat1", "sqlite_stat2", "sqlite_stat3", "sqlite_stat4"
        }:
            raise VectorError("unsupported sqlite statistics table")
        columns = [
            str(row[1])
            for row in connection.execute(f"PRAGMA table_xinfo({quote(table)})").fetchall()
        ]
        if not columns or len(columns) > MAX_COLUMNS:
            raise VectorError("invalid column count")
        rowid = rowid_expression(connection, table)
        projection = ",".join(quote(column) for column in columns)
        if rowid is not None:
            projection += f",{rowid}"
        rows = connection.execute(f"SELECT {projection} FROM {quote(table)}").fetchall()
        total_rows += len(rows)
        if total_rows > MAX_ROWS:
            raise VectorError("too many rows")
        row_hashes: list[bytes] = []
        for row in rows:
            row_writer = Writer()
            row_writer.frame("row")
            row_writer.frame(table)
            row_writer.fixed(struct.pack(">Q", len(columns) + int(rowid is not None)))
            if rowid is not None:
                row_writer.frame("org.sqlite-capsule.compact.pseudo-rowid/1")
                row_writer.value(row[-1])
            for column, value in zip(columns, row[: len(columns)], strict=True):
                row_writer.frame(column)
                row_writer.value(value)
            writer.count += row_writer.count
            if writer.count > MAX_BYTES:
                raise VectorError("compact logical stream exceeded byte ceiling")
            row_hashes.append(row_writer.digest.digest())
        writer.frame("table")
        writer.frame(table)
        writer.fixed(struct.pack(">Q", len(columns) + int(rowid is not None)))
        if rowid is not None:
            writer.frame("org.sqlite-capsule.compact.pseudo-rowid/1")
        for column in columns:
            writer.frame(column)
        writer.fixed(struct.pack(">Q", len(row_hashes)))
        for digest in sorted(row_hashes):
            writer.fixed(digest)
    return {
        "digest_sha256": writer.digest.hexdigest(),
        "schema_objects": len(schema),
        "tables": len(tables),
        "rows": total_rows,
        "stream_bytes": writer.count,
        "page_size": page_size,
    }


def build(path: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(path)
    connection.executescript((VECTOR_ROOT / "fixture.sql").read_text(encoding="utf-8"))
    connection.commit()
    return connection


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    expected = json.loads((VECTOR_ROOT / "vectors.json").read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "compact.sqlite"
        connection = build(path)
        baseline = logical_state(connection)
        before_file = file_sha256(path)
        connection.execute("VACUUM")
        after = logical_state(connection)
        after_file = file_sha256(path)
        connection.close()
    if baseline != expected["baseline"]:
        print(json.dumps(baseline, sort_keys=True))
        return 1
    if after != baseline or after_file == before_file:
        raise VectorError("VACUUM did not preserve logical state while changing physical bytes")
    for name, mutation in (
        ("domain row", "UPDATE bag SET note='changed' WHERE rowid=1"),
        ("schema", "DROP VIEW bag_view; CREATE VIEW bag_view AS SELECT note FROM bag"),
        ("sqlite_sequence", "UPDATE sqlite_sequence SET seq=10 WHERE name='keyed'"),
        ("signature row", "UPDATE capsule_signature SET signature=X'0304' WHERE key_id='key'"),
        ("implicit rowid", "UPDATE bag SET rowid=2 WHERE rowid=3"),
    ):
        with tempfile.TemporaryDirectory() as directory:
            mutation_path = Path(directory) / "mutation.sqlite"
            connection = build(mutation_path)
            connection.executescript(mutation)
            connection.commit()
            mutated = logical_state(connection)
            connection.close()
        if mutated["digest_sha256"] == baseline["digest_sha256"]:
            raise VectorError(f"{name} mutation did not invalidate compact digest")
    print(
        "compact logical-state vector: "
        f"objects={baseline['schema_objects']} tables={baseline['tables']} "
        f"rows={baseline['rows']} bytes={baseline['stream_bytes']} "
        f"sha256={baseline['digest_sha256']}"
    )
    print("compact logical-state vectors: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

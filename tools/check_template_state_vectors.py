#!/usr/bin/env python3
"""Independently verify Capsule dataset-state/1 canonical digest vectors."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
import sqlite3
import struct
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
VECTOR_FILE = ROOT / "compatibility" / "template-state-v1" / "vectors.json"
CONTEXT = b"SQLite Capsule dataset-state canonical stream v1\0"
MAX_BYTES = 512 * 1024 * 1024


class VectorError(ValueError):
    pass


class Writer:
    def __init__(self) -> None:
        self.digest = hashlib.sha256()
        self.count = 0

    def raw(self, value: bytes) -> None:
        self.count += len(value)
        if self.count > MAX_BYTES:
            raise VectorError("dataset-state stream exceeded its byte ceiling")
        self.digest.update(value)

    def u32(self, value: int) -> None:
        self.raw(struct.pack(">I", value))

    def u64(self, value: int) -> None:
        self.raw(struct.pack(">Q", value))

    def text(self, value: str) -> None:
        encoded = value.encode("utf-8")
        self.u64(len(encoded))
        self.raw(encoded)

    def value(self, value: Any) -> None:
        if value is None:
            self.raw(b"\x00")
        elif isinstance(value, int):
            self.raw(b"\x01")
            self.raw(struct.pack(">q", value))
        elif isinstance(value, float):
            if not math.isfinite(value):
                raise VectorError("non-finite SQLite REAL")
            self.raw(b"\x02")
            self.raw(struct.pack(">d", value))
        elif isinstance(value, str):
            self.raw(b"\x03")
            self.text(value)
        elif isinstance(value, bytes):
            self.raw(b"\x04")
            self.u64(len(value))
            self.raw(value)
        else:
            raise VectorError(f"unsupported SQLite value: {type(value).__name__}")


def quote_identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def build_fixture() -> sqlite3.Connection:
    connection = sqlite3.connect(":memory:")
    connection.executescript((ROOT / "format" / "capsule-v0.3.sql").read_text(encoding="utf-8"))
    connection.executescript(
        (ROOT / "format" / "capsule-signed-app-v0.3.sql").read_text(encoding="utf-8")
    )
    connection.executescript(
        (ROOT / "compatibility" / "signed-app-v0.3" / "fixture-v0.3.sql").read_text(
            encoding="utf-8"
        )
    )
    connection.executescript(
        (ROOT / "compatibility" / "template-state-v1" / "type-spectrum.sql").read_text(
            encoding="utf-8"
        )
    )
    return connection


def dataset_state(connection: sqlite3.Connection, dataset_id: str) -> tuple[int, int, str]:
    manifest = connection.execute(
        "SELECT app_id, data_schema_id, data_schema_version FROM capsule_manifest WHERE id=1"
    ).fetchone()
    if manifest is None:
        raise VectorError("missing manifest")
    tables = connection.execute(
        "SELECT sequence, table_name, primary_key_json FROM capsule_dataset_table "
        "WHERE dataset_id=? ORDER BY sequence, table_name COLLATE BINARY",
        (dataset_id,),
    ).fetchall()
    if not tables:
        raise VectorError(f"dataset has no tables: {dataset_id}")
    writer = Writer()
    writer.raw(CONTEXT)
    writer.text(str(manifest[0]))
    writer.text(str(manifest[1]))
    writer.u64(int(manifest[2]))
    writer.text(dataset_id)
    writer.u32(len(tables))
    total_rows = 0
    for sequence, table_name, primary_key_json in tables:
        primary_key = json.loads(primary_key_json)
        columns = [
            str(row[1])
            for row in connection.execute(
                f"PRAGMA table_xinfo({quote_identifier(str(table_name))})"
            ).fetchall()
        ]
        row_count = int(
            connection.execute(
                f"SELECT count(*) FROM {quote_identifier(str(table_name))}"
            ).fetchone()[0]
        )
        total_rows += row_count
        writer.u32(int(sequence))
        writer.text(str(table_name))
        writer.u32(len(primary_key))
        for key in primary_key:
            writer.text(str(key))
        writer.u32(len(columns))
        for column in columns:
            writer.text(column)
        writer.u64(row_count)
        select = ", ".join(quote_identifier(column) for column in columns)
        ordering = ", ".join(
            f"{quote_identifier(str(column))} COLLATE BINARY ASC" for column in primary_key
        )
        for row in connection.execute(
            f"SELECT {select} FROM {quote_identifier(str(table_name))} ORDER BY {ordering}"
        ):
            for value in row:
                writer.value(value)
    return total_rows, writer.count, writer.digest.hexdigest()


def main() -> int:
    expected = json.loads(VECTOR_FILE.read_text(encoding="utf-8"))
    connection = build_fixture()
    try:
        actual = {
            dataset: dataset_state(connection, dataset)
            for dataset in (
                "content",
                "empty-state",
                "ordering",
                "settings",
                "type-spectrum",
            )
        }
    finally:
        connection.close()
    for dataset, (row_count, stream_bytes, digest) in actual.items():
        wanted = expected["datasets"][dataset]
        if (
            row_count != wanted["stored_row_count"]
            or stream_bytes != wanted["canonical_stream_bytes"]
            or digest != wanted["state_sha256"]
        ):
            print(
                f"template-state vector {dataset}: got rows={row_count} "
                f"bytes={stream_bytes} sha256={digest}"
            )
            return 1
        print(
            f"template-state vector {dataset}: rows={row_count} "
            f"bytes={stream_bytes} sha256={digest}"
        )
    print("template-state vectors: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

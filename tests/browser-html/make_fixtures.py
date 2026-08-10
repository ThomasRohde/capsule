from __future__ import annotations

import hashlib
import json
import sqlite3
import sys
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase  # noqa: E402
from tools.capsule_html import (  # noqa: E402
    MAX_CAPSULE_BYTES,
    _gzip,
    _metadata,
    _render_html,
    _runtime_inputs,
    export_html,
)


def _write_parity_capsule(source: Path, target: Path) -> None:
    target.write_bytes(source.read_bytes())
    parameters = {
        "integer": {"required": True, "type": "integer"},
        "number": {"required": True, "type": "number"},
        "boolean": {"required": True, "type": "boolean"},
        "payload": {"required": True, "type": "json"},
        "text": {"required": True, "type": "string"},
    }
    endpoints = [
        (
            "parity.parameters",
            "SELECT :integer AS integer_value, :number AS number_value, "
            ":boolean AS boolean_value, :payload AS payload_json, :text AS text_value",
            parameters,
            "row",
            "Exercise every endpoint parameter coercion and JSON-column decoding rule.",
        ),
        (
            "parity.scalar-int64",
            "SELECT 9223372036854775807",
            {},
            "scalar",
            "Exercise signed 64-bit integer JSON-number behavior.",
        ),
        (
            "parity.blob-rejected",
            "SELECT zeroblob(4) AS content",
            {},
            "row",
            "Exercise the generic bridge BLOB rejection rule.",
        ),
        (
            "parity.row-limit",
            "WITH RECURSIVE numbers(value) AS (VALUES(1) UNION ALL "
            "SELECT value + 1 FROM numbers WHERE value < 1001) SELECT value FROM numbers",
            {},
            "rows",
            "Exercise the 1000-row result limit.",
        ),
        (
            "parity.byte-limit",
            "SELECT hex(zeroblob(1048577)) AS text",
            {},
            "row",
            "Exercise the two-megabyte JSON result limit.",
        ),
        (
            "parity.slow",
            "WITH RECURSIVE numbers(value) AS (VALUES(1) UNION ALL "
            "SELECT value + 1 FROM numbers WHERE value < 250000) SELECT sum(value) FROM numbers",
            {},
            "scalar",
            "Hold the serial worker briefly while exercising the eight-request concurrency bound.",
        ),
    ]
    connection = sqlite3.connect(target)
    try:
        connection.executemany(
            "INSERT INTO capsule_endpoint "
            "(name, operation, sql_text, parameters_json, result_mode, description, enabled) "
            "VALUES (?, 'read', ?, ?, ?, ?, 1)",
            [
                (name, sql_text, json.dumps(spec, sort_keys=True, separators=(",", ":")), result_mode, description)
                for name, sql_text, spec, result_mode, description in endpoints
            ],
        )
        connection.commit()
    finally:
        connection.close()
    with CapsuleDatabase(target, read_only=True) as parity:
        report = parity.verify()
    if not report["ok"]:
        raise RuntimeError("Parity fixture is invalid: " + "; ".join(report["errors"]))


def _write_unverified_export(
    source: Path,
    output: Path,
    manifest: dict[str, object],
    runtime: dict[str, bytes],
    mutate: Callable[[sqlite3.Connection], None],
) -> None:
    hostile = output.with_suffix(".capsule.sqlite")
    hostile.write_bytes(source.read_bytes())
    connection = sqlite3.connect(hostile)
    try:
        mutate(connection)
        connection.commit()
    finally:
        connection.close()
    database = hostile.read_bytes()
    compressed = _gzip(database)
    metadata = _metadata(database, compressed, manifest, "view", runtime)
    output.write_bytes(_render_html(metadata, compressed, runtime))
    hostile.unlink()


def main() -> None:
    capsule = Path(sys.argv[1]).resolve()
    output = Path(sys.argv[2]).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with CapsuleDatabase(capsule, read_only=True) as source:
        manifest = source.verify()["manifest"]
    runtime = _runtime_inputs()

    def corrupt_asset(connection: sqlite3.Connection) -> None:
        row = connection.execute("SELECT path, content FROM capsule_asset ORDER BY path LIMIT 1").fetchone()
        connection.execute("UPDATE capsule_asset SET content = ? WHERE path = ?", (bytes(row[1]) + b"\x00", row[0]))

    def add_trigger(connection: sqlite3.Connection) -> None:
        connection.execute(
            "CREATE TRIGGER hostile_trigger AFTER INSERT ON capsule_change_log "
            "BEGIN DELETE FROM capsule_change_log; END"
        )

    def add_invalid_endpoint(connection: sqlite3.Connection) -> None:
        connection.execute(
            "INSERT INTO capsule_endpoint "
            "(name, operation, sql_text, parameters_json, result_mode, description, enabled) "
            "VALUES ('hostile.pragma', 'read', 'PRAGMA user_version', '{}', 'scalar', "
            "'An intentionally invalid verifier fixture.', 1)"
        )

    def fail_application_check(connection: sqlite3.Connection) -> None:
        check_id = connection.execute(
            "SELECT id FROM capsule_check WHERE severity = 'error' ORDER BY id LIMIT 1"
        ).fetchone()[0]
        connection.execute(
            "UPDATE capsule_check SET expected_json = ? WHERE id = ?",
            (json.dumps("intentionally-wrong"), check_id),
        )

    for target, mutation in [
        (output, corrupt_asset),
        (output.parent / "invalid-trigger.html", add_trigger),
        (output.parent / "invalid-endpoint.html", add_invalid_endpoint),
        (output.parent / "invalid-check.html", fail_application_check),
    ]:
        _write_unverified_export(capsule, target, manifest, runtime, mutation)

    parity_capsule = output.parent / "parity.capsule.sqlite"
    _write_parity_capsule(capsule, parity_capsule)
    export_html(parity_capsule, output.parent / "parity-view.html", profile="view", replace=True)

    source_bytes = capsule.read_bytes()
    source_compressed = _gzip(source_bytes)
    oversized_metadata = _metadata(source_bytes, source_compressed, manifest, "view", runtime)
    oversized_metadata["source"]["bytes"] = MAX_CAPSULE_BYTES + 1
    oversized_metadata["current"]["uncompressed_bytes"] = MAX_CAPSULE_BYTES + 1
    (output.parent / "oversize-metadata.html").write_bytes(
        _render_html(oversized_metadata, source_compressed, runtime)
    )

    expanded = source_bytes + (b"\x00" * 1_048_576)
    overrun_compressed = _gzip(expanded)
    overrun_metadata = _metadata(source_bytes, source_compressed, manifest, "view", runtime)
    overrun_metadata["current"]["compressed_sha256"] = hashlib.sha256(overrun_compressed).hexdigest()
    overrun_metadata["current"]["compressed_bytes"] = len(overrun_compressed)
    (output.parent / "decompression-overrun.html").write_bytes(
        _render_html(overrun_metadata, overrun_compressed, runtime)
    )


if __name__ == "__main__":
    main()

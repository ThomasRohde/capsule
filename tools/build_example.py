#!/usr/bin/env python3
"""Reproducibly assemble the Diagram Studio SQLite Capsule example."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sqlite3
import sys
import tempfile
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[1]
EXAMPLE = ROOT / "examples" / "diagram-studio"
SOURCE = EXAMPLE / "source"
DEFAULT_OUTPUT = ROOT / "capsules" / "diagram-studio.capsule.sqlite"

if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase  # noqa: E402


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def compact_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def insert_mapping(
    connection: sqlite3.Connection,
    table: str,
    row: dict[str, Any],
    *,
    json_fields: Iterable[str] = (),
) -> None:
    prepared = dict(row)
    for field in json_fields:
        prepared[field] = compact_json(prepared[field])
    columns = list(prepared)
    placeholders = ", ".join(f":{column}" for column in columns)
    connection.execute(
        f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({placeholders})", prepared
    )


def embedded_documents() -> list[dict[str, Any]]:
    specs = [
        ("vision", "Vision", ROOT / "docs" / "vision.md", 10),
        ("architecture", "Architecture", ROOT / "docs" / "architecture.md", 20),
        ("format-contract", "Format contract", ROOT / "docs" / "format-contract.md", 30),
        ("security", "Security and trust model", ROOT / "docs" / "security.md", 40),
        ("authoring", "Authoring and distribution", ROOT / "docs" / "authoring.md", 45),
        ("example-about", "Diagram Studio example", SOURCE / "docs" / "about.md", 100),
        ("example-data-model", "Diagram Studio data model", SOURCE / "docs" / "data-model.md", 110),
        ("agent-operation", "Agent operation", SOURCE / "docs" / "agent-operation.md", 120),
    ]
    return [
        {
            "slug": slug,
            "title": title,
            "media_type": "text/markdown",
            "content": path.read_text(encoding="utf-8"),
            "sequence": sequence,
        }
        for slug, title, path, sequence in specs
    ]


def embedded_assets() -> list[dict[str, Any]]:
    specs = [
        (
            "app/index.html",
            "text/html; charset=utf-8",
            SOURCE / "app" / "index.html",
            1,
            "Diagram Studio application shell.",
        ),
        (
            "app/styles.css",
            "text/css; charset=utf-8",
            SOURCE / "app" / "styles.css",
            0,
            "Offline Diagram Studio visual design.",
        ),
        (
            "app/theme.js",
            "text/javascript; charset=utf-8",
            SOURCE / "app" / "theme.js",
            1,
            "Light, dark, and system theme selection and persistence.",
        ),
        (
            "app/capsule-client.js",
            "text/javascript; charset=utf-8",
            ROOT / "runtime" / "browser" / "capsule-client.js",
            1,
            "Host-neutral named-endpoint client for loopback and HTML browser hosts.",
        ),
        (
            "app/geometry.js",
            "text/javascript; charset=utf-8",
            SOURCE / "app" / "geometry.js",
            1,
            "Deterministic shape, transform, layout, port, and routing geometry.",
        ),
        (
            "app/interchange.js",
            "text/javascript; charset=utf-8",
            SOURCE / "app" / "interchange.js",
            1,
            "Versioned bounded interchange validation and standalone SVG serialisation.",
        ),
        (
            "app/app.js",
            "text/javascript; charset=utf-8",
            SOURCE / "app" / "app.js",
            1,
            "Diagram renderer, editor, scene player, and named-endpoint client.",
        ),
        (
            "bootstrap/capsule_host.py",
            "text/x-python; charset=utf-8",
            ROOT / "runtime" / "capsule_host.py",
            1,
            "Compatible standalone generic host for survival when only the database remains.",
        ),
        (
            "legal/LICENSE.txt",
            "text/plain; charset=utf-8",
            ROOT / "LICENSE",
            0,
            "Licence text for the bootstrap sources.",
        ),
    ]
    assets: list[dict[str, Any]] = []
    for path, media_type, source_path, executable, description in specs:
        content = source_path.read_bytes()
        assets.append(
            {
                "path": path,
                "media_type": media_type,
                "content": content,
                "sha256": sha256(content),
                "executable": executable,
                "cache_policy": "no-store",
                "description": description,
            }
        )
    return assets


def seed_platform(connection: sqlite3.Connection) -> None:
    manifest = load_json(SOURCE / "manifest.json")
    insert_mapping(
        connection,
        "capsule_manifest",
        manifest,
        json_fields=("permissions_json",),
    )

    for asset in embedded_assets():
        insert_mapping(connection, "capsule_asset", asset)

    for command in load_json(SOURCE / "commands.json"):
        insert_mapping(connection, "capsule_command", command, json_fields=("argv_json",))

    for runbook in load_json(SOURCE / "runbooks.json"):
        insert_mapping(connection, "capsule_runbook", runbook)

    for document in embedded_documents():
        insert_mapping(connection, "capsule_doc", document)

    for endpoint in load_json(SOURCE / "endpoints.json"):
        endpoint = dict(endpoint)
        steps = endpoint.pop("steps", [])
        insert_mapping(
            connection,
            "capsule_endpoint",
            endpoint,
            json_fields=("parameters_json",),
        )
        for step in steps:
            insert_mapping(
                connection,
                "capsule_endpoint_step",
                {"endpoint_name": endpoint["name"], **step},
            )

    for check in load_json(SOURCE / "checks.json"):
        insert_mapping(
            connection,
            "capsule_check",
            check,
            json_fields=("expected_json",),
        )

    for prompt in load_json(SOURCE / "prompts.json"):
        insert_mapping(connection, "capsule_prompt", prompt)


def seed_diagram(connection: sqlite3.Connection) -> None:
    data = load_json(SOURCE / "data" / "diagram.json")
    document = data["document"]
    insert_mapping(connection, "diagram_document", document)
    history = data.get(
        "history",
        {
            "diagram_id": document["id"],
            "cursor": 0,
            "tip": 0,
            "updated_at": document["updated_at"],
        },
    )
    insert_mapping(connection, "diagram_history", history)
    default_created = document["created_at"]
    default_updated = document["updated_at"]

    layers = data.get(
        "layers",
        [
            {
                "id": "layer-content",
                "diagram_id": document["id"],
                "name": "Content",
                "position": 1,
                "visible": 1,
                "locked": 0,
            }
        ],
    )
    for layer in layers:
        insert_mapping(
            connection,
            "diagram_layer",
            {
                **layer,
                "created_at": layer.get("created_at", default_created),
                "updated_at": layer.get("updated_at", default_updated),
            },
        )

    for node in data["nodes"]:
        prepared = {
            **node,
            "layer_id": node.get(
                "layer_id",
                "layer-background" if node.get("kind") == "container" else "layer-content",
            ),
            "created_at": node.get("created_at", default_created),
            "updated_at": node.get("updated_at", default_updated),
        }
        insert_mapping(
            connection,
            "diagram_node",
            prepared,
            json_fields=("style_json", "data_json"),
        )

    for edge in data["edges"]:
        prepared = {
            **edge,
            "layer_id": edge.get("layer_id", "layer-connectors"),
            "source_port": edge.get("source_port", "auto"),
            "target_port": edge.get("target_port", "auto"),
            "route_mode": edge.get("route_mode", "orthogonal"),
            "waypoints_json": edge.get("waypoints_json", []),
            "created_at": edge.get("created_at", default_created),
            "updated_at": edge.get("updated_at", default_updated),
        }
        insert_mapping(
            connection,
            "diagram_edge",
            prepared,
            json_fields=("style_json", "waypoints_json"),
        )

    for group in data.get("groups", []):
        members = group.get("members", [])
        prepared = {
            **{key: value for key, value in group.items() if key != "members"},
            "layer_id": group.get("layer_id", "layer-content"),
            "created_at": group.get("created_at", default_created),
            "updated_at": group.get("updated_at", default_updated),
        }
        insert_mapping(connection, "diagram_group", prepared)
        for position, node_id in enumerate(members, 1):
            insert_mapping(
                connection,
                "diagram_group_member",
                {"group_id": group["id"], "node_id": node_id, "position": position},
            )

    for scene in data["scenes"]:
        prepared = {
            **scene,
            "created_at": scene.get("created_at", default_created),
            "updated_at": scene.get("updated_at", default_updated),
        }
        insert_mapping(
            connection,
            "diagram_scene",
            prepared,
            json_fields=("viewport_json", "focus_json"),
        )

    for override in data.get("scene_overrides", []):
        insert_mapping(
            connection,
            "diagram_scene_override",
            override,
            json_fields=("style_json",),
        )


def build_example(output: Path = DEFAULT_OUTPUT) -> dict[str, Any]:
    output = output.expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    file_descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{output.name}.", suffix=".tmp", dir=output.parent
    )
    os.close(file_descriptor)
    temporary = Path(temporary_name)
    temporary.unlink(missing_ok=True)

    try:
        connection = sqlite3.connect(temporary)
        try:
            connection.execute("PRAGMA page_size = 4096")
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute("PRAGMA foreign_keys = ON")
            manifest = load_json(SOURCE / "manifest.json")
            if (
                manifest.get("format_id") != "org.sqlite-capsule"
                or manifest.get("format_version") != "0.2"
                or manifest.get("runtime_protocol") != "capsule-http/0.2"
            ):
                raise ValueError("The example manifest must use the current capsule v0.2 profile")
            format_schema = ROOT / "format" / "capsule-v0.2.sql"
            connection.executescript(format_schema.read_text())
            connection.executescript((EXAMPLE / "domain.sql").read_text())
            seed_platform(connection)
            seed_diagram(connection)
            connection.commit()
            connection.execute("VACUUM")
        finally:
            connection.close()

        with CapsuleDatabase(temporary, read_only=True) as capsule:
            verification = capsule.verify()
        if not verification["ok"]:
            raise RuntimeError(
                "Generated capsule failed verification: " + "; ".join(verification["errors"])
            )

        temporary.replace(output)
        result = {
            "ok": True,
            "output": str(output),
            "bytes": output.stat().st_size,
            "sha256": sha256(output.read_bytes()),
            "assets": len(embedded_assets()),
            "documents": len(embedded_documents()),
            "checks": verification["checks"],
        }
        return result
    finally:
        temporary.unlink(missing_ok=True)


def check_example(output: Path = DEFAULT_OUTPUT) -> dict[str, Any]:
    """Confirm that the checked distribution is exactly reproducible from source."""

    output = output.expanduser().resolve()
    if not output.is_file():
        return {"ok": False, "output": str(output), "error": "Distribution file is missing"}
    with tempfile.TemporaryDirectory() as directory:
        expected = Path(directory) / output.name
        build_example(expected)
        current_hash = sha256(output.read_bytes())
        expected_hash = sha256(expected.read_bytes())
        return {
            "ok": current_hash == expected_hash,
            "output": str(output),
            "current_sha256": current_hash,
            "expected_sha256": expected_hash,
            "bytes": output.stat().st_size,
        }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify that the existing output is byte-identical to a fresh build",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        result = check_example(args.output) if args.check else build_example(args.output)
    except Exception as exc:  # noqa: BLE001 - command-line boundary
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

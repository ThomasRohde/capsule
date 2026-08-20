#!/usr/bin/env python3
"""Create signed v0.3 working/release fixtures for the native M07 E2E."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CREATOR = (
    ROOT
    / "plugins"
    / "capsule-creator"
    / "skills"
    / "create-capsule"
    / "scripts"
    / "capsule_project.py"
)
DEVELOPMENT_SEED = (
    ROOT / "compatibility" / "signed-app-v0.2" / "development-seed.hex"
)
APP_ID = "org.sqlite-capsule.upgrade-fixture"
SCHEMA_ID = f"{APP_ID}.data"
SIGNED_AT = "2026-08-20T08:00:00Z"
PNG_1X1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
)


def run(command: list[str]) -> dict[str, object]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    channel = completed.stdout if completed.returncode == 0 else completed.stderr
    try:
        payload = json.loads(channel)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid JSON from {command[0]}: {channel}") from error
    if completed.returncode != 0 or payload.get("ok") is not True:
        raise RuntimeError(f"command failed: {command!r}: {payload!r}")
    return payload


def write_json(path: Path, value: object) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def configure_project(
    project: Path,
    *,
    app_version: str,
    capsule_id: str,
    revision_id: str,
    item_title: str,
    asset_marker: str,
) -> None:
    config_path = project / "capsule-project.json"
    config = json.loads(config_path.read_text(encoding="utf-8"))
    config.update(
        {
            "app_version": app_version,
            "capsule_id": capsule_id,
            "revision_id": revision_id,
            "data_schema_id": SCHEMA_ID,
            "created_at": "2026-08-20T08:00:00Z",
            "content_updated_at": "2026-08-20T08:00:00Z",
            "released_at": "2026-08-20T08:00:00Z",
        }
    )
    write_json(config_path, config)
    seed_path = project / "source" / "data" / "seed.json"
    seed = json.loads(seed_path.read_text(encoding="utf-8"))
    seed["item"][0].update(
        {
            "title": item_title,
            "created_at": "2026-08-20T08:00:00Z",
            "updated_at": "2026-08-20T08:00:00Z",
        }
    )
    write_json(seed_path, seed)
    app_js = project / "source" / "app" / "app.js"
    app_js.write_text(
        app_js.read_text(encoding="utf-8").rstrip()
        + f'\n;globalThis.__UPGRADE_RELEASE_MARKER__ = "{asset_marker}";\n',
        encoding="utf-8",
        newline="\n",
    )
    endpoints_path = project / "source" / "endpoints.json"
    endpoints = json.loads(endpoints_path.read_text(encoding="utf-8"))
    endpoints[0]["description"] = f"List items with {asset_marker}."
    write_json(endpoints_path, endpoints)


def sign(native_cli: Path, source: Path, output: Path) -> dict[str, object]:
    return run(
        [
            str(native_cli),
            "sign",
            str(source),
            str(output),
            "--publisher-id",
            "org.sqlite-capsule.m07-fixture",
            "--publisher-name",
            "SQLite Capsule M07 Fixture",
            "--key",
            str(DEVELOPMENT_SEED),
            "--signed-at",
            SIGNED_AT,
        ]
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def mutate_working_capsule(path: Path) -> None:
    icon_sha256 = hashlib.sha256(PNG_1X1).hexdigest()
    with sqlite3.connect(path) as connection:
        connection.execute(
            "UPDATE item SET title=?, completed=1, updated_at=? WHERE id='item-welcome'",
            ("USER DATA SENTINEL · preserved by M07", "2026-08-20T08:05:00Z"),
        )
        connection.execute(
            "INSERT INTO item (id,title,completed,created_at,updated_at) VALUES (?,?,?,?,?)",
            (
                "item-user",
                "SECOND USER SENTINEL",
                0,
                "2026-08-20T08:04:00Z",
                "2026-08-20T08:05:00Z",
            ),
        )
        connection.execute(
            "INSERT INTO capsule_instance_asset (id,media_type,content,sha256,width,height,description) VALUES (?,?,?,?,?,?,?)",
            ("instance-icon", "image/png", PNG_1X1, icon_sha256, 1, 1, "M07 user icon"),
        )
        connection.execute(
            "UPDATE capsule_instance SET title=?,description=?,document_kind=?,tags_json=?,icon_asset_id=?,content_updated_at=? WHERE id=1",
            (
                "M07 working document",
                "User-owned profile sentinel",
                "upgrade-test-document",
                json.dumps(["m07", "user-profile"], separators=(",", ":")),
                "instance-icon",
                "2026-08-20T08:05:00Z",
            ),
        )
        connection.commit()


def build_fixtures(native_cli: Path, destination: Path) -> dict[str, object]:
    native_cli = native_cli.resolve(strict=True)
    destination = destination.resolve()
    if destination.exists():
        raise FileExistsError(f"refusing to replace fixture directory: {destination}")
    destination.mkdir(parents=True)
    source_project = destination / "source-project"
    target_project = destination / "target-project"
    run(
        [
            sys.executable,
            str(CREATOR),
            "init",
            str(source_project),
            "--title",
            "M07 Upgrade Fixture",
            "--app-id",
            APP_ID,
            "--app-version",
            "1.0.0",
            "--format-version",
            "0.3",
            "--template",
        ]
    )
    shutil.copytree(source_project, target_project)
    configure_project(
        source_project,
        app_version="1.0.0",
        capsule_id="11111111-1111-4111-8111-111111111111",
        revision_id="11111111-1111-4111-8111-111111111112",
        item_title="SOURCE CLEAN SEED",
        asset_marker="SOURCE-RELEASE-ASSET",
    )
    configure_project(
        target_project,
        app_version="1.1.0",
        capsule_id="22222222-2222-4222-8222-222222222222",
        revision_id="22222222-2222-4222-8222-222222222223",
        item_title="TARGET CLEAN PRESET",
        asset_marker="TARGET-RELEASE-ASSET",
    )
    source_unsigned = destination / "working-unsigned.sqlitecapsule"
    target_unsigned = destination / "release-unsigned.sqlitecapsule"
    for project, output in (
        (source_project, source_unsigned),
        (target_project, target_unsigned),
    ):
        run(
            [
                sys.executable,
                str(CREATOR),
                "build",
                str(project),
                str(output),
                "--resource-root",
                str(ROOT),
            ]
        )
    working = destination / "working.sqlitecapsule"
    release = destination / "release.sqlitecapsule"
    source_sign = sign(native_cli, source_unsigned, working)
    target_sign = sign(native_cli, target_unsigned, release)
    mutate_working_capsule(working)
    with sqlite3.connect(release) as connection:
        release_marker = connection.execute(
            "SELECT CAST(content AS TEXT) FROM capsule_asset WHERE path='app/app.js'"
        ).fetchone()[0]
        target_digest = connection.execute(
            "SELECT lower(hex(application_digest)) FROM capsule_signature ORDER BY key_id LIMIT 1"
        ).fetchone()[0]
    if "TARGET-RELEASE-ASSET" not in release_marker:
        raise RuntimeError("target release asset marker is absent")
    return {
        "profile": "org.sqlite-capsule.native-upgrade-fixtures/1",
        "working": str(working),
        "release": str(release),
        "working_sha256": sha256_file(working),
        "release_sha256": sha256_file(release),
        "publisher_key_id": source_sign["key_id"],
        "same_publisher_key": source_sign["key_id"] == target_sign["key_id"],
        "target_application_digest": target_digest,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--native-cli", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        report = build_fixtures(arguments.native_cli, arguments.output)
    except Exception as error:  # noqa: BLE001 - fixture errors must be visible
        print(json.dumps({"ok": False, "error": str(error)}), file=sys.stderr)
        return 1
    print(json.dumps({"ok": True, **report}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

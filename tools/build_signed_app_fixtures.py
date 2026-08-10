#!/usr/bin/env python3
"""Build the hostile signed-app fixture matrix with a public test-only key."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sqlite3
import subprocess
from contextlib import closing
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "capsules" / "diagram-studio.capsule.sqlite"
TEST_SEED = ROOT / "compatibility" / "signed-app-v0.2" / "development-seed.hex"
DEFAULT_NATIVE = ROOT / "native" / "target" / "debug" / (
    "capsule-native.exe" if __import__("os").name == "nt" else "capsule-native"
)


def _run_json(command: list[str]) -> tuple[int, dict[str, Any]]:
    completed = subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    channel = completed.stdout if completed.returncode == 0 else completed.stderr
    try:
        payload = json.loads(channel)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"invalid JSON from {command[0]}") from exc
    return completed.returncode, payload


def _copy_and_mutate(
    source: Path,
    target: Path,
    mutation: Callable[[sqlite3.Connection], None],
) -> None:
    shutil.copyfile(source, target)
    with closing(sqlite3.connect(target)) as connection:
        mutation(connection)
        connection.commit()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def build_fixtures(native_cli: Path, output_directory: Path) -> dict[str, Any]:
    native_cli = native_cli.expanduser().resolve()
    output_directory = output_directory.expanduser().resolve()
    if not native_cli.is_file():
        raise FileNotFoundError(f"native CLI not found: {native_cli}")
    if output_directory.exists():
        raise FileExistsError(f"refusing to replace fixture directory: {output_directory}")
    output_directory.mkdir(parents=True)

    unsigned = output_directory / "unsigned.capsule.sqlite"
    shutil.copyfile(SOURCE, unsigned)
    valid = output_directory / "signed-valid.capsule.sqlite"
    return_code, sign_report = _run_json(
        [
            str(native_cli),
            "sign",
            str(SOURCE),
            str(valid),
            "--publisher-id",
            "org.sqlite-capsule.development",
            "--publisher-name",
            "SQLite Capsule Development Fixture",
            "--key",
            str(TEST_SEED),
            "--signed-at",
            "2026-08-08T12:34:56Z",
        ]
    )
    if return_code != 0 or sign_report.get("ok") is not True:
        raise RuntimeError(f"native signing failed: {sign_report!r}")

    unknown = output_directory / "signed-unknown-key.capsule.sqlite"
    shutil.copyfile(valid, unknown)

    _copy_and_mutate(
        valid,
        output_directory / "signed-wrong-key.capsule.sqlite",
        lambda connection: connection.execute(
            "UPDATE capsule_signature SET key_id = ?, public_key = ?",
            (
                "ed25519:sha256:" + hashlib.sha256(bytes([7]) * 32).hexdigest(),
                bytes([7]) * 32,
            ),
        ),
    )

    def stale_profile(connection: sqlite3.Connection) -> None:
        connection.execute("PRAGMA ignore_check_constraints=ON")
        connection.execute(
            "UPDATE capsule_publisher SET profile = 'org.sqlite-capsule.signed-app/0.0'"
        )

    _copy_and_mutate(
        valid,
        output_directory / "signed-stale-version.capsule.sqlite",
        stale_profile,
    )
    _copy_and_mutate(
        valid,
        output_directory / "signed-malformed.capsule.sqlite",
        lambda connection: connection.execute("DROP TABLE capsule_signature"),
    )
    _copy_and_mutate(
        valid,
        output_directory / "signed-code-mutated.capsule.sqlite",
        lambda connection: connection.execute(
            "UPDATE capsule_asset SET content = X'00' WHERE path = 'app/app.js'"
        ),
    )
    _copy_and_mutate(
        valid,
        output_directory / "signed-permission-mutated.capsule.sqlite",
        lambda connection: connection.execute(
            "UPDATE capsule_manifest SET permissions_json = ? WHERE id = 1",
            ('{"database.read":{"required":false}}',),
        ),
    )
    _copy_and_mutate(
        valid,
        output_directory / "signed-schema-mutated.capsule.sqlite",
        lambda connection: connection.execute(
            "CREATE INDEX fixture_title_idx ON diagram_document(title)"
        ),
    )
    _copy_and_mutate(
        valid,
        output_directory / "signed-data-only-mutated.capsule.sqlite",
        lambda connection: connection.execute(
            "UPDATE diagram_document SET title = title || ' (fixture edit)' "
            "WHERE id = 'diagram-main'"
        ),
    )

    expected = {
        "unsigned.capsule.sqlite": (0, False),
        "signed-valid.capsule.sqlite": (0, True),
        "signed-unknown-key.capsule.sqlite": (0, True),
        "signed-wrong-key.capsule.sqlite": (0, False),
        "signed-stale-version.capsule.sqlite": (1, None),
        "signed-malformed.capsule.sqlite": (1, None),
        "signed-code-mutated.capsule.sqlite": (0, False),
        "signed-permission-mutated.capsule.sqlite": (0, False),
        "signed-schema-mutated.capsule.sqlite": (0, False),
        "signed-data-only-mutated.capsule.sqlite": (0, True),
    }
    fixtures = []
    for name, (expected_return_code, expected_signature_valid) in expected.items():
        path = output_directory / name
        return_code, report = _run_json(
            [str(native_cli), "verify-signature", str(path)]
        )
        if return_code != expected_return_code:
            raise AssertionError(f"{name}: verifier exit {return_code} != {expected_return_code}")
        if expected_signature_valid is not None and report.get("signature_valid") is not expected_signature_valid:
            raise AssertionError(
                f"{name}: signature_valid {report.get('signature_valid')!r} "
                f"!= {expected_signature_valid!r}"
            )
        fixtures.append(
            {
                "name": name,
                "bytes": path.stat().st_size,
                "sha256": _sha256(path),
                "verifier_exit": return_code,
                "signature_valid": report.get("signature_valid"),
                "publisher_known": report.get("publisher_known"),
                "publisher_trusted": report.get("publisher_trusted"),
                "executable_allowed": report.get("executable_allowed"),
                "error": report.get("error"),
            }
        )

    manifest = {
        "format": "org.sqlite-capsule.signed-app-fixtures/0.2",
        "warning": "All keys and signatures are public development fixtures and confer no trust.",
        "native_cli": str(native_cli),
        "fixtures": fixtures,
    }
    (output_directory / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-cli", type=Path, default=DEFAULT_NATIVE)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=ROOT / "native" / "target" / "signed-app-fixtures",
    )
    arguments = parser.parse_args(argv)
    try:
        manifest = build_fixtures(arguments.native_cli, arguments.output_dir)
    except (AssertionError, FileExistsError, FileNotFoundError, OSError, RuntimeError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, indent=2, sort_keys=True))
        return 1
    print(json.dumps({"ok": True, **manifest}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

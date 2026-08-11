from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import tempfile
import unittest

from tools.sign_release import (
    ENV_KEY_FILE,
    ENV_KEY_HEX,
    ENV_NATIVE_CLI,
    ENV_OUTPUT,
    ENV_PUBLISHER_ID,
    ENV_PUBLISHER_NAME,
    ENV_SIGNED_AT,
    ENV_SOURCE,
    ReleaseSigningError,
    SigningSettings,
    materialized_key,
    sanitized_child_environment,
    settings_from_args,
    sign_release,
)


ROOT = Path(__file__).resolve().parents[1]
CAPSULE = ROOT / "capsules" / "diagram-studio.capsule.sqlite"
TEST_SEED = ROOT / "compatibility" / "signed-app-v0.2" / "development-seed.hex"
NATIVE_CANDIDATES = (
    ROOT / "native" / "target" / "release" / (
        "capsule-native.exe" if os.name == "nt" else "capsule-native"
    ),
    ROOT / "native" / "target" / "debug" / (
        "capsule-native.exe" if os.name == "nt" else "capsule-native"
    ),
)


def empty_arguments() -> argparse.Namespace:
    return argparse.Namespace(
        source=None,
        output=None,
        native_cli=None,
        publisher_id=None,
        publisher_name=None,
        key_file=None,
        signed_at=None,
    )


class ReleaseSigningTests(unittest.TestCase):
    def configuration(
        self,
        directory: Path,
        *,
        native_cli: Path | None = None,
        **overrides: str,
    ) -> dict[str, str]:
        if native_cli is None:
            native_cli = directory / (
                "capsule-native.exe" if os.name == "nt" else "capsule-native"
            )
            native_cli.write_bytes(b"configuration-only fixture")
        environment = {
            ENV_SOURCE: str(CAPSULE),
            ENV_OUTPUT: str(directory / "signed.sqlitecapsule"),
            ENV_NATIVE_CLI: str(native_cli),
            ENV_PUBLISHER_ID: "org.sqlite-capsule.automation-test",
            ENV_PUBLISHER_NAME: "SQLite Capsule Automation Test",
            ENV_KEY_FILE: str(TEST_SEED),
            ENV_SIGNED_AT: "2026-08-10T12:34:56Z",
        }
        environment.update(overrides)
        return environment

    def test_environment_resolves_complete_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            environment = self.configuration(Path(raw))
            settings = settings_from_args(empty_arguments(), environment)
        self.assertEqual(settings.source, CAPSULE.resolve())
        self.assertEqual(settings.publisher_id, environment[ENV_PUBLISHER_ID])
        self.assertEqual(settings.key_file, TEST_SEED.resolve())
        self.assertIsNone(settings.key_hex)
        self.assertEqual(settings.signed_at, environment[ENV_SIGNED_AT])

    def test_key_file_and_key_hex_are_mutually_exclusive(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            environment = self.configuration(Path(raw), **{ENV_KEY_HEX: "2a" * 32})
            with self.assertRaisesRegex(ReleaseSigningError, "mutually exclusive"):
                settings_from_args(empty_arguments(), environment)

    def test_hex_secret_is_private_temporary_input_and_not_in_child_env(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            settings = SigningSettings(
                source=CAPSULE,
                output=directory / "signed.sqlitecapsule",
                native_cli=directory / "capsule-native",
                publisher_id="org.example",
                publisher_name="Example",
                key_file=None,
                key_hex="2a" * 32,
                signed_at="2026-08-10T12:34:56Z",
            )
            child = sanitized_child_environment({ENV_KEY_HEX: settings.key_hex, "KEEP": "yes"})
            self.assertNotIn(ENV_KEY_HEX, child)
            self.assertEqual(child["KEEP"], "yes")
            with materialized_key(settings) as path:
                self.assertEqual(path.read_bytes(), bytes([42]) * 32)
                materialized = path
            self.assertFalse(materialized.exists())

    def test_generated_timestamp_uses_exact_utc_seconds(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            environment = self.configuration(Path(raw))
            environment.pop(ENV_SIGNED_AT)
            settings = settings_from_args(empty_arguments(), environment)
        self.assertRegex(settings.signed_at, re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"))

    def test_end_to_end_signing_when_native_cli_is_built(self) -> None:
        native = next((path for path in NATIVE_CANDIDATES if path.is_file()), None)
        if native is None:
            self.skipTest("capsule-native has not been built")
        with tempfile.TemporaryDirectory() as raw:
            environment = self.configuration(Path(raw), native_cli=native)
            settings = settings_from_args(empty_arguments(), environment)
            report = sign_release(settings, environment)
            self.assertTrue(report["ok"])
            self.assertTrue(report["signature_valid"])
            self.assertTrue(settings.output.is_file())

    def test_documentation_names_automation_wrapper_and_secret_policy(self) -> None:
        authoring = (ROOT / "docs" / "authoring.md").read_text(encoding="utf-8")
        self.assertIn("python tools/sign_release.py", authoring)
        self.assertIn(ENV_KEY_FILE, authoring)
        self.assertIn(ENV_KEY_HEX, authoring)
        self.assertIn("secret variable is not passed", authoring)
        self.assertIn("child processes", authoring)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import hashlib
import shutil
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest import mock

from tools.capsule_signatures import PROFILE, signature_inventory


ROOT = Path(__file__).resolve().parents[1]
CAPSULE = ROOT / "capsules" / "diagram-studio.capsule.sqlite"
SIGNED_SCHEMA = (ROOT / "format" / "capsule-signed-app-v0.2.sql").read_text(
    encoding="utf-8"
)


class CapsuleSignatureInventoryTests(unittest.TestCase):
    def copy_capsule(self, directory: Path) -> Path:
        target = directory / "fixture.capsule.sqlite"
        shutil.copyfile(CAPSULE, target)
        return target

    def test_unsigned_inventory_is_explicitly_non_authenticating(self) -> None:
        report = signature_inventory(CAPSULE)
        self.assertTrue(report["ok"])
        self.assertTrue(report["integrity_ok"])
        self.assertFalse(report["signature_extension_present"])
        self.assertIsNone(report["signature_valid"])
        self.assertFalse(report["publisher_known"])
        self.assertFalse(report["publisher_trusted"])
        self.assertEqual(report["revocation_status"], "not_checked")
        self.assertFalse(report["executable_allowed"])

    def test_partial_extension_is_reported_as_malformed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_directory:
            target = self.copy_capsule(Path(raw_directory))
            with closing(sqlite3.connect(target)) as connection:
                connection.execute(
                    "CREATE TABLE capsule_publisher ("
                    "id INTEGER PRIMARY KEY, profile TEXT NOT NULL, "
                    "publisher_id TEXT NOT NULL, publisher_name TEXT NOT NULL)"
                )
                connection.commit()
            report = signature_inventory(target)
        self.assertFalse(report["ok"])
        self.assertTrue(report["signature_extension_present"])
        self.assertFalse(report["signature_inventory_valid"])
        self.assertIn("signed-app extension is partial", report["errors"])

    def test_well_formed_rows_remain_unverified_without_native_cli(self) -> None:
        with tempfile.TemporaryDirectory() as raw_directory:
            target = self.copy_capsule(Path(raw_directory))
            public_key = bytes(range(32))
            key_id = "ed25519:sha256:" + hashlib.sha256(public_key).hexdigest()
            with closing(sqlite3.connect(target)) as connection:
                connection.executescript(SIGNED_SCHEMA)
                connection.execute(
                    "INSERT INTO capsule_publisher VALUES (1, ?, ?, ?)",
                    (PROFILE, "org.example", "Example Publisher"),
                )
                connection.execute(
                    "INSERT INTO capsule_signature VALUES (?, 'ed25519', ?, ?, ?, ?)",
                    (
                        key_id,
                        public_key,
                        bytes(32),
                        bytes(64),
                        "2026-08-08T12:34:56Z",
                    ),
                )
                connection.commit()
            report = signature_inventory(target)
        self.assertTrue(report["ok"])
        self.assertTrue(report["signature_inventory_valid"])
        self.assertIsNone(report["signature_valid"])
        self.assertIsNone(report["signatures"][0]["cryptographically_valid"])

    @mock.patch("tools.capsule_signatures.subprocess.run")
    def test_explicit_native_verifier_can_supply_crypto_results(self, run: mock.Mock) -> None:
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = Path(raw_directory)
            target = self.copy_capsule(directory)
            verifier = directory / "capsule-native"
            verifier.write_bytes(b"fixture")
            run.return_value = mock.Mock(
                returncode=0,
                stdout=(
                    '{"ok":true,"signature_valid":false,"publisher_known":false,'
                    '"publisher_trusted":false,"revocation_status":"not_checked",'
                    '"executable_allowed":false,"signatures":[]}'
                ),
                stderr="",
            )
            report = signature_inventory(target, native_verifier=verifier)
        self.assertTrue(report["native_verifier"]["invoked"])
        self.assertFalse(report["signature_valid"])
        run.assert_called_once()


if __name__ == "__main__":
    unittest.main()

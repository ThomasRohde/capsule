from __future__ import annotations

import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.build_example import build_example  # noqa: E402
from tools.capsule_conformance import DEFAULT_SPEC, check_conformance  # noqa: E402


class IndependentConformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temp_root = tempfile.TemporaryDirectory()
        cls.capsule = Path(cls.temp_root.name) / "diagram-studio.capsule.sqlite"
        build_example(cls.capsule)

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temp_root.cleanup()

    def copy_capsule(self) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        directory = tempfile.TemporaryDirectory()
        path = Path(directory.name) / "copy.capsule.sqlite"
        shutil.copy2(self.capsule, path)
        return directory, path

    def test_reference_capsule_matches_independent_spec(self) -> None:
        report = check_conformance(self.capsule)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["spec"], str(DEFAULT_SPEC.resolve()))

    def test_missing_required_column_is_rejected_independently(self) -> None:
        directory, path = self.copy_capsule()
        try:
            connection = sqlite3.connect(path)
            connection.execute("ALTER TABLE capsule_asset DROP COLUMN cache_policy")
            connection.commit()
            connection.close()
            report = check_conformance(path)
            self.assertFalse(report["ok"])
            self.assertTrue(any("cache_policy" in error for error in report["errors"]))
        finally:
            directory.cleanup()

    def test_identity_and_discovery_shape_are_rejected(self) -> None:
        directory, path = self.copy_capsule()
        try:
            connection = sqlite3.connect(path)
            connection.execute("PRAGMA application_id = 7")
            connection.execute("PRAGMA user_version = 1")
            connection.execute("DROP VIEW START_HERE")
            connection.execute("CREATE VIEW START_HERE AS SELECT 1 AS wrong_shape")
            connection.commit()
            connection.close()
            report = check_conformance(path)
            self.assertFalse(report["ok"])
            self.assertTrue(any("application_id" in error for error in report["errors"]))
            self.assertTrue(any("user_version" in error for error in report["errors"]))
            self.assertTrue(any("START_HERE" in error for error in report["errors"]))
        finally:
            directory.cleanup()

    def test_required_grant_table_is_enforced(self) -> None:
        directory, path = self.copy_capsule()
        try:
            connection = sqlite3.connect(path)
            connection.execute("DROP TABLE capsule_grant")
            connection.commit()
            connection.close()
            report = check_conformance(path)
            self.assertFalse(report["ok"])
            self.assertTrue(any("capsule_grant" in error for error in report["errors"]))
        finally:
            directory.cleanup()

    def test_repository_cli_exposes_independent_conformance_command(self) -> None:
        result = subprocess.run(
            [sys.executable, str(ROOT / "tools" / "capsule.py"), "conformance", str(self.capsule)],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(report["ok"], report)

    def test_security_preview_is_explicitly_non_executable(self) -> None:
        signed = json.loads(
            (ROOT / "format" / "capsule-signed-compartment-preview.json").read_text(encoding="utf-8")
        )
        self.assertEqual(signed["status"], "design-only")
        self.assertIn("immutable_application_compartment", signed)
        self.assertIn("mutable_data_compartment", signed)


if __name__ == "__main__":
    unittest.main()

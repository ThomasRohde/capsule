from __future__ import annotations

import hashlib
import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase  # noqa: E402
from tools.build_example import build_example  # noqa: E402
from tools.capsule_author import (  # noqa: E402
    AuthoringError,
    diff_capsules,
    pack_capsule,
    unpack_capsule,
)


class AuthoringRoundTripTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._class_temp = tempfile.TemporaryDirectory()
        cls.base_capsule = Path(cls._class_temp.name) / "diagram-studio.capsule.sqlite"
        build_example(cls.base_capsule)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._class_temp.cleanup()

    def test_unpack_pack_is_semantic_and_byte_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle = root / "bundle"
            first = root / "first.capsule.sqlite"
            second = root / "second.capsule.sqlite"

            unpack_result = unpack_capsule(self.base_capsule, bundle)
            self.assertTrue(unpack_result["ok"])
            metadata = json.loads((bundle / "capsule-unpack.json").read_text())
            self.assertEqual(metadata["bundle_format"], "org.sqlite-capsule.authoring-bundle/0.2")
            self.assertTrue((bundle / "assets" / "by-sha256").is_dir())

            pack_capsule(bundle, first)
            pack_capsule(bundle, second)
            comparison = diff_capsules(self.base_capsule, first)
            self.assertTrue(comparison["equal"], comparison)
            self.assertEqual(
                hashlib.sha256(first.read_bytes()).hexdigest(),
                hashlib.sha256(second.read_bytes()).hexdigest(),
            )

            cli = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "tools" / "capsule.py"),
                    "diff",
                    str(self.base_capsule),
                    str(first),
                ],
                check=False,
                capture_output=True,
                text=True,
                timeout=20,
            )
            self.assertEqual(cli.returncode, 0, cli.stdout + cli.stderr)
            self.assertTrue(json.loads(cli.stdout)["equal"])

    def test_runtime_edit_round_trips_and_diff_identifies_rows(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            edited = root / "edited.capsule.sqlite"
            edited.write_bytes(self.base_capsule.read_bytes())
            with CapsuleDatabase(edited) as capsule:
                capsule.execute_endpoint(
                    "node.move",
                    "write",
                    {
                        "operation_id": "operation-authoring-move",
                        "diagram_id": "diagram-main",
                        "expected_cursor": 0,
                        "id": "node-app-assets",
                        "from_x": 780.0,
                        "from_y": 470.0,
                        "to_x": 901.0,
                        "to_y": 502.0,
                    },
                )

            changed = diff_capsules(self.base_capsule, edited)
            self.assertFalse(changed["equal"])
            self.assertIn("diagram_node", changed["tables"])
            self.assertIn("capsule_change_log", changed["tables"])

            bundle = root / "edited-bundle"
            repacked = root / "repacked.capsule.sqlite"
            unpack_capsule(edited, bundle)
            pack_capsule(bundle, repacked)
            comparison = diff_capsules(edited, repacked)
            self.assertTrue(comparison["equal"], comparison)

    def test_authoring_tools_refuse_implicit_replacement(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle = root / "bundle"
            unpack_capsule(self.base_capsule, bundle)
            with self.assertRaises(AuthoringError):
                unpack_capsule(self.base_capsule, bundle)

            output = root / "output.capsule.sqlite"
            output.write_text("unrelated", encoding="utf-8")
            with self.assertRaises(AuthoringError):
                pack_capsule(bundle, output)
            self.assertEqual(output.read_text(encoding="utf-8"), "unrelated")

    def test_unpack_rejects_tables_that_depend_on_implicit_rowid_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capsule = root / "implicit-rowid.capsule.sqlite"
            capsule.write_bytes(self.base_capsule.read_bytes())
            connection = sqlite3.connect(capsule)
            connection.execute("CREATE TABLE implicit_identity (value TEXT NOT NULL)")
            connection.execute("INSERT INTO implicit_identity(value) VALUES ('one'), ('two')")
            connection.commit()
            connection.close()
            with self.assertRaisesRegex(AuthoringError, "no explicit primary key"):
                unpack_capsule(capsule, root / "bundle")

    def test_pack_rejects_triggers_and_virtual_tables_before_building(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle = root / "bundle"
            unpack_capsule(self.base_capsule, bundle)
            metadata_path = bundle / "capsule-unpack.json"
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))

            trigger = dict(metadata)
            trigger["schema"] = [
                *metadata["schema"],
                {
                    "type": "trigger",
                    "name": "hidden_side_effect",
                    "table": "diagram_node",
                    "sql": "CREATE TRIGGER hidden_side_effect AFTER UPDATE ON diagram_node BEGIN SELECT 1; END",
                },
            ]
            metadata_path.write_text(json.dumps(trigger), encoding="utf-8")
            with self.assertRaisesRegex(AuthoringError, "Triggers are forbidden"):
                pack_capsule(bundle, root / "trigger.sqlite")

            virtual = dict(metadata)
            virtual["schema"] = [
                *metadata["schema"],
                {
                    "type": "table",
                    "name": "search_index",
                    "table": "search_index",
                    "sql": "CREATE VIRTUAL TABLE search_index USING fts5(content)",
                },
            ]
            metadata_path.write_text(json.dumps(virtual), encoding="utf-8")
            with self.assertRaisesRegex(AuthoringError, "Virtual tables are forbidden"):
                pack_capsule(bundle, root / "virtual.sqlite")

    def test_repository_help_discovers_runtime_and_authoring_commands(self) -> None:
        result = subprocess.run(
            [sys.executable, str(ROOT / "tools" / "capsule.py"), "--help"],
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("start", result.stdout)
        self.assertIn("unpack", result.stdout)
        self.assertIn("diff", result.stdout)
        self.assertNotIn("migrate", result.stdout)

if __name__ == "__main__":
    unittest.main()

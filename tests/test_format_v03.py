from __future__ import annotations

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

from runtime.capsule_host import CapsuleDatabase, CapsuleError  # noqa: E402
from tools.capsule_conformance import check_conformance  # noqa: E402
from tools.capsule_signatures import signature_inventory  # noqa: E402


class FormatV03Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.capsule = Path(self.directory.name) / "vector-v03.capsule.sqlite"
        connection = sqlite3.connect(self.capsule)
        connection.executescript(
            (ROOT / "format" / "capsule-v0.3.sql").read_text(encoding="utf-8")
        )
        connection.executescript(
            (ROOT / "format" / "capsule-signed-app-v0.3.sql").read_text(
                encoding="utf-8"
            )
        )
        connection.executescript(
            (
                ROOT
                / "compatibility"
                / "signed-app-v0.3"
                / "fixture-v0.3.sql"
            ).read_text(encoding="utf-8")
        )
        connection.commit()
        connection.close()

    def tearDown(self) -> None:
        self.directory.cleanup()

    def test_python_inspection_and_conformance_dispatch_to_v03(self) -> None:
        with CapsuleDatabase(self.capsule, read_only=True) as capsule:
            report = capsule.verify()
            overview = capsule.overview()
        self.assertTrue(report["ok"], report)
        self.assertEqual(overview["application"]["app_id"], "org.sqlite-capsule.vector")
        self.assertEqual(
            overview["instance"]["revision_id"],
            "22222222-2222-4222-8222-222222222222",
        )
        self.assertEqual(overview["data_schema"]["version"], 2)
        independent = check_conformance(self.capsule)
        self.assertTrue(independent["ok"], independent)
        self.assertTrue(independent["spec"].endswith("capsule-v0.3.conformance.json"))

    def test_repository_cli_inspects_and_verifies_v03(self) -> None:
        for command in ("inspect", "verify"):
            result = subprocess.run(
                [sys.executable, str(ROOT / "tools" / "capsule.py"), command, str(self.capsule)],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(result.stdout)
            if command == "verify":
                self.assertEqual(payload["overview"]["format"]["version"], "0.3")
            else:
                self.assertEqual(payload["manifest"]["format_version"], "0.3")
                self.assertEqual(
                    payload["overview"]["application"]["app_id"],
                    "org.sqlite-capsule.vector",
                )
                self.assertEqual(
                    payload["overview"]["instance"]["capsule_id"],
                    "11111111-1111-4111-8111-111111111111",
                )
                self.assertEqual(
                    payload["overview"]["data_schema"],
                    {"id": "org.sqlite-capsule.vector-data", "version": 2},
                )

    def test_overview_and_cli_inspect_reject_hostile_v03_profile_mutations(self) -> None:
        mutations = {
            "application-id": "PRAGMA application_id = 7",
            "user-version": "PRAGMA user_version = 2",
            "missing-manifest": "DELETE FROM capsule_manifest",
            "format-id": "UPDATE capsule_manifest SET format_id = 'hostile' WHERE id = 1",
            "format-version": (
                "UPDATE capsule_manifest SET format_version = '9.9' WHERE id = 1"
            ),
            "runtime-protocol": (
                "UPDATE capsule_manifest SET runtime_protocol = 'hostile' WHERE id = 1"
            ),
            "minimum-host-profile": (
                "UPDATE capsule_manifest SET minimum_host_profile = 'hostile' WHERE id = 1"
            ),
            "missing-released-at": (
                "UPDATE capsule_manifest SET released_at = '' WHERE id = 1"
            ),
            "invalid-released-at": (
                "UPDATE capsule_manifest SET released_at = '2023-02-29T00:00:00Z' WHERE id = 1"
            ),
        }
        for name, sql in mutations.items():
            with self.subTest(name=name):
                connection = sqlite3.connect(self.capsule)
                try:
                    connection.execute("PRAGMA ignore_check_constraints=ON")
                    connection.execute(sql)
                    connection.commit()
                finally:
                    connection.close()

                with CapsuleDatabase(self.capsule, read_only=True) as capsule:
                    with self.assertRaises(CapsuleError):
                        capsule.overview()
                result = subprocess.run(
                    [
                        sys.executable,
                        str(ROOT / "tools" / "capsule.py"),
                        "inspect",
                        str(self.capsule),
                    ],
                    check=False,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                )
                self.assertEqual(result.returncode, 1, result.stdout or result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertFalse(json.loads(result.stderr)["ok"])
                self.tearDown()
                self.setUp()

    def test_overview_and_cli_inspect_reject_an_extra_manifest_row(self) -> None:
        alternate = Path(self.directory.name) / "extra-manifest.capsule.sqlite"
        schema = (ROOT / "format" / "capsule-v0.3.sql").read_text(encoding="utf-8")
        schema = schema.replace(
            "id                      INTEGER PRIMARY KEY CHECK (id = 1),",
            "id                      INTEGER PRIMARY KEY,",
            1,
        )
        connection = sqlite3.connect(alternate)
        connection.executescript(schema)
        connection.executescript(
            (ROOT / "format" / "capsule-signed-app-v0.3.sql").read_text(
                encoding="utf-8"
            )
        )
        connection.executescript(
            (
                ROOT
                / "compatibility"
                / "signed-app-v0.3"
                / "fixture-v0.3.sql"
            ).read_text(encoding="utf-8")
        )
        connection.execute(
            "INSERT INTO capsule_manifest SELECT 2, format_id, format_version, app_id, "
            "app_version, entry_asset, runtime_protocol, permissions_json, data_schema_id, "
            "data_schema_version, minimum_host_profile, released_at "
            "FROM capsule_manifest WHERE id = 1"
        )
        connection.commit()
        connection.close()

        with CapsuleDatabase(alternate, read_only=True) as capsule:
            with self.assertRaisesRegex(CapsuleError, "exactly one row"):
                capsule.overview()
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools" / "capsule.py"),
                "inspect",
                str(alternate),
            ],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(result.returncode, 1, result.stdout or result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertIn("exactly one row", json.loads(result.stderr)["error"])

    def test_overview_and_cli_inspect_reject_non_string_tags_without_traceback(self) -> None:
        connection = sqlite3.connect(self.capsule)
        connection.execute(
            "UPDATE capsule_instance SET tags_json = '[{\"private\":\"value\"}]' WHERE id = 1"
        )
        connection.commit()
        connection.close()

        with CapsuleDatabase(self.capsule, read_only=True) as capsule:
            with self.assertRaisesRegex(CapsuleError, "tags violate"):
                capsule.overview()
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools" / "capsule.py"),
                "inspect",
                str(self.capsule),
            ],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(result.returncode, 1, result.stdout or result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn("tags violate", json.loads(result.stderr)["error"])

    def test_signature_inventory_accepts_the_v03_profile_without_inferring_trust(self) -> None:
        report = signature_inventory(self.capsule)
        self.assertTrue(report["ok"], report)
        self.assertTrue(report["signature_extension_present"])
        self.assertEqual(report["publisher"]["id"], "org.example.vector")
        self.assertIsNone(report["signature_valid"])
        self.assertFalse(report["publisher_trusted"])

    def test_named_write_advances_revision_atomically(self) -> None:
        with CapsuleDatabase(self.capsule) as capsule:
            before = capsule.overview()["instance"]
            result = capsule.execute_endpoint(
                "vector.write", "write", {"value": "changed"}
            )
            after = capsule.overview()["instance"]
        self.assertEqual(result["step_changes"], [1, 1])
        self.assertNotEqual(before["revision_id"], after["revision_id"])
        self.assertRegex(
            after["revision_id"],
            r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
        )
        self.assertGreaterEqual(after["content_updated_at"], before["content_updated_at"])

    def test_all_bounded_identity_fields_and_exact_rows_fail_closed(self) -> None:
        mutations = {
            "application-row": ("DELETE FROM capsule_application", ()),
            "instance-row": ("DELETE FROM capsule_instance", ()),
            "application-id-bytes": (
                "UPDATE capsule_manifest SET app_id = ? WHERE id = 1",
                ("é" * 512,),
            ),
            "application-version-bytes": (
                "UPDATE capsule_manifest SET app_version = ? WHERE id = 1",
                ("a" + "é" * 127,),
            ),
            "application-version-shape": (
                "UPDATE capsule_manifest SET app_version = '!' WHERE id = 1",
                (),
            ),
            "application-name-bytes": (
                "UPDATE capsule_application SET name = ? WHERE id = 1",
                ("€" * 256,),
            ),
            "application-description-bytes": (
                "UPDATE capsule_application SET description = ? WHERE id = 1",
                ("€" * 4096,),
            ),
            "application-category-bytes": (
                "UPDATE capsule_application SET category = ? WHERE id = 1",
                ("€" * 128,),
            ),
            "application-icon": (
                "UPDATE capsule_asset SET media_type = 'image/svg+xml' WHERE path = 'app/icon.png'",
                (),
            ),
            "application-icon-reference-bytes": (
                "UPDATE capsule_application SET icon_asset = ? WHERE id = 1",
                ("é" * 1024,),
            ),
            "release-doc-reference-bytes": (
                "UPDATE capsule_application SET release_notes_doc = ? WHERE id = 1",
                ("é" * 1024,),
            ),
            "instance-asset-hash": (
                "UPDATE capsule_instance_asset SET sha256 = ? WHERE id = 'instance-icon'",
                ("0" * 64,),
            ),
            "instance-asset-bytes": (
                "UPDATE capsule_instance_asset SET content = zeroblob(524289) "
                "WHERE id = 'instance-icon'",
                (),
            ),
            "instance-asset-width": (
                "UPDATE capsule_instance_asset SET width = 1025 WHERE id = 'instance-icon'",
                (),
            ),
            "instance-asset-height": (
                "UPDATE capsule_instance_asset SET height = 0 WHERE id = 'instance-icon'",
                (),
            ),
            "capsule-uuid": (
                "UPDATE capsule_instance SET capsule_id = 'not-a-uuid' WHERE id = 1",
                (),
            ),
            "revision-uuid": (
                "UPDATE capsule_instance SET revision_id = 'not-a-uuid' WHERE id = 1",
                (),
            ),
            "instance-title-bytes": (
                "UPDATE capsule_instance SET title = ? WHERE id = 1",
                ("€" * 512,),
            ),
            "instance-description-bytes": (
                "UPDATE capsule_instance SET description = ? WHERE id = 1",
                ("€" * 8192,),
            ),
            "document-kind-shape": (
                "UPDATE capsule_instance SET document_kind = '../unsafe' WHERE id = 1",
                (),
            ),
            "duplicate-tags": (
                "UPDATE capsule_instance SET tags_json = '[\"same\",\"same\"]' WHERE id = 1",
                (),
            ),
            "tag-bytes": (
                "UPDATE capsule_instance SET tags_json = ? WHERE id = 1",
                (json.dumps(["€" * 128]),),
            ),
            "tag-count": (
                "UPDATE capsule_instance SET tags_json = ? WHERE id = 1",
                (json.dumps([f"tag-{index}" for index in range(65)]),),
            ),
            "instance-icon-reference-bytes": (
                "UPDATE capsule_instance SET icon_asset_id = ? WHERE id = 1",
                ("é" * 256,),
            ),
            "instance-cover-reference-bytes": (
                "UPDATE capsule_instance SET cover_asset_id = ? WHERE id = 1",
                ("é" * 256,),
            ),
            "released-at": (
                "UPDATE capsule_manifest SET released_at = '2023-02-29T00:00:00Z' WHERE id = 1",
                (),
            ),
            "created-at": (
                "UPDATE capsule_instance SET created_at = '2026-13-01T00:00:00Z' WHERE id = 1",
                (),
            ),
            "updated-at": (
                "UPDATE capsule_instance SET content_updated_at = '2026-01-01T24:00:00Z' WHERE id = 1",
                (),
            ),
            "data-schema-id-bytes": (
                "UPDATE capsule_manifest SET data_schema_id = ? WHERE id = 1",
                ("€" * 512,),
            ),
        }
        for name, (sql, parameters) in mutations.items():
            with self.subTest(name=name):
                connection = sqlite3.connect(self.capsule)
                connection.execute("PRAGMA ignore_check_constraints=ON")
                connection.execute(sql, parameters)
                connection.commit()
                connection.close()
                with CapsuleDatabase(self.capsule, read_only=True) as capsule:
                    report = capsule.verify()
                self.assertFalse(report["ok"], report)
                self.tearDown()
                self.setUp()

    def test_unknown_platform_objects_fail_closed(self) -> None:
        for name, sql in {
            "table": "CREATE TABLE capsule_unreviewed_table(id INTEGER)",
            "view": "CREATE VIEW capsule_unreviewed_view AS SELECT 1 AS value",
            "index": "CREATE INDEX capsule_unreviewed_index ON vector_domain(note)",
        }.items():
            with self.subTest(name=name):
                connection = sqlite3.connect(self.capsule)
                try:
                    connection.execute(sql)
                    connection.commit()
                finally:
                    connection.close()
                with CapsuleDatabase(self.capsule, read_only=True) as capsule:
                    report = capsule.verify()
                self.assertFalse(report["ok"], report)
                self.tearDown()
                self.setUp()

    def test_hidden_extra_identity_rows_fail_closed_without_schema_checks(self) -> None:
        alternate = Path(self.directory.name) / "extra-identity.capsule.sqlite"
        schema = (ROOT / "format" / "capsule-v0.3.sql").read_text(encoding="utf-8")
        schema = schema.replace(
            "INTEGER PRIMARY KEY CHECK (id = 1)",
            "INTEGER PRIMARY KEY",
        )
        connection = sqlite3.connect(alternate)
        connection.executescript(schema)
        connection.executescript(
            (ROOT / "format" / "capsule-signed-app-v0.3.sql").read_text(
                encoding="utf-8"
            )
        )
        connection.executescript(
            (
                ROOT
                / "compatibility"
                / "signed-app-v0.3"
                / "fixture-v0.3.sql"
            ).read_text(encoding="utf-8")
        )
        connection.execute(
            "INSERT INTO capsule_application "
            "SELECT 2, name, description, category, icon_asset, release_notes_doc "
            "FROM capsule_application WHERE id = 1"
        )
        connection.execute(
            "INSERT INTO capsule_instance "
            "SELECT 2, 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', "
            "'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', title, description, "
            "document_kind, tags_json, icon_asset_id, cover_asset_id, created_at, "
            "content_updated_at FROM capsule_instance WHERE id = 1"
        )
        connection.commit()
        connection.close()
        with CapsuleDatabase(alternate, read_only=True) as capsule:
            report = capsule.verify()
        self.assertFalse(report["ok"], report)

    def test_full_format_tuple_mismatch_fails_all_python_dispatchers(self) -> None:
        connection = sqlite3.connect(self.capsule)
        connection.execute("PRAGMA ignore_check_constraints=ON")
        connection.execute(
            "UPDATE capsule_manifest SET minimum_host_profile = 'unsupported' WHERE id = 1"
        )
        connection.commit()
        connection.close()
        with CapsuleDatabase(self.capsule, read_only=True) as capsule:
            self.assertFalse(capsule.verify()["ok"])
        self.assertFalse(signature_inventory(self.capsule)["ok"])

    def test_signature_inventory_uses_publisher_utf8_byte_bounds(self) -> None:
        connection = sqlite3.connect(self.capsule)
        connection.execute(
            "UPDATE capsule_publisher SET publisher_name = ? WHERE id = 1",
            ("😀" * 512,),
        )
        connection.commit()
        connection.close()
        report = signature_inventory(self.capsule)
        self.assertFalse(report["ok"], report)
        self.assertIn("capsule_publisher row is malformed", report["errors"])


if __name__ == "__main__":
    unittest.main()

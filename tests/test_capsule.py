from __future__ import annotations

import hashlib
import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.parse
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import (  # noqa: E402
    CapsuleError,
    CapsuleDatabase,
    CapsuleHTTPServer,
    EndpointError,
    MAX_CAPSULE_BYTES,
    MAX_CONCURRENT_REQUESTS,
    MAX_JSON_DEPTH,
    MAX_REQUEST_BYTES,
    MAX_RESULT_ROWS,
    encode_cursor_result,
    windows_current_principal,
    windows_state_identity_key,
)
from tools.build_example import DEFAULT_OUTPUT, build_example, check_example  # noqa: E402


class CapsuleFixture(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._class_temp = tempfile.TemporaryDirectory()
        cls.base_capsule = Path(cls._class_temp.name) / "diagram-studio.capsule.sqlite"
        build_example(cls.base_capsule)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._class_temp.cleanup()

    def writable_copy(self) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        directory = tempfile.TemporaryDirectory()
        path = Path(directory.name) / "copy.capsule.sqlite"
        shutil.copy2(self.base_capsule, path)
        return directory, path


class CapsuleBuildTests(CapsuleFixture):
    def test_generated_capsule_verifies_and_contains_full_boot_information(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            report = capsule.verify()
            manifest = capsule.manifest()
            documents = {document["slug"] for document in capsule.documents()}
            assets = {asset["path"] for asset in capsule.list_assets()}
            start_rows = capsule._connection.execute("SELECT * FROM START_HERE").fetchall()
            runbook = "\n".join(step["body_md"] for step in capsule.runbooks("agent"))

        self.assertTrue(report["ok"], report)
        self.assertEqual(manifest["format_id"], "org.sqlite-capsule")
        self.assertEqual(manifest["app_id"], "org.sqlite-capsule.diagram-studio")
        self.assertIn("vision", documents)
        self.assertIn("architecture", documents)
        self.assertIn("agent-operation", documents)
        self.assertIn("app/index.html", assets)
        self.assertIn("app/theme.js", assets)
        self.assertIn("bootstrap/capsule_host.py", assets)
        self.assertGreaterEqual(len(start_rows), 6)
        self.assertTrue(any(row["command_template"] for row in start_rows))
        self.assertTrue(all(row["risk_class"] for row in start_rows if row["command_id"]))
        self.assertIn("`format_version` is `0.2`", runbook)
        self.assertIn("`PRAGMA user_version` is `2`", runbook)
        self.assertIn("`capsule-http/0.2`", runbook)

    def test_embedded_host_matches_repository_runtime_and_runs_verification(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            extracted = Path(directory) / "capsule_host.py"
            with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
                capsule.extract_asset("bootstrap/capsule_host.py", extracted)
            expected = (ROOT / "runtime" / "capsule_host.py").read_bytes()
            self.assertEqual(extracted.read_bytes(), expected)
            result = subprocess.run(
                [sys.executable, str(extracted), "verify", str(self.base_capsule)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(json.loads(result.stdout)["ok"])

    def test_embedded_app_keeps_optional_presentation_and_clear_camera_feedback(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            asset = capsule.asset("app/app.js")
            self.assertIsNotNone(asset)
            script = asset.content.decode("utf-8")

        self.assertNotIn('addEventListener("fullscreenchange"', script)
        self.assertNotIn("exitPresentation(false)", script)
        self.assertIn("`cursor ${Math.round(point.x)}, ${Math.round(point.y)}", script)
        self.assertIn("`view ${Math.round(state.viewBox.x)}, ${Math.round(state.viewBox.y)}", script)

    def test_repeated_builds_are_byte_identical(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.sqlite"
            second = Path(directory) / "second.sqlite"
            build_example(first)
            build_example(second)
            first_hash = hashlib.sha256(first.read_bytes()).hexdigest()
            second_hash = hashlib.sha256(second.read_bytes()).hexdigest()
            self.assertEqual(first_hash, second_hash)

    def test_checked_in_distribution_is_fresh(self) -> None:
        result = check_example(DEFAULT_OUTPUT)
        self.assertTrue(result["ok"], result)

    def test_oversized_capsule_is_rejected_before_open(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        with path.open("ab") as output:
            output.truncate(MAX_CAPSULE_BYTES + 1)
        with self.assertRaises(CapsuleError):
            CapsuleDatabase(path)

    def test_result_row_limit_is_enforced(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.execute("CREATE TABLE rows (value TEXT)")
        connection.executemany("INSERT INTO rows VALUES (?)", ((str(index),) for index in range(MAX_RESULT_ROWS + 1)))
        cursor = connection.execute("SELECT value FROM rows")
        with self.assertRaises(EndpointError):
            encode_cursor_result(cursor, "rows")
        connection.close()

    def test_fine_grained_permission_grants_are_optional_and_inspectable(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        with CapsuleDatabase(path) as capsule:
            initial = capsule.permissions()
            self.assertEqual(initial["effective"]["network"]["decision"], "prompt")
            capsule._connection.execute(
                "INSERT INTO capsule_grant (capability, decision, reason, granted_at) "
                "VALUES (?, ?, ?, ?)",
                ("fullscreen", "deny", "Not needed for this session", "2026-08-08T00:00:00Z"),
            )
            capsule._connection.commit()
            updated = capsule.permissions()
        self.assertEqual(updated["grants"]["fullscreen"]["decision"], "deny")
        self.assertEqual(updated["effective"]["fullscreen"]["reason"], "Not needed for this session")

    def test_standalone_database_only_boot_protocol(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            directory_path = Path(directory)
            capsule_path = directory_path / "only-file.capsule.sqlite"
            shutil.copy2(self.base_capsule, capsule_path)
            cache = directory_path / "fresh-cache"
            extracted = cache / "capsule_host.py"
            connection = sqlite3.connect(capsule_path)
            command_rows = {
                row[0]: json.loads(row[1])
                for row in connection.execute(
                    "SELECT id, argv_json FROM capsule_command "
                    "WHERE id IN ('extract.embedded','verify.embedded','start.embedded',"
                    "'status.embedded','stop.embedded')"
                ).fetchall()
            }
            asset_hash = connection.execute(
                "SELECT sha256 FROM capsule_asset WHERE path = 'bootstrap/capsule_host.py'"
            ).fetchone()[0]
            connection.close()

            substitutions = {
                "{python}": sys.executable,
                "{capsule}": str(capsule_path),
                "{cache}": str(cache),
                "{cache}/capsule_host.py": str(extracted),
            }

            def arguments(command_id: str) -> list[str]:
                return [substitutions.get(value, value) for value in command_rows[command_id]]

            extract = subprocess.run(
                arguments("extract.embedded"),
                check=False,
                capture_output=True,
                text=True,
                timeout=15,
            )
            self.assertEqual(extract.returncode, 0, extract.stderr)
            self.assertEqual(hashlib.sha256(extracted.read_bytes()).hexdigest(), asset_hash)

            environment = dict(
                **__import__("os").environ,
                SQLITE_CAPSULE_STATE_DIR=str(directory_path / "standalone-state"),
            )
            verify = subprocess.run(
                arguments("verify.embedded"),
                env=environment,
                check=False,
                capture_output=True,
                text=True,
                timeout=15,
            )
            self.assertEqual(verify.returncode, 0, verify.stderr)
            self.assertTrue(json.loads(verify.stdout)["ok"])

            start = subprocess.run(
                arguments("start.embedded"),
                env=environment,
                check=False,
                capture_output=True,
                text=True,
                timeout=15,
            )
            self.assertEqual(start.returncode, 0, start.stderr)
            start_result = json.loads(start.stdout)
            self.assertTrue(start_result["health"]["ok"])
            self.assertNotIn("shutdown_token", start_result)
            try:
                status = subprocess.run(
                    arguments("status.embedded"),
                    env=environment,
                    check=False,
                    capture_output=True,
                    text=True,
                    timeout=15,
                )
                self.assertEqual(status.returncode, 0, status.stderr)
                status_result = json.loads(status.stdout)
                self.assertTrue(status_result["running"])
                self.assertNotIn("shutdown_token", status_result["state"])
            finally:
                stop = subprocess.run(
                    arguments("stop.embedded"),
                    env=environment,
                    check=False,
                    capture_output=True,
                    text=True,
                    timeout=15,
                )
            self.assertEqual(stop.returncode, 0, stop.stderr)
            self.assertFalse(json.loads(stop.stdout)["running"])

    def test_extract_host_refuses_unrelated_existing_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "capsule_host.py"
            target.write_text("unrelated", encoding="utf-8")
            with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
                with self.assertRaises(CapsuleError):
                    capsule.extract_asset("bootstrap/capsule_host.py", target)
            self.assertEqual(target.read_text(encoding="utf-8"), "unrelated")


class VerificationAdversarialTests(CapsuleFixture):
    def mutate(self, sql_text: str, parameters: tuple[object, ...] = ()) -> Path:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        connection = sqlite3.connect(path)
        connection.execute(sql_text, parameters)
        connection.commit()
        connection.close()
        return path

    def assert_verification_error(self, path: Path, fragment: str) -> None:
        with CapsuleDatabase(path, read_only=True) as capsule:
            report = capsule.verify()
        self.assertFalse(report["ok"], report)
        self.assertTrue(
            any(fragment in error for error in report["errors"]),
            report["errors"],
        )

    def test_missing_required_column_is_rejected(self) -> None:
        path = self.mutate(
            "ALTER TABLE capsule_asset RENAME COLUMN cache_policy TO cache_policy_missing"
        )
        self.assert_verification_error(path, "missing columns: cache_policy")

    def test_incompatible_start_here_projection_is_rejected(self) -> None:
        path = self.mutate("DROP VIEW START_HERE")
        connection = sqlite3.connect(path)
        connection.execute("CREATE VIEW START_HERE AS SELECT 1 AS wrong")
        connection.commit()
        connection.close()
        self.assert_verification_error(path, "START_HERE has incompatible columns")

    def test_unsupported_user_version_is_rejected(self) -> None:
        path = self.mutate("PRAGMA user_version = 999")
        self.assert_verification_error(path, "Unsupported user_version")

    def test_endpoint_placeholder_mismatch_is_rejected(self) -> None:
        path = self.mutate(
            "UPDATE capsule_endpoint SET sql_text = 'SELECT :undeclared', "
            "parameters_json = '{}' WHERE name = 'diagram.get'"
        )
        self.assert_verification_error(path, "uses undeclared parameters")

    def test_pragma_endpoint_is_rejected(self) -> None:
        path = self.mutate(
            "UPDATE capsule_endpoint SET sql_text = 'PRAGMA user_version', "
            "parameters_json = '{}' WHERE name = 'diagram.get'"
        )
        self.assert_verification_error(path, "begins with disallowed 'PRAGMA'")

    def test_triggers_are_rejected(self) -> None:
        path = self.mutate(
            "CREATE TRIGGER hidden_side_effect AFTER UPDATE ON diagram_node "
            "BEGIN UPDATE diagram_document SET updated_at = CURRENT_TIMESTAMP; END"
        )
        self.assert_verification_error(path, "Triggers are not permitted")

    def test_unsafe_asset_header_is_rejected_and_csp_survives(self) -> None:
        path = self.mutate(
            "UPDATE capsule_asset SET media_type = ? WHERE path = 'app/index.html'",
            ("text/html\r\n\r\n",),
        )
        self.assert_verification_error(path, "unsafe media type")
        capsule = CapsuleDatabase(path)
        server = CapsuleHTTPServer(("127.0.0.1", 0), capsule, "token", quiet=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(capsule.close)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        base = f"http://127.0.0.1:{server.server_address[1]}"
        with self.assertRaises(urllib.error.HTTPError) as caught:
            urllib.request.urlopen(base + "/")
        self.assertEqual(caught.exception.code, 500)
        self.assertIn("default-src 'none'", caught.exception.headers["Content-Security-Policy"])
        server.shutdown()
        thread.join(timeout=5)

    def test_internal_serve_cannot_bind_off_loopback(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools" / "capsule.py"),
                "_serve",
                str(self.base_capsule),
                "--host",
                "0.0.0.0",
                "--port",
                "8765",
                "--shutdown-token",
                "test-token",
                "--trust-capsule",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("loopback", result.stderr)


class EndpointTests(CapsuleFixture):
    def test_read_endpoint_decodes_json_columns(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            nodes = capsule.execute_endpoint(
                "diagram.nodes", "read", {"diagram_id": "diagram-main"}
            )
        self.assertGreaterEqual(len(nodes), 10)
        self.assertIsInstance(nodes[0]["style_json"], dict)
        self.assertIsInstance(nodes[0]["data_json"], dict)

    def test_named_write_persists_after_reopen(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        with CapsuleDatabase(path) as capsule:
            result = capsule.execute_endpoint(
                "node.move",
                "write",
                {
                    "operation_id": "operation-endpoint-move",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "id": "node-app-assets",
                    "from_x": 780.0,
                    "from_y": 470.0,
                    "to_x": 812.0,
                    "to_y": 493.0,
                },
            )
            self.assertEqual(result["changes"], 3)
            self.assertEqual(result["step_changes"], [0, 1, 1, 1])
        with CapsuleDatabase(path, read_only=True) as capsule:
            nodes = capsule.execute_endpoint(
                "diagram.nodes", "read", {"diagram_id": "diagram-main"}
            )
            node = next(item for item in nodes if item["id"] == "node-app-assets")
            log_count = capsule._connection.execute(
                "SELECT count(*) FROM capsule_change_log WHERE endpoint_name = 'node.move'"
            ).fetchone()[0]
        self.assertEqual((node["x"], node["y"]), (812.0, 493.0))
        self.assertEqual(log_count, 1)

    def test_durable_node_move_undo_and_redo_survive_reopen(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        operation_id = "operation-durable-move"
        move = {
            "operation_id": operation_id,
            "diagram_id": "diagram-main",
            "expected_cursor": 0,
            "id": "node-app-assets",
            "from_x": 780.0,
            "from_y": 470.0,
            "to_x": 864.0,
            "to_y": 516.0,
        }
        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint("node.move", "write", move)

        with CapsuleDatabase(path) as capsule:
            history = capsule.execute_endpoint(
                "diagram.history", "read", {"diagram_id": "diagram-main"}
            )
            self.assertEqual(history["cursor"], 1)
            self.assertEqual(history["undo_operation_id"], operation_id)
            capsule.execute_endpoint(
                "node.move.undo",
                "write",
                {
                    "operation_id": operation_id,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )

        with CapsuleDatabase(path) as capsule:
            undone_node = next(
                node
                for node in capsule.execute_endpoint(
                    "diagram.nodes", "read", {"diagram_id": "diagram-main"}
                )
                if node["id"] == "node-app-assets"
            )
            history = capsule.execute_endpoint(
                "diagram.history", "read", {"diagram_id": "diagram-main"}
            )
            self.assertEqual((undone_node["x"], undone_node["y"]), (780.0, 470.0))
            self.assertEqual(history["cursor"], 0)
            self.assertEqual(history["redo_operation_id"], operation_id)
            capsule.execute_endpoint(
                "node.move.redo",
                "write",
                {
                    "operation_id": operation_id,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                },
            )

        with CapsuleDatabase(path, read_only=True) as capsule:
            redone_node = next(
                node
                for node in capsule.execute_endpoint(
                    "diagram.nodes", "read", {"diagram_id": "diagram-main"}
                )
                if node["id"] == "node-app-assets"
            )
            history = capsule.execute_endpoint(
                "diagram.history", "read", {"diagram_id": "diagram-main"}
            )
            operation = capsule._connection.execute(
                "SELECT state, sequence FROM diagram_operation WHERE id = ?",
                (operation_id,),
            ).fetchone()
        self.assertEqual((redone_node["x"], redone_node["y"]), (864.0, 516.0))
        self.assertEqual(history["cursor"], 1)
        self.assertEqual(tuple(operation), ("applied", 1))

    def test_create_rename_resize_and_redo_branch_are_durable(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        node_id = "node-history-test"
        base = {
            "id": node_id,
            "diagram_id": "diagram-main",
            "kind": "note",
            "label": "History test",
            "x": 100.0,
            "y": 120.0,
            "width": 240.0,
            "height": 125.0,
            "z_index": 99,
            "style_json": {"fill": "#172554"},
            "data_json": {"description": "Durable operation fixture"},
        }
        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "node.create",
                "write",
                {
                    **base,
                    "operation_id": "operation-create",
                    "expected_cursor": 0,
                },
            )
            capsule.execute_endpoint(
                "node.rename",
                "write",
                {
                    "operation_id": "operation-rename",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                    "id": node_id,
                    "from_label": "History test",
                    "to_label": "Renamed history test",
                },
            )
            capsule.execute_endpoint(
                "node.resize",
                "write",
                {
                    "operation_id": "operation-resize",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 2,
                    "id": node_id,
                    "from_width": 240.0,
                    "from_height": 125.0,
                    "to_width": 312.0,
                    "to_height": 168.0,
                },
            )

        with CapsuleDatabase(path) as capsule:
            history = capsule.execute_endpoint(
                "diagram.history", "read", {"diagram_id": "diagram-main"}
            )
            self.assertEqual((history["cursor"], history["tip"]), (3, 3))
            capsule.execute_endpoint(
                history["undo_endpoint"],
                "write",
                {
                    "operation_id": history["undo_operation_id"],
                    "diagram_id": "diagram-main",
                    "expected_cursor": 3,
                },
            )

        with CapsuleDatabase(path) as capsule:
            node = capsule._connection.execute(
                "SELECT label, width, height FROM diagram_node WHERE id = ?", (node_id,)
            ).fetchone()
            self.assertEqual(tuple(node), ("Renamed history test", 240.0, 125.0))
            capsule.execute_endpoint(
                "node.rename",
                "write",
                {
                    "operation_id": "operation-branch",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 2,
                    "id": node_id,
                    "from_label": "Renamed history test",
                    "to_label": "Branched history test",
                },
            )

        with CapsuleDatabase(path, read_only=True) as capsule:
            history = capsule.execute_endpoint(
                "diagram.history", "read", {"diagram_id": "diagram-main"}
            )
            operations = capsule._connection.execute(
                "SELECT id, sequence, state FROM diagram_operation ORDER BY sequence"
            ).fetchall()
            node = capsule._connection.execute(
                "SELECT label, width, height FROM diagram_node WHERE id = ?", (node_id,)
            ).fetchone()
        self.assertEqual((history["cursor"], history["tip"]), (3, 3))
        self.assertIsNone(history["redo_operation_id"])
        self.assertEqual(
            [tuple(row) for row in operations],
            [
                ("operation-create", 1, "applied"),
                ("operation-rename", 2, "applied"),
                ("operation-branch", 3, "applied"),
            ],
        )
        self.assertEqual(tuple(node), ("Branched history test", 240.0, 125.0))

    def test_legacy_single_node_delete_is_absent(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            endpoint = capsule._connection.execute(
                "SELECT enabled FROM capsule_endpoint WHERE name = 'node.delete'"
            ).fetchone()
        self.assertIsNone(endpoint)

    def test_every_example_model_write_uses_atomic_steps(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            writes = capsule._connection.execute(
                "SELECT e.name, count(s.sequence) AS step_count "
                "FROM capsule_endpoint e "
                "LEFT JOIN capsule_endpoint_step s ON s.endpoint_name = e.name "
                "WHERE e.operation = 'write' GROUP BY e.name ORDER BY e.name"
            ).fetchall()
        self.assertGreaterEqual(len(writes), 20)
        self.assertTrue(all(row["step_count"] >= 3 for row in writes), [tuple(row) for row in writes])

    def test_multi_node_transform_is_atomic_and_reversible(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        changes = [
            {
                "id": "node-app-assets",
                "from": {"x": 780.0, "y": 470.0, "width": 240.0, "height": 120.0},
                "to": {"x": 800.0, "y": 500.0, "width": 264.0, "height": 144.0},
            },
            {
                "id": "node-domain-data",
                "from": {"x": 1090.0, "y": 470.0, "width": 240.0, "height": 120.0},
                "to": {"x": 1110.0, "y": 500.0, "width": 264.0, "height": 144.0},
            },
        ]
        with CapsuleDatabase(path) as capsule:
            result = capsule.execute_endpoint(
                "nodes.transform",
                "write",
                {
                    "operation_id": "operation-multi-transform",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "summary": "Transform two nodes",
                    "changes_json": changes,
                },
            )
            transformed = capsule._connection.execute(
                "SELECT id, x, y, width, height FROM diagram_node "
                "WHERE id IN ('node-app-assets', 'node-domain-data') ORDER BY id"
            ).fetchall()
            self.assertEqual(result["step_changes"], [0, 2, 1, 1])
            self.assertEqual(
                [tuple(row) for row in transformed],
                [
                    ("node-app-assets", 800.0, 500.0, 264.0, 144.0),
                    ("node-domain-data", 1110.0, 500.0, 264.0, 144.0),
                ],
            )

        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "nodes.transform.undo",
                "write",
                {
                    "operation_id": "operation-multi-transform",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )
            restored = capsule._connection.execute(
                "SELECT id, x, y, width, height FROM diagram_node "
                "WHERE id IN ('node-app-assets', 'node-domain-data') ORDER BY id"
            ).fetchall()
        self.assertEqual(
            [tuple(row) for row in restored],
            [
                ("node-app-assets", 780.0, 470.0, 240.0, 120.0),
                ("node-domain-data", 1090.0, 470.0, 240.0, 120.0),
            ],
        )

    def test_selected_delete_restores_layers_routes_groups_and_scene_references(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        operation_id = "operation-semantic-delete"
        with CapsuleDatabase(path) as capsule:
            capsule._connection.execute(
                "INSERT INTO diagram_group "
                "(id, diagram_id, layer_id, name, z_index, locked, created_at, updated_at) "
                "VALUES ('group-delete-test', 'diagram-main', 'layer-content', 'Delete fixture', 1, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
            )
            capsule._connection.execute(
                "INSERT INTO diagram_group_member (group_id, node_id, position) "
                "VALUES ('group-delete-test', 'node-app-assets', 1)"
            )
            capsule._connection.execute(
                "INSERT INTO diagram_scene_override "
                "(scene_id, node_id, x, y, width, height, visible, style_json) "
                "VALUES ('scene-one-file', 'node-app-assets', 804, 492, 252, 132, 1, '{\"emphasis\":\"strong\"}')"
            )
            capsule._connection.commit()
            node_before = capsule._connection.execute(
                "SELECT layer_id, z_index FROM diagram_node WHERE id = 'node-app-assets'"
            ).fetchone()
            edges_before = capsule._connection.execute(
                "SELECT id, layer_id, source_id, target_id, source_port, target_port, route_mode, waypoints_json "
                "FROM diagram_edge WHERE source_id = 'node-app-assets' OR target_id = 'node-app-assets' ORDER BY id"
            ).fetchall()
            focus_before = capsule._connection.execute(
                "SELECT focus_json FROM diagram_scene WHERE id = 'scene-one-file'"
            ).fetchone()[0]
            capsule.execute_endpoint(
                "nodes.delete",
                "write",
                {
                    "operation_id": operation_id,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "node_ids_json": ["node-app-assets"],
                },
            )
            self.assertTrue(capsule.verify()["ok"])
            focus_deleted = json.loads(
                capsule._connection.execute(
                    "SELECT focus_json FROM diagram_scene WHERE id = 'scene-one-file'"
                ).fetchone()[0]
            )
            self.assertNotIn("node-app-assets", focus_deleted)

        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "nodes.delete.undo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 1},
            )
            node_after = capsule._connection.execute(
                "SELECT layer_id, z_index FROM diagram_node WHERE id = 'node-app-assets'"
            ).fetchone()
            edges_after = capsule._connection.execute(
                "SELECT id, layer_id, source_id, target_id, source_port, target_port, route_mode, waypoints_json "
                "FROM diagram_edge WHERE source_id = 'node-app-assets' OR target_id = 'node-app-assets' ORDER BY id"
            ).fetchall()
            member = capsule._connection.execute(
                "SELECT group_id, node_id, position FROM diagram_group_member "
                "WHERE group_id = 'group-delete-test'"
            ).fetchone()
            override = capsule._connection.execute(
                "SELECT x, y, width, height, visible, style_json FROM diagram_scene_override "
                "WHERE scene_id = 'scene-one-file' AND node_id = 'node-app-assets'"
            ).fetchone()
            focus_after = capsule._connection.execute(
                "SELECT focus_json FROM diagram_scene WHERE id = 'scene-one-file'"
            ).fetchone()[0]
            self.assertTrue(capsule.verify()["ok"])
        self.assertEqual(tuple(node_after), tuple(node_before))
        self.assertEqual([tuple(row) for row in edges_after], [tuple(row) for row in edges_before])
        self.assertEqual(tuple(member), ("group-delete-test", "node-app-assets", 1))
        self.assertEqual(tuple(override), (804.0, 492.0, 252.0, 132.0, 1, '{"emphasis":"strong"}'))
        self.assertEqual(json.loads(focus_after), json.loads(focus_before))

        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "nodes.delete.redo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 0},
            )
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_node WHERE id = 'node-app-assets'"
                ).fetchone()
            )
            self.assertTrue(capsule.verify()["ok"])

    def test_edge_delete_restores_all_semantic_fields_across_reopen(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        operation_id = "operation-edge-delete"
        edge_id = "edge-assets-browser"
        fields = (
            "id, diagram_id, layer_id, source_id, target_id, kind, label, source_port, "
            "target_port, route_mode, waypoints_json, style_json"
        )
        with CapsuleDatabase(path) as capsule:
            before = capsule._connection.execute(
                f"SELECT {fields} FROM diagram_edge WHERE id = ?", (edge_id,)
            ).fetchone()
            self.assertIsNotNone(before)
            capsule.execute_endpoint(
                "edge.delete",
                "write",
                {
                    "operation_id": operation_id,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "id": edge_id,
                },
            )
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_edge WHERE id = ?", (edge_id,)
                ).fetchone()
            )

        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "edge.delete.undo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 1},
            )
            restored = capsule._connection.execute(
                f"SELECT {fields} FROM diagram_edge WHERE id = ?", (edge_id,)
            ).fetchone()
            self.assertEqual(tuple(restored), tuple(before))
            self.assertTrue(capsule.verify()["ok"])

        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "edge.delete.redo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 0},
            )
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_edge WHERE id = ?", (edge_id,)
                ).fetchone()
            )
            self.assertTrue(capsule.verify()["ok"])

    def test_layer_and_group_commands_are_durable_and_reversible(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        layer_operation = "operation-layer-visibility"
        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "layer.update",
                "write",
                {
                    "operation_id": layer_operation,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "layer_id": "layer-content",
                    "from_visible": 1,
                    "from_locked": 0,
                    "to_visible": 0,
                    "to_locked": 1,
                },
            )
        with CapsuleDatabase(path) as capsule:
            layer = capsule._connection.execute(
                "SELECT visible, locked FROM diagram_layer WHERE id = 'layer-content'"
            ).fetchone()
            self.assertEqual(tuple(layer), (0, 1))
            capsule.execute_endpoint(
                "layer.update.undo",
                "write",
                {
                    "operation_id": layer_operation,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )
            capsule.execute_endpoint(
                "group.toggle",
                "write",
                {
                    "operation_id": "operation-group-pair",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "action": "group",
                    "group_id": "group-test-pair",
                    "layer_id": "layer-content",
                    "name": "Test pair",
                    "node_ids_json": ["node-app-assets", "node-domain-data"],
                },
            )
        with CapsuleDatabase(path) as capsule:
            members = capsule._connection.execute(
                "SELECT node_id, position FROM diagram_group_member "
                "WHERE group_id = 'group-test-pair' ORDER BY position"
            ).fetchall()
            self.assertEqual(
                [tuple(row) for row in members],
                [("node-app-assets", 1), ("node-domain-data", 2)],
            )
            capsule.execute_endpoint(
                "group.toggle.undo",
                "write",
                {
                    "operation_id": "operation-group-pair",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )
        with CapsuleDatabase(path) as capsule:
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_group WHERE id = 'group-test-pair'"
                ).fetchone()
            )
            capsule.execute_endpoint(
                "group.toggle.redo",
                "write",
                {
                    "operation_id": "operation-group-pair",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                },
            )
        with CapsuleDatabase(path, read_only=True) as capsule:
            self.assertEqual(
                capsule._connection.execute(
                    "SELECT count(*) FROM diagram_group_member "
                    "WHERE group_id = 'group-test-pair'"
                ).fetchone()[0],
                2,
            )
            self.assertTrue(capsule.verify()["ok"])

    def test_layer_order_and_node_structure_commands_round_trip(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        before_layers = [
            {"id": "layer-background", "position": 1},
            {"id": "layer-connectors", "position": 2},
            {"id": "layer-content", "position": 3},
        ]
        after_layers = [
            {"id": "layer-background", "position": 1},
            {"id": "layer-content", "position": 2},
            {"id": "layer-connectors", "position": 3},
        ]
        with CapsuleDatabase(path) as capsule:
            capsule.execute_endpoint(
                "layers.reorder",
                "write",
                {
                    "operation_id": "operation-layer-reorder",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "before_json": before_layers,
                    "after_json": after_layers,
                },
            )
        with CapsuleDatabase(path) as capsule:
            order = capsule._connection.execute(
                "SELECT id FROM diagram_layer ORDER BY position"
            ).fetchall()
            self.assertEqual(
                [row[0] for row in order],
                ["layer-background", "layer-content", "layer-connectors"],
            )
            capsule.execute_endpoint(
                "layers.reorder.undo",
                "write",
                {
                    "operation_id": "operation-layer-reorder",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )
            capsule.execute_endpoint(
                "nodes.structure",
                "write",
                {
                    "operation_id": "operation-node-layer",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "summary": "Move node to connectors layer",
                    "changes_json": [
                        {
                            "id": "node-app-assets",
                            "from": {"layer_id": "layer-content", "z_index": 10},
                            "to": {"layer_id": "layer-connectors", "z_index": 30},
                        }
                    ],
                },
            )
        with CapsuleDatabase(path) as capsule:
            structured = capsule._connection.execute(
                "SELECT layer_id, z_index FROM diagram_node WHERE id = 'node-app-assets'"
            ).fetchone()
            self.assertEqual(tuple(structured), ("layer-connectors", 30))
            capsule.execute_endpoint(
                "nodes.structure.undo",
                "write",
                {
                    "operation_id": "operation-node-layer",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 1,
                },
            )
        with CapsuleDatabase(path, read_only=True) as capsule:
            restored = capsule._connection.execute(
                "SELECT layer_id, z_index FROM diagram_node WHERE id = 'node-app-assets'"
            ).fetchone()
            layer_order = capsule._connection.execute(
                "SELECT id FROM diagram_layer ORDER BY position"
            ).fetchall()
            self.assertTrue(capsule.verify()["ok"])
        self.assertEqual(tuple(restored), ("layer-content", 10))
        self.assertEqual(
            [row[0] for row in layer_order],
            ["layer-background", "layer-connectors", "layer-content"],
        )

    def test_connector_configuration_round_trips_ports_and_route_intent(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        operation_id = "operation-configure-edge"
        with CapsuleDatabase(path) as capsule:
            edge = capsule._connection.execute(
                "SELECT id, source_id, target_id, source_port, target_port, route_mode "
                "FROM diagram_edge ORDER BY id LIMIT 1"
            ).fetchone()
            parameters = {
                "operation_id": operation_id,
                "diagram_id": "diagram-main",
                "expected_cursor": 0,
                "id": edge["id"],
                "from_source_id": edge["source_id"],
                "from_target_id": edge["target_id"],
                "from_source_port": edge["source_port"],
                "from_target_port": edge["target_port"],
                "from_route_mode": edge["route_mode"],
                "to_source_id": edge["source_id"],
                "to_target_id": edge["target_id"],
                "to_source_port": "east",
                "to_target_port": "west",
                "to_route_mode": "direct",
            }
            capsule.execute_endpoint("edge.configure", "write", parameters)
        with CapsuleDatabase(path) as capsule:
            configured = capsule._connection.execute(
                "SELECT source_port, target_port, route_mode FROM diagram_edge WHERE id = ?",
                (edge["id"],),
            ).fetchone()
            self.assertEqual(tuple(configured), ("east", "west", "direct"))
            capsule.execute_endpoint(
                "edge.configure.undo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 1},
            )
        with CapsuleDatabase(path, read_only=True) as capsule:
            restored = capsule._connection.execute(
                "SELECT source_port, target_port, route_mode FROM diagram_edge WHERE id = ?",
                (edge["id"],),
            ).fetchone()
        self.assertEqual(
            tuple(restored),
            (edge["source_port"], edge["target_port"], edge["route_mode"]),
        )

    def test_scene_sequence_and_override_round_trip(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        operation_id = "operation-scene-capture"
        with CapsuleDatabase(path) as capsule:
            before = capsule.execute_endpoint(
                "diagram.scenes", "read", {"diagram_id": "diagram-main"}
            )
            after = json.loads(json.dumps(before))
            after[0]["title"] = "Captured overview"
            after[0]["overrides_json"] = [
                {
                    "node_id": "node-app-assets",
                    "x": 804.0,
                    "y": 492.0,
                    "width": 252.0,
                    "height": 132.0,
                    "visible": 1,
                    "style_json": {"emphasis": "strong"},
                }
            ]
            capsule.execute_endpoint(
                "scenes.apply",
                "write",
                {
                    "operation_id": operation_id,
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "summary": "Capture scene override",
                    "before_json": before,
                    "after_json": after,
                },
            )
        with CapsuleDatabase(path) as capsule:
            scene = capsule._connection.execute(
                "SELECT title FROM diagram_scene WHERE id = ?", (before[0]["id"],)
            ).fetchone()
            override = capsule._connection.execute(
                "SELECT node_id, x, y, width, height, visible FROM diagram_scene_override "
                "WHERE scene_id = ?",
                (before[0]["id"],),
            ).fetchone()
            self.assertEqual(scene["title"], "Captured overview")
            self.assertEqual(tuple(override), ("node-app-assets", 804.0, 492.0, 252.0, 132.0, 1))
            capsule.execute_endpoint(
                "scenes.apply.undo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 1},
            )
        with CapsuleDatabase(path) as capsule:
            self.assertEqual(
                capsule._connection.execute(
                    "SELECT title FROM diagram_scene WHERE id = ?", (before[0]["id"],)
                ).fetchone()[0],
                before[0]["title"],
            )
            capsule.execute_endpoint(
                "scenes.apply.redo",
                "write",
                {"operation_id": operation_id, "diagram_id": "diagram-main", "expected_cursor": 0},
            )
        with CapsuleDatabase(path, read_only=True) as capsule:
            self.assertEqual(
                capsule._connection.execute(
                    "SELECT count(*) FROM diagram_scene_override WHERE scene_id = ?",
                    (before[0]["id"],),
                ).fetchone()[0],
                1,
            )
            self.assertTrue(capsule.verify()["ok"])

    def test_interchange_import_rolls_back_dangling_input_and_round_trips_valid_input(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        valid_document = {
            "format": "org.sqlite-capsule.diagram-studio/1",
            "layers": [],
            "nodes": [
                {
                    "id": "node-import-test",
                    "kind": "note",
                    "label": "Imported atomically",
                    "x": 120.0,
                    "y": 160.0,
                    "width": 240.0,
                    "height": 125.0,
                    "z_index": 100,
                    "style_json": {},
                    "data_json": {"shape": "note"},
                }
            ],
            "edges": [],
            "groups": [],
            "scenes": [],
        }
        dangling_document = json.loads(json.dumps(valid_document))
        dangling_document["edges"] = [
            {
                "id": "edge-import-dangling",
                "source_id": "node-import-test",
                "target_id": "missing-node",
            }
        ]
        cross_layer_group = json.loads(json.dumps(valid_document))
        cross_layer_group["groups"] = [
            {
                "id": "group-import-cross-layer",
                "layer_id": "layer-connectors",
                "name": "Invalid cross-layer group",
                "member_ids": ["node-import-test"],
            }
        ]
        dangling_scene = json.loads(json.dumps(valid_document))
        dangling_scene["scenes"] = [
            {
                "id": "scene-import-dangling",
                "title": "Invalid scene",
                "viewport_json": {"x": 0, "y": 0, "zoom": 1},
                "focus_json": ["missing-node"],
                "overrides_json": [{"node_id": "missing-node", "visible": 1}],
            }
        ]
        with CapsuleDatabase(path) as capsule:
            for suffix, invalid_document in (
                ("dangling", dangling_document),
                ("cross-layer", cross_layer_group),
                ("scene-reference", dangling_scene),
            ):
                with self.subTest(suffix=suffix):
                    with self.assertRaisesRegex(EndpointError, "step 2 changed 0 rows"):
                        capsule.execute_endpoint(
                            "diagram.import",
                            "write",
                            {
                                "operation_id": f"operation-import-{suffix}",
                                "diagram_id": "diagram-main",
                                "expected_cursor": 0,
                                "document_json": invalid_document,
                            },
                        )
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_node WHERE id = 'node-import-test'"
                ).fetchone()
            )
            capsule.execute_endpoint(
                "diagram.import",
                "write",
                {
                    "operation_id": "operation-import-valid",
                    "diagram_id": "diagram-main",
                    "expected_cursor": 0,
                    "document_json": valid_document,
                },
            )
        with CapsuleDatabase(path) as capsule:
            self.assertIsNotNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_node WHERE id = 'node-import-test'"
                ).fetchone()
            )
            capsule.execute_endpoint(
                "diagram.import.undo",
                "write",
                {"operation_id": "operation-import-valid", "diagram_id": "diagram-main", "expected_cursor": 1},
            )
        with CapsuleDatabase(path) as capsule:
            self.assertIsNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_node WHERE id = 'node-import-test'"
                ).fetchone()
            )
            capsule.execute_endpoint(
                "diagram.import.redo",
                "write",
                {"operation_id": "operation-import-valid", "diagram_id": "diagram-main", "expected_cursor": 0},
            )
        with CapsuleDatabase(path, read_only=True) as capsule:
            self.assertIsNotNone(
                capsule._connection.execute(
                    "SELECT id FROM diagram_node WHERE id = 'node-import-test'"
                ).fetchone()
            )
            self.assertTrue(capsule.verify()["ok"])

    def test_stale_node_move_rolls_back_model_and_history(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        with CapsuleDatabase(path) as capsule:
            with self.assertRaisesRegex(EndpointError, "step 3 changed 0 rows; expected 1"):
                capsule.execute_endpoint(
                    "node.move",
                    "write",
                    {
                        "operation_id": "operation-stale-move",
                        "diagram_id": "diagram-main",
                        "expected_cursor": 7,
                        "id": "node-app-assets",
                        "from_x": 780.0,
                        "from_y": 470.0,
                        "to_x": 900.0,
                        "to_y": 600.0,
                    },
                )
            node = capsule._connection.execute(
                "SELECT x, y FROM diagram_node WHERE id = 'node-app-assets'"
            ).fetchone()
            history = capsule._connection.execute(
                "SELECT cursor, tip FROM diagram_history WHERE diagram_id = 'diagram-main'"
            ).fetchone()
            operation_count = capsule._connection.execute(
                "SELECT count(*) FROM diagram_operation"
            ).fetchone()[0]
        self.assertEqual(tuple(node), (780.0, 470.0))
        self.assertEqual(tuple(history), (0, 0))
        self.assertEqual(operation_count, 0)

    def test_endpoint_rejects_missing_and_unknown_parameters(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            with self.assertRaises(EndpointError):
                capsule.execute_endpoint("diagram.nodes", "read", {})
            with self.assertRaises(EndpointError):
                capsule.execute_endpoint(
                    "diagram.nodes",
                    "read",
                    {"diagram_id": "diagram-main", "unexpected": "value"},
                )

    def test_endpoint_rejects_null_for_required_parameter(self) -> None:
        with CapsuleDatabase(self.base_capsule, read_only=True) as capsule:
            with self.assertRaises(EndpointError):
                capsule.execute_endpoint("diagram.nodes", "read", {"diagram_id": None})

    def test_authorizer_protects_capsule_control_tables(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        connection = sqlite3.connect(path)
        connection.execute(
            "INSERT INTO capsule_endpoint "
            "(name, operation, sql_text, parameters_json, result_mode, description, enabled) "
            "VALUES (?, ?, ?, ?, ?, ?, 1)",
            (
                "malicious.control-delete",
                "write",
                "DELETE FROM capsule_manifest WHERE id = 1",
                "{}",
                "changes",
                "Test-only protected-table mutation.",
            ),
        )
        connection.commit()
        connection.close()
        with CapsuleDatabase(path) as capsule:
            with self.assertRaises(EndpointError):
                capsule.execute_endpoint("malicious.control-delete", "write", {})
            self.assertEqual(capsule.manifest()["id"], 1)


class CompoundEndpointTests(CapsuleFixture):
    def add_compound_fixture(self, path: Path, *, failing: bool = False) -> str:
        endpoint_name = "counter.increment-failing" if failing else "counter.increment"
        first_sql = "UPDATE test_counter SET value = value + :amount WHERE id = 'main'"
        second_sql = (
            "UPDATE test_audit SET amount = amount WHERE id = 'missing'"
            if failing
            else "INSERT INTO test_audit (id, amount) VALUES ('audit-' || :amount, :amount)"
        )
        connection = sqlite3.connect(path)
        connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS test_counter (
                id TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS test_audit (
                id TEXT PRIMARY KEY,
                amount INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO test_counter (id, value) VALUES ('main', 0);
            """
        )
        connection.execute(
            "INSERT INTO capsule_endpoint "
            "(name, operation, sql_text, parameters_json, result_mode, description, enabled) "
            "VALUES (?, 'write', ?, ?, 'changes', ?, 1)",
            (
                endpoint_name,
                first_sql,
                json.dumps({"amount": {"type": "integer", "required": True}}),
                "Test-only atomic two-step counter command.",
            ),
        )
        connection.executemany(
            "INSERT INTO capsule_endpoint_step "
            "(endpoint_name, sequence, sql_text, required_changes) VALUES (?, ?, ?, ?)",
            (
                (endpoint_name, 1, first_sql, 1),
                (endpoint_name, 2, second_sql, 1),
            ),
        )
        connection.commit()
        connection.close()
        return endpoint_name

    def test_compound_endpoint_commits_once_and_logs_once(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        endpoint_name = self.add_compound_fixture(path)

        with CapsuleDatabase(path) as capsule:
            report = capsule.verify()
            self.assertTrue(report["ok"], report)
            result = capsule.execute_endpoint(endpoint_name, "write", {"amount": 3})
            counter = capsule._connection.execute(
                "SELECT value FROM test_counter WHERE id = 'main'"
            ).fetchone()[0]
            audit = capsule._connection.execute(
                "SELECT id, amount FROM test_audit"
            ).fetchall()
            log = capsule._connection.execute(
                "SELECT endpoint_name, changed_rows FROM capsule_change_log "
                "WHERE endpoint_name = ?",
                (endpoint_name,),
            ).fetchall()

        self.assertEqual(result["changes"], 2)
        self.assertEqual(result["step_changes"], [1, 1])
        self.assertEqual(counter, 3)
        self.assertEqual([tuple(row) for row in audit], [("audit-3", 3)])
        self.assertEqual([tuple(row) for row in log], [(endpoint_name, 2)])

    def test_compound_endpoint_rolls_back_on_row_count_mismatch(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        endpoint_name = self.add_compound_fixture(path, failing=True)

        with CapsuleDatabase(path) as capsule:
            report = capsule.verify()
            self.assertTrue(report["ok"], report)
            with self.assertRaisesRegex(EndpointError, "step 2 changed 0 rows; expected 1"):
                capsule.execute_endpoint(endpoint_name, "write", {"amount": 4})
            counter = capsule._connection.execute(
                "SELECT value FROM test_counter WHERE id = 'main'"
            ).fetchone()[0]
            log_count = capsule._connection.execute(
                "SELECT count(*) FROM capsule_change_log WHERE endpoint_name = ?",
                (endpoint_name,),
            ).fetchone()[0]

        self.assertEqual(counter, 0)
        self.assertEqual(log_count, 0)

class DetachedLifecycleTests(CapsuleFixture):
    def test_windows_default_state_identity_is_principal_scoped_and_path_safe(self) -> None:
        sandbox = windows_state_identity_key("Flounder\\CodexSandboxOffline")
        user = windows_state_identity_key("Flounder\\thoma")
        self.assertRegex(sandbox, r"^win-v2-[0-9a-f]{16}$")
        self.assertRegex(user, r"^win-v2-[0-9a-f]{16}$")
        self.assertNotEqual(sandbox, user)

    @unittest.skipUnless(__import__("os").name == "nt", "Windows token test")
    def test_windows_principal_comes_from_the_process_token(self) -> None:
        principal = windows_current_principal()
        self.assertIn("\\", principal)
        expected_directory = f"sqlite-capsule-state-{windows_state_identity_key(principal)}"
        environment_principal = (
            f"{__import__('os').environ.get('USERDOMAIN', '')}\\"
            f"{__import__('os').environ.get('USERNAME', '')}"
        )
        if principal.casefold() != environment_principal.casefold():
            self.assertNotEqual(
                windows_state_identity_key(principal),
                windows_state_identity_key(environment_principal),
            )
        self.assertRegex(expected_directory, r"^sqlite-capsule-state-win-v2-[0-9a-f]{16}$")

    def test_start_status_and_stop_use_capsule_specific_state(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        state_dir = Path(directory.name) / "state"
        environment = dict(**__import__("os").environ, SQLITE_CAPSULE_STATE_DIR=str(state_dir))
        command = [sys.executable, str(ROOT / "tools" / "capsule.py")]

        start = subprocess.run(
            command + ["start", str(path), "--trust-capsule"],
            env=environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(start.returncode, 0, start.stderr)
        start_result = json.loads(start.stdout)
        self.assertTrue(start_result["health"]["ok"])
        self.assertNotIn("shutdown_token", start_result)
        try:
            status = subprocess.run(
                command + ["status", str(path)],
                env=environment,
                check=False,
                capture_output=True,
                text=True,
                timeout=15,
            )
            self.assertEqual(status.returncode, 0, status.stderr)
            self.assertTrue(json.loads(status.stdout)["running"])
            self.assertNotIn("shutdown_token", json.loads(status.stdout)["state"])
        finally:
            stop = subprocess.run(
                command + ["stop", str(path)],
                env=environment,
                check=False,
                capture_output=True,
                text=True,
                timeout=15,
            )
        self.assertEqual(stop.returncode, 0, stop.stderr)
        self.assertFalse(json.loads(stop.stdout)["running"])


class HttpSmokeTests(CapsuleFixture):
    def test_concurrency_limit_returns_service_unavailable(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        capsule = CapsuleDatabase(path)
        server = CapsuleHTTPServer(("127.0.0.1", 0), capsule, "shutdown-test-token", quiet=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(capsule.close)
        self.addCleanup(server.server_close)
        self.addCleanup(thread.join, 5)
        self.addCleanup(server.shutdown)
        held = [server._request_slots.acquire(blocking=False) for _ in range(MAX_CONCURRENT_REQUESTS)]
        self.assertTrue(all(held))
        try:
            with self.assertRaises(urllib.error.HTTPError) as error:
                urllib.request.urlopen(
                    f"http://127.0.0.1:{server.server_address[1]}/", timeout=2
                )
            self.assertEqual(error.exception.code, 503)
        finally:
            for acquired in held:
                if acquired:
                    server._request_slots.release()

    def test_browser_shell_read_and_write_protocol(self) -> None:
        directory, path = self.writable_copy()
        self.addCleanup(directory.cleanup)
        capsule = CapsuleDatabase(path)
        server = CapsuleHTTPServer(("127.0.0.1", 0), capsule, "shutdown-test-token", quiet=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(capsule.close)
        self.addCleanup(server.shutdown)
        base = f"http://127.0.0.1:{server.server_address[1]}"

        with urllib.request.urlopen(base + "/__capsule/manifest") as response:
            manifest_response = json.loads(response.read().decode("utf-8"))
        token = manifest_response["session_token"]
        self.assertEqual(
            manifest_response["manifest"]["app_id"], "org.sqlite-capsule.diagram-studio"
        )

        query = urllib.parse.urlencode({"diagram_id": "diagram-main"})
        request = urllib.request.Request(
            base + "/__capsule/read/diagram.nodes?" + query,
            headers={"X-Capsule-Token": token},
        )
        with urllib.request.urlopen(request) as response:
            nodes_response = json.loads(response.read().decode("utf-8"))
        self.assertTrue(nodes_response["ok"])
        self.assertGreaterEqual(len(nodes_response["result"]), 10)

        permissions_request = urllib.request.Request(
            base + "/__capsule/permissions", headers={"X-Capsule-Token": token}
        )
        with urllib.request.urlopen(permissions_request) as response:
            permissions_response = json.loads(response.read().decode("utf-8"))
        self.assertTrue(permissions_response["ok"])
        self.assertEqual(
            permissions_response["permissions"]["effective"]["network"]["decision"], "prompt"
        )

        payload = json.dumps(
            {
                "operation_id": "operation-http-move",
                "diagram_id": "diagram-main",
                "expected_cursor": 0,
                "id": "node-domain-data",
                "from_x": 1090.0,
                "from_y": 470.0,
                "to_x": 1104.0,
                "to_y": 482.0,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            base + "/__capsule/write/node.move",
            method="POST",
            data=payload,
            headers={
                "Content-Type": "application/json",
                "X-Capsule-Token": token,
            },
        )
        with urllib.request.urlopen(request) as response:
            write_response = json.loads(response.read().decode("utf-8"))
        self.assertEqual(write_response["result"]["changes"], 3)

        with urllib.request.urlopen(base + "/") as response:
            html = response.read().decode("utf-8")
            csp = response.headers["Content-Security-Policy"]
        self.assertIn("Diagram Studio", html)
        self.assertIn("default-src 'none'", csp)

        oversized = urllib.request.Request(
            base + "/__capsule/write/node.move",
            method="POST",
            data=b"{" + b"x" * (MAX_REQUEST_BYTES + 1) + b"}",
            headers={
                "Content-Type": "application/json",
                "X-Capsule-Token": token,
            },
        )
        with self.assertRaises(urllib.error.HTTPError) as error:
            urllib.request.urlopen(oversized)
        self.assertEqual(error.exception.code, 413)

        nested: object = {}
        for _ in range(MAX_JSON_DEPTH + 1):
            nested = {"value": nested}
        deep_request = urllib.request.Request(
            base + "/__capsule/write/node.move",
            method="POST",
            data=json.dumps(nested).encode("utf-8"),
            headers={
                "Content-Type": "application/json",
                "X-Capsule-Token": token,
            },
        )
        with self.assertRaises(urllib.error.HTTPError) as error:
            urllib.request.urlopen(deep_request)
        self.assertEqual(error.exception.code, 400)

        server.shutdown()
        thread.join(timeout=5)
        self.assertFalse(thread.is_alive())


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import importlib.util
import json
import sqlite3
import sys
import tempfile
import threading
import unittest
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = (
    ROOT
    / "plugins"
    / "capsule-creator"
    / "skills"
    / "create-capsule"
    / "scripts"
    / "capsule_project.py"
)
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase, CapsuleHTTPServer  # noqa: E402

SPEC = importlib.util.spec_from_file_location("capsule_project", SCRIPT)
assert SPEC and SPEC.loader
capsule_project = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capsule_project)


class CapsuleCreatorPluginTests(unittest.TestCase):
    def test_scaffold_build_and_check_produce_a_verified_capsule(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "field-notes"
            output = root / "field-notes.capsule.sqlite"

            created = capsule_project.init_project(
                project,
                title="Field Notes",
                app_id="org.example.field-notes",
            )
            self.assertTrue(created["ok"])
            built = capsule_project.build_project(project, output, repo_root=ROOT)
            self.assertTrue(built["ok"])
            self.assertEqual(built["manifest"]["app_id"], "org.example.field-notes")
            self.assertEqual(built["warnings"], [])
            self.assertTrue(capsule_project.check_project(project, output, repo_root=ROOT)["ok"])

            with CapsuleDatabase(output, read_only=True) as capsule:
                verification = capsule.verify()
                items = capsule.execute_endpoint("item.list", "read", {})
            self.assertTrue(verification["ok"], verification)
            self.assertEqual(items[0]["id"], "item-welcome")

            connection = sqlite3.connect(output)
            try:
                assets = {
                    row[0]
                    for row in connection.execute(
                        "SELECT path FROM capsule_asset ORDER BY path"
                    ).fetchall()
                }
                commands = [
                    json.loads(row[0])
                    for row in connection.execute(
                        "SELECT argv_json FROM capsule_command WHERE argv_json IS NOT NULL"
                    ).fetchall()
                ]
            finally:
                connection.close()
            self.assertIn("app/capsule-client.js", assets)
            self.assertIn("bootstrap/capsule_host.py", assets)
            flattened = " ".join(argument for command in commands for argument in command).lower()
            self.assertNotIn("cargo", flattened)
            self.assertNotIn("src-tauri", flattened)

            capsule = CapsuleDatabase(output)
            server = CapsuleHTTPServer(("127.0.0.1", 0), capsule, "plugin-test-token", quiet=True)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                base = f"http://127.0.0.1:{server.server_address[1]}"
                with urllib.request.urlopen(base + "/", timeout=3) as response:
                    html = response.read().decode("utf-8")
                self.assertIn('href="/app/styles.css"', html)
                self.assertIn('src="/app/capsule-client.js"', html)
                self.assertIn('src="/app/app.js"', html)
                for path, content_type in (
                    ("/app/styles.css", "text/css"),
                    ("/app/capsule-client.js", "text/javascript"),
                    ("/app/app.js", "text/javascript"),
                ):
                    with urllib.request.urlopen(base + path, timeout=3) as response:
                        self.assertTrue(response.headers["Content-Type"].startswith(content_type))
                        self.assertGreater(len(response.read()), 0)
            finally:
                server.shutdown()
                thread.join(timeout=5)
                server.server_close()
                capsule.close()

    def test_scaffold_and_build_refuse_implicit_replacement(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "starter"
            output = root / "starter.capsule.sqlite"
            capsule_project.init_project(
                project,
                title="Starter",
                app_id="org.example.starter",
            )
            with self.assertRaises(capsule_project.ProjectError):
                capsule_project.init_project(
                    project,
                    title="Replacement",
                    app_id="org.example.replacement",
                )

            capsule_project.build_project(project, output, repo_root=ROOT)
            original = output.read_bytes()
            with self.assertRaises(capsule_project.ProjectError):
                capsule_project.build_project(project, output, repo_root=ROOT)
            self.assertEqual(output.read_bytes(), original)

    def test_domain_source_cannot_redefine_platform_or_add_triggers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "unsafe"
            capsule_project.init_project(
                project,
                title="Unsafe",
                app_id="org.example.unsafe",
            )
            (project / "domain.sql").write_text(
                "CREATE TABLE example (id TEXT PRIMARY KEY);\n"
                "CREATE TRIGGER mutate AFTER INSERT ON example BEGIN SELECT 1; END;\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(capsule_project.ProjectError, "forbidden triggers"):
                capsule_project.build_project(
                    project,
                    root / "unsafe.capsule.sqlite",
                    repo_root=ROOT,
                )


if __name__ == "__main__":
    unittest.main()

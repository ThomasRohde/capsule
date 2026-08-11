from __future__ import annotations

import importlib.util
import hashlib
import json
import re
import shutil
import sqlite3
import subprocess
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
PLUGIN = ROOT / "plugins" / "capsule-creator"
INSPECTOR = (
    PLUGIN
    / "skills"
    / "create-capsule"
    / "assets"
    / "examples"
    / "capsule-inspector.capsule.sqlite"
)
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase, CapsuleHTTPServer  # noqa: E402

SPEC = importlib.util.spec_from_file_location("capsule_project", SCRIPT)
assert SPEC and SPEC.loader
capsule_project = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capsule_project)


class CapsuleCreatorPluginTests(unittest.TestCase):
    def test_plugin_manifests_cover_codex_marketplace_and_agent_plugins_v1(self) -> None:
        codex_manifest = json.loads(
            (PLUGIN / ".codex-plugin" / "plugin.json").read_text(encoding="utf-8")
        )
        portable_manifest = json.loads(
            (PLUGIN / "plugin.json").read_text(encoding="utf-8")
        )
        marketplace = json.loads(
            (ROOT / ".agents" / "plugins" / "marketplace.json").read_text(
                encoding="utf-8"
            )
        )

        portable_fields = {
            "$schema",
            "name",
            "version",
            "description",
            "author",
            "homepage",
            "repository",
            "license",
            "keywords",
            "extensions",
        }
        self.assertEqual(
            portable_manifest["$schema"],
            "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        )
        self.assertLessEqual(set(portable_manifest), portable_fields)
        self.assertRegex(
            portable_manifest["name"],
            r"^(?!.*(?:--|\.\.))[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$",
        )
        self.assertLessEqual(len(portable_manifest["name"]), 64)
        for field in (
            "name",
            "version",
            "description",
            "author",
            "homepage",
            "repository",
            "license",
            "keywords",
        ):
            self.assertEqual(portable_manifest[field], codex_manifest[field])

        self.assertEqual(marketplace["name"], "sqlite-capsule")
        self.assertEqual(
            marketplace["interface"]["displayName"], "SQLite Capsule"
        )
        self.assertEqual(len(marketplace["plugins"]), 1)
        entry = marketplace["plugins"][0]
        self.assertEqual(entry["name"], portable_manifest["name"])
        self.assertEqual(entry["source"], {
            "source": "local",
            "path": "./plugins/capsule-creator",
        })
        self.assertEqual(entry["policy"], {
            "installation": "AVAILABLE",
            "authentication": "ON_INSTALL",
        })
        self.assertEqual(entry["category"], "Developer Tools")

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
            built = capsule_project.build_project(project, output)
            self.assertTrue(built["ok"])
            self.assertEqual(built["manifest"]["app_id"], "org.example.field-notes")
            self.assertEqual(built["warnings"], [])
            self.assertTrue(capsule_project.check_project(project, output)["ok"])

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

            capsule_project.build_project(project, output)
            original = output.read_bytes()
            with self.assertRaises(capsule_project.ProjectError):
                capsule_project.build_project(project, output)
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
                )

    def test_schema_inspection_catches_comment_obfuscated_trigger_and_virtual_table(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for kind, declaration, message in (
                (
                    "trigger",
                    "CREATE/**/TRIGGER mutate AFTER INSERT ON example BEGIN SELECT 1; END;",
                    "created forbidden triggers",
                ),
                (
                    "virtual",
                    "CREATE/**/VIRTUAL/**/TABLE search_index USING fts5(content);",
                    "created forbidden virtual tables",
                ),
            ):
                with self.subTest(kind=kind):
                    project = root / kind
                    capsule_project.init_project(
                        project,
                        title=kind.title(),
                        app_id=f"org.example.{kind}",
                    )
                    (project / "domain.sql").write_text(
                        "CREATE TABLE example (id TEXT PRIMARY KEY);\n" + declaration,
                        encoding="utf-8",
                    )
                    with self.assertRaisesRegex(capsule_project.ProjectError, message):
                        capsule_project.build_project(project, root / f"{kind}.sqlite")

    def test_project_can_exclude_pure_content_from_executable_assets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "content-flags"
            output = root / "content-flags.capsule.sqlite"
            capsule_project.init_project(
                project,
                title="Content Flags",
                app_id="org.example.content-flags",
            )
            (project / "source" / "app" / "article.html").write_text(
                "<article>inert content</article>", encoding="utf-8"
            )
            config_path = project / "capsule-project.json"
            config = json.loads(config_path.read_text(encoding="utf-8"))
            config["non_executable_assets"] = ["app/article.html"]
            config_path.write_text(json.dumps(config), encoding="utf-8")

            capsule_project.build_project(project, output)
            connection = sqlite3.connect(output)
            flags = dict(
                connection.execute(
                    "SELECT path, executable FROM capsule_asset "
                    "WHERE path IN ('app/article.html', 'app/index.html', 'app/app.js')"
                ).fetchall()
            )
            connection.close()
            self.assertEqual(flags["app/article.html"], 0)
            self.assertEqual(flags["app/index.html"], 1)
            self.assertEqual(flags["app/app.js"], 1)

    def test_seed_tables_follow_foreign_key_dependency_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "seed-order"
            output = root / "seed-order.capsule.sqlite"
            capsule_project.init_project(
                project,
                title="Seed Order",
                app_id="org.example.seed-order",
            )
            with (project / "domain.sql").open("a", encoding="utf-8", newline="\n") as target:
                target.write(
                    "\nCREATE TABLE z_parent (id TEXT PRIMARY KEY);\n"
                    "CREATE TABLE a_child (\n"
                    "  id TEXT PRIMARY KEY,\n"
                    "  parent_id TEXT NOT NULL REFERENCES z_parent(id)\n"
                    ");\n"
                )
            seed_path = project / "source" / "data" / "seed.json"
            seed = json.loads(seed_path.read_text(encoding="utf-8"))
            seed["a_child"] = [{"id": "child-1", "parent_id": "parent-1"}]
            seed["z_parent"] = [{"id": "parent-1"}]
            seed_path.write_text(
                json.dumps(seed, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
                newline="\n",
            )

            capsule_project.build_project(project, output)
            connection = sqlite3.connect(output)
            try:
                child = connection.execute(
                    "SELECT id, parent_id FROM a_child"
                ).fetchone()
            finally:
                connection.close()
            self.assertEqual(child, ("child-1", "parent-1"))

    def test_plugin_copy_builds_and_verifies_without_repository_access(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            copied_plugin = root / "capsule-creator"
            shutil.copytree(PLUGIN, copied_plugin)
            copied_script = (
                copied_plugin
                / "skills"
                / "create-capsule"
                / "scripts"
                / "capsule_project.py"
            )
            project = root / "outside-project"
            output = root / "outside.capsule.sqlite"

            commands = [
                [
                    sys.executable,
                    str(copied_script),
                    "init",
                    str(project),
                    "--title",
                    "Outside Project",
                    "--app-id",
                    "org.example.outside-project",
                ],
                [sys.executable, str(copied_script), "build", str(project), str(output)],
                [sys.executable, str(copied_script), "host", "verify", str(output)],
                [sys.executable, str(copied_script), "conformance", str(output)],
                [sys.executable, str(copied_script), "check", str(project), str(output)],
            ]
            for command in commands:
                completed = subprocess.run(
                    command,
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=30,
                    check=False,
                )
                self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
                self.assertNotIn(str(ROOT), completed.stdout + completed.stderr)

            with CapsuleDatabase(output, read_only=True) as capsule:
                self.assertTrue(capsule.verify()["ok"])

    def test_reference_inspector_is_current_and_carries_local_wasm(self) -> None:
        self.assertTrue(INSPECTOR.is_file())
        with CapsuleDatabase(INSPECTOR, read_only=True) as capsule:
            verification = capsule.verify()
            self.assertTrue(verification["ok"], verification)
            self.assertEqual(verification["manifest"]["app_id"], "org.sqlite-capsule.inspector")
        connection = sqlite3.connect(INSPECTOR)
        try:
            assets = {
                row[0]: (row[1], row[2])
                for row in connection.execute(
                    "SELECT path, media_type, length(content) FROM capsule_asset"
                ).fetchall()
            }
        finally:
            connection.close()
        self.assertEqual(assets["app/vendor/sqlite-wasm/sqlite3.wasm"][0], "application/wasm")
        self.assertGreater(assets["app/vendor/sqlite-wasm/sqlite3.wasm"][1], 800_000)
        self.assertIn("app/vendor/sqlite-wasm/index.mjs", assets)
        self.assertIn("app/sha256.js", assets)
        self.assertIn("app/favicon.svg", assets)
        self.assertIn("app/legal/sqlite-wasm/LICENSE.Apache-2.0.txt", assets)
        self.assertIn("app/legal/sqlite-wasm/THIRD_PARTY.md", assets)

        source = INSPECTOR.parent / "capsule-inspector" / "source" / "app"
        index = (source / "index.html").read_text(encoding="utf-8")
        app = (source / "app.js").read_text(encoding="utf-8")
        styles = (source / "styles.css").read_text(encoding="utf-8")
        icon = (source / "favicon.svg").read_text(encoding="utf-8")
        canonical_icon = (
            ROOT
            / "docs"
            / "images"
            / "brand"
            / "sqlite-capsule-mark-verified.svg"
        ).read_text(encoding="utf-8")
        self.assertIn('<html lang="en" data-theme="light">', index)
        self.assertEqual(index.count('src="/app/favicon.svg"'), 2)
        self.assertNotIn('class="database-shape"', index)
        self.assertIn("No external network", index)
        self.assertNotIn("Local only", index)
        self.assertIn(
            'title="Runs on this device and communicates only with the local Capsule Host."',
            index,
        )
        self.assertIn("white-space: nowrap", styles)
        for path_data in re.findall(r'\bd="([^"]+)"', canonical_icon):
            self.assertIn(f'd="{path_data}"', icon)
        self.assertIn('transform="translate(12 34) scale(.9)"', icon)
        self.assertNotIn('translate(-14 5)', icon)
        self.assertIn('<mask id="loupe-knockout"', icon)
        self.assertIn(
            'stroke="black" stroke-width="20"',
            icon,
        )
        self.assertIn('mask="url(#loupe-knockout)"', icon)
        self.assertIn('<circle cx="184" cy="176" r="42"/>', icon)
        for mode in ("light", "dark", "system"):
            self.assertIn(f'data-theme-option="{mode}"', index)
        self.assertIn('matchMedia("(prefers-color-scheme: dark)")', app)
        self.assertIn('localStorage.getItem("capsule-inspector-theme") || "light"', app)
        self.assertIn('.theme-options button[aria-pressed="true"]', styles)

    def test_repository_instructions_keep_plugin_current_without_capsule_only_installer_rebuilds(self) -> None:
        instructions = (ROOT / "AGENTS.md").read_text(encoding="utf-8")
        self.assertIn(
            "Keep `plugins/capsule-creator/` synchronized with material changes",
            instructions,
        )
        self.assertIn("standalone copy without repository access", instructions)
        self.assertIn(
            "generated-capsule-only changes do not require an installer",
            instructions,
        )
        self.assertNotIn("verified after every feature", instructions)

    def test_inspector_portable_sha256_matches_standard_vectors(self) -> None:
        module_path = (
            PLUGIN
            / "skills"
            / "create-capsule"
            / "assets"
            / "examples"
            / "capsule-inspector"
            / "source"
            / "app"
            / "sha256.js"
        )
        node = shutil.which("node")
        if node is None:
            self.skipTest("Node.js is unavailable")
        vectors = [b"", b"abc", bytes(range(256)), b"capsule" * 10_000]
        script = (
            "import { portableSha256 } from " + json.dumps(module_path.as_uri()) + ";"
            "const encoder = new TextEncoder();"
            "const vectors = [new Uint8Array(0), encoder.encode('abc'),"
            "Uint8Array.from({length: 256}, (_, index) => index),"
            "encoder.encode('capsule'.repeat(10000))];"
            "for (const value of vectors) console.log(portableSha256(Uint8Array.from(value)));"
        )
        completed = subprocess.run(
            [node, "--input-type=module", "--eval", script],
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(
            completed.stdout.splitlines(),
            [hashlib.sha256(value).hexdigest() for value in vectors],
        )

    def test_skill_routes_to_self_contained_authoring_references(self) -> None:
        skill = PLUGIN / "skills" / "create-capsule"
        skill_text = (skill / "SKILL.md").read_text(encoding="utf-8")
        self.assertNotIn("tools/capsule.py", skill_text)
        self.assertNotIn("repository `AGENTS.md`", skill_text)
        references = (
            "authoring-contract.md",
            "format-and-runtime.md",
            "quality-playbook.md",
            "fluent-ui.md",
            "inspector-black-box.md",
        )
        for name in references:
            path = skill / "references" / name
            self.assertTrue(path.is_file(), name)
            self.assertGreater(len(path.read_text(encoding="utf-8")), 1_000, name)
            self.assertIn(f"references/{name}", skill_text)


if __name__ == "__main__":
    unittest.main()

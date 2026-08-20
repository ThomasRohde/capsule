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


def _validate_frozen_schema(
    instance: object,
    schema: dict[str, object],
    *,
    root: dict[str, object] | None = None,
    path: str = "$",
) -> None:
    """Small stdlib validator for the frozen projection-schema vocabulary."""
    root = schema if root is None else root
    reference = schema.get("$ref")
    if isinstance(reference, str):
        if not reference.startswith("#/"):
            raise AssertionError(f"{path}: external schema reference is not frozen")
        resolved: object = root
        for part in reference[2:].split("/"):
            if not isinstance(resolved, dict) or part not in resolved:
                raise AssertionError(f"{path}: unresolved schema reference {reference}")
            resolved = resolved[part]
        if not isinstance(resolved, dict):
            raise AssertionError(f"{path}: schema reference is not an object")
        _validate_frozen_schema(instance, resolved, root=root, path=path)
        return

    alternatives = schema.get("oneOf")
    if isinstance(alternatives, list):
        matches = 0
        for alternative in alternatives:
            try:
                _validate_frozen_schema(instance, alternative, root=root, path=path)
            except AssertionError:
                continue
            matches += 1
        if matches != 1:
            raise AssertionError(f"{path}: expected exactly one oneOf match, got {matches}")
        return

    if "const" in schema and instance != schema["const"]:
        raise AssertionError(f"{path}: expected constant {schema['const']!r}")
    if "enum" in schema and instance not in schema["enum"]:
        raise AssertionError(f"{path}: value is outside the enum")

    expected_types = schema.get("type")
    if isinstance(expected_types, str):
        expected_types = [expected_types]
    if isinstance(expected_types, list):
        type_checks = {
            "null": instance is None,
            "boolean": isinstance(instance, bool),
            "integer": isinstance(instance, int) and not isinstance(instance, bool),
            "number": isinstance(instance, (int, float)) and not isinstance(instance, bool),
            "string": isinstance(instance, str),
            "array": isinstance(instance, list),
            "object": isinstance(instance, dict),
        }
        if not any(type_checks.get(str(kind), False) for kind in expected_types):
            raise AssertionError(f"{path}: expected type {expected_types!r}")

    if isinstance(instance, dict):
        required = schema.get("required", [])
        if isinstance(required, list):
            missing = [key for key in required if key not in instance]
            if missing:
                raise AssertionError(f"{path}: missing required properties {missing!r}")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            properties = {}
        if schema.get("additionalProperties") is False:
            unexpected = sorted(set(instance) - set(properties))
            if unexpected:
                raise AssertionError(f"{path}: unexpected properties {unexpected!r}")
        for key, value in instance.items():
            subschema = properties.get(key)
            if isinstance(subschema, dict):
                _validate_frozen_schema(
                    value, subschema, root=root, path=f"{path}.{key}"
                )

    if isinstance(instance, list):
        minimum = schema.get("minItems")
        maximum = schema.get("maxItems")
        if isinstance(minimum, int) and len(instance) < minimum:
            raise AssertionError(f"{path}: fewer than {minimum} items")
        if isinstance(maximum, int) and len(instance) > maximum:
            raise AssertionError(f"{path}: more than {maximum} items")
        if schema.get("uniqueItems") is True:
            canonical = [
                json.dumps(value, ensure_ascii=False, sort_keys=True) for value in instance
            ]
            if len(canonical) != len(set(canonical)):
                raise AssertionError(f"{path}: array items are not unique")
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, value in enumerate(instance):
                _validate_frozen_schema(
                    value, item_schema, root=root, path=f"{path}[{index}]"
                )

    if isinstance(instance, str):
        minimum = schema.get("minLength")
        maximum = schema.get("maxLength")
        byte_maximum = schema.get("x-maxUtf8Bytes")
        if isinstance(minimum, int) and len(instance) < minimum:
            raise AssertionError(f"{path}: string is shorter than {minimum}")
        if isinstance(maximum, int) and len(instance) > maximum:
            raise AssertionError(f"{path}: string is longer than {maximum}")
        if isinstance(byte_maximum, int) and len(instance.encode("utf-8")) > byte_maximum:
            raise AssertionError(f"{path}: UTF-8 value exceeds {byte_maximum} bytes")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.search(pattern, instance) is None:
            raise AssertionError(f"{path}: string does not match {pattern!r}")

    if isinstance(instance, int) and not isinstance(instance, bool):
        minimum = schema.get("minimum")
        maximum = schema.get("maximum")
        if isinstance(minimum, int) and instance < minimum:
            raise AssertionError(f"{path}: integer is below {minimum}")
        if isinstance(maximum, int) and instance > maximum:
            raise AssertionError(f"{path}: integer exceeds {maximum}")


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
            for format_version, explicit in (("0.2", False), ("0.3", True)):
                with self.subTest(format_version=format_version):
                    project = root / f"outside-project-v{format_version[-1]}"
                    output = root / f"outside-v{format_version[-1]}.capsule.sqlite"
                    init_command = [
                        sys.executable,
                        str(copied_script),
                        "init",
                        str(project),
                        "--title",
                        f"Outside Project {format_version}",
                        "--app-id",
                        f"org.example.outside-v{format_version[-1]}",
                    ]
                    if explicit:
                        init_command.extend(("--format-version", format_version))
                    commands = [
                        init_command,
                        [
                            sys.executable,
                            str(copied_script),
                            "build",
                            str(project),
                            str(output),
                        ],
                        [sys.executable, str(copied_script), "host", "verify", str(output)],
                        [sys.executable, str(copied_script), "conformance", str(output)],
                        [
                            sys.executable,
                            str(copied_script),
                            "check",
                            str(project),
                            str(output),
                        ],
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
                        self.assertEqual(
                            completed.returncode,
                            0,
                            completed.stderr or completed.stdout,
                        )
                        self.assertNotIn(str(ROOT), completed.stdout + completed.stderr)

                    connection = sqlite3.connect(output)
                    try:
                        artifact_version = connection.execute(
                            "SELECT format_version FROM capsule_manifest WHERE id = 1"
                        ).fetchone()[0]
                        user_version = connection.execute("PRAGMA user_version").fetchone()[0]
                    finally:
                        connection.close()
                    self.assertEqual(artifact_version, format_version)
                    self.assertEqual(user_version, 2 if format_version == "0.2" else 3)
                    with CapsuleDatabase(output, read_only=True) as capsule:
                        self.assertTrue(capsule.verify()["ok"])

    def test_standalone_v03_copy_enforces_column_and_json_byte_boundaries(self) -> None:
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

            def run(*arguments: str) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    [sys.executable, str(copied_script), *arguments],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=60,
                    check=False,
                )

            def project(name: str) -> Path:
                target = root / name
                completed = run(
                    "init",
                    str(target),
                    "--title",
                    name,
                    "--app-id",
                    f"org.example.{name}",
                    "--format-version",
                    "0.3",
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)
                return target

            def classify(target: Path, table: str, **updates: object) -> None:
                path = target / "source" / "data-contract.json"
                contract = json.loads(path.read_text(encoding="utf-8"))
                declaration = {
                    "dataset_id": "items",
                    "table_name": table,
                    "sequence": 20,
                    "primary_key_json": ["id"],
                    "ignored_columns_json": [],
                    "immutable_columns_json": ["id"],
                }
                declaration.update(updates)
                contract["tables"].append(declaration)
                path.write_text(
                    json.dumps(contract, ensure_ascii=False, indent=2) + "\n",
                    encoding="utf-8",
                    newline="\n",
                )

            exact = project("columns-exact")
            exact_name = "é" * 128
            exact_columns = [f'"column_{index:03}" TEXT' for index in range(254)]
            with (exact / "domain.sql").open("a", encoding="utf-8", newline="\n") as target:
                target.write(
                    "\nCREATE TABLE column_boundary (id INTEGER PRIMARY KEY, "
                    + ", ".join(exact_columns)
                    + f', "{exact_name}" TEXT GENERATED ALWAYS AS (\'ok\') STORED);\n'
                )
            classify(exact, "column_boundary")
            exact_output = root / "columns-exact.capsule.sqlite"
            completed = run("build", str(exact), str(exact_output))
            self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
            completed = run("host", "verify", str(exact_output))
            self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
            connection = sqlite3.connect(exact_output)
            try:
                xinfo = connection.execute(
                    "PRAGMA table_xinfo(column_boundary)"
                ).fetchall()
            finally:
                connection.close()
            self.assertEqual(len(xinfo), 256)
            self.assertEqual(len(exact_name.encode("utf-8")), 256)

            for name, generated in (
                (
                    "columns-plus-one",
                    ', "generated_a" TEXT GENERATED ALWAYS AS (\'a\') STORED, '
                    '"generated_b" TEXT GENERATED ALWAYS AS (\'b\') STORED',
                ),
                (
                    "column-name-plus-one",
                    f', "{("é" * 128) + "a"}" TEXT GENERATED ALWAYS AS (\'a\') STORED',
                ),
            ):
                with self.subTest(case=name):
                    target = project(name)
                    ordinary = [f'"column_{index:03}" TEXT' for index in range(254)]
                    if name == "column-name-plus-one":
                        ordinary = []
                    with (target / "domain.sql").open(
                        "a", encoding="utf-8", newline="\n"
                    ) as sql:
                        sql.write(
                            "\nCREATE TABLE hostile_columns (id INTEGER PRIMARY KEY"
                            + (", " + ", ".join(ordinary) if ordinary else "")
                            + generated
                            + ");\n"
                        )
                    classify(target, "hostile_columns")
                    completed = run(
                        "build", str(target), str(root / f"{name}.capsule.sqlite")
                    )
                    self.assertNotEqual(completed.returncode, 0)
                    self.assertIn(
                        "tables/columns exceed the host schema ceiling",
                        completed.stderr + completed.stdout,
                    )

            for width, accepted in ((252, True), (253, False)):
                with self.subTest(json_name_width=width):
                    target = project(f"json-{width}")
                    names = [f"c{index:02}_" + ("x" * (width - 4)) for index in range(64)]
                    encoded = json.dumps(
                        names,
                        ensure_ascii=False,
                        sort_keys=True,
                        separators=(",", ":"),
                    ).encode("utf-8")
                    self.assertEqual(len(encoded), 16_321 if accepted else 16_385)
                    with (target / "domain.sql").open(
                        "a", encoding="utf-8", newline="\n"
                    ) as sql:
                        sql.write(
                            "\nCREATE TABLE json_boundary (id INTEGER PRIMARY KEY, "
                            + ", ".join(f'"{column}" TEXT' for column in names)
                            + ");\n"
                        )
                    classify(
                        target,
                        "json_boundary",
                        ignored_columns_json=names,
                        immutable_columns_json=names,
                    )
                    output = root / f"json-{width}.capsule.sqlite"
                    completed = run("build", str(target), str(output))
                    if accepted:
                        self.assertEqual(
                            completed.returncode, 0, completed.stderr or completed.stdout
                        )
                        completed = run("host", "verify", str(output))
                        self.assertEqual(
                            completed.returncode, 0, completed.stderr or completed.stdout
                        )
                    else:
                        self.assertNotEqual(completed.returncode, 0)
                        self.assertIn(
                            "exceeds the host JSON ceiling",
                            completed.stderr + completed.stdout,
                        )

    def test_standalone_v03_template_proof_is_derived_from_seed_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            copied_plugin = root / "capsule-creator"
            shutil.copytree(PLUGIN, copied_plugin)
            script = (
                copied_plugin
                / "skills"
                / "create-capsule"
                / "scripts"
                / "capsule_project.py"
            )
            project = root / "template-project"
            output = root / "template.capsule.sqlite"

            def run(*arguments: str) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    [sys.executable, str(script), *arguments],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=60,
                    check=False,
                )

            created = run(
                "init",
                str(project),
                "--title",
                "Clean Template",
                "--app-id",
                "org.example.clean-template",
                "--format-version",
                "0.3",
                "--template",
            )
            self.assertEqual(created.returncode, 0, created.stderr or created.stdout)
            built = run("build", str(project), str(output))
            self.assertEqual(built.returncode, 0, built.stderr or built.stdout)
            self.assertNotIn(str(ROOT), created.stdout + built.stdout + built.stderr)
            verified = run("host", "verify", str(output))
            self.assertEqual(verified.returncode, 0, verified.stderr or verified.stdout)

            connection = sqlite3.connect(output)
            try:
                content = connection.execute(
                    "SELECT content FROM capsule_doc WHERE slug = ?1",
                    ("org.sqlite-capsule.template-state",),
                ).fetchone()[0]
            finally:
                connection.close()
            proof = json.loads(content)
            self.assertEqual(
                content,
                json.dumps(
                    proof,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                ),
            )
            self.assertEqual(proof["profile"], "org.sqlite-capsule.template-state/1")
            self.assertEqual(proof["dataset_state_profile"], "org.sqlite-capsule.dataset-state/1")
            self.assertEqual(proof["datasets"][0]["dataset_id"], "items")
            self.assertEqual(proof["datasets"][0]["disposition"], "seed")
            self.assertEqual(proof["datasets"][0]["stored_row_count"], 1)
            self.assertRegex(proof["datasets"][0]["state_sha256"], r"^[0-9a-f]{64}$")

            config_path = project / "capsule-project.json"
            config = json.loads(config_path.read_text(encoding="utf-8"))
            config["template_state"] = {"items": "empty"}
            config_path.write_text(
                json.dumps(config, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
                newline="\n",
            )
            rejected = run("build", str(project), str(root / "invalid.sqlite"))
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("declared empty but contains rows", rejected.stderr)

    def test_standalone_signed_v03_native_projections_match_frozen_schemas(self) -> None:
        cargo = shutil.which("cargo")
        if cargo is None:
            self.skipTest("Cargo is unavailable")

        native_root = ROOT / "native"
        built = subprocess.run(
            [cargo, "build", "-p", "sqlite-capsule-cli", "--bin", "capsule-native"],
            cwd=native_root,
            text=True,
            capture_output=True,
            timeout=300,
            check=False,
        )
        self.assertEqual(built.returncode, 0, built.stderr or built.stdout)
        metadata = subprocess.run(
            [cargo, "metadata", "--format-version", "1", "--no-deps"],
            cwd=native_root,
            text=True,
            capture_output=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(metadata.returncode, 0, metadata.stderr or metadata.stdout)
        target_directory = Path(json.loads(metadata.stdout)["target_directory"])
        binary_name = "capsule-native.exe" if sys.platform == "win32" else "capsule-native"
        native_cli = target_directory / "debug" / binary_name
        self.assertTrue(native_cli.is_file(), native_cli)

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
            project = root / "signed-v03-project"
            unsigned = root / "unsigned.capsule.sqlite"
            signed = root / "signed.capsule.sqlite"
            signing_key = root / "development-seed.hex"
            shutil.copyfile(
                ROOT / "compatibility" / "signed-app-v0.2" / "development-seed.hex",
                signing_key,
            )

            def run_plugin(*arguments: str) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    [sys.executable, str(copied_script), *arguments],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=60,
                    check=False,
                )

            for arguments in (
                (
                    "init",
                    str(project),
                    "--title",
                    "Signed v0.3 Projection",
                    "--app-id",
                    "org.example.signed-v03-projection",
                    "--format-version",
                    "0.3",
                    "--template",
                ),
                ("build", str(project), str(unsigned)),
                ("host", "verify", str(unsigned)),
            ):
                completed = run_plugin(*arguments)
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr or completed.stdout,
                )
                self.assertNotIn(str(ROOT), completed.stdout + completed.stderr)

            unsigned_sha256 = hashlib.sha256(unsigned.read_bytes()).hexdigest()
            signed_report = subprocess.run(
                [
                    str(native_cli),
                    "sign",
                    str(unsigned),
                    str(signed),
                    "--publisher-id",
                    "org.example.test-publisher",
                    "--publisher-name",
                    "SQLite Capsule Test Publisher",
                    "--key",
                    str(signing_key),
                    "--signed-at",
                    "2026-08-12T12:34:56Z",
                ],
                cwd=root,
                text=True,
                capture_output=True,
                timeout=120,
                check=False,
            )
            self.assertEqual(
                signed_report.returncode,
                0,
                signed_report.stderr or signed_report.stdout,
            )
            signing_projection = json.loads(signed_report.stdout)
            self.assertTrue(signing_projection["ok"])
            self.assertTrue(signing_projection["signature_valid"])
            self.assertTrue(signed.is_file())
            self.assertEqual(
                hashlib.sha256(unsigned.read_bytes()).hexdigest(),
                unsigned_sha256,
                "native signing mutated the standalone creator output",
            )

            def run_native(command: str) -> dict[str, object]:
                completed = subprocess.run(
                    [str(native_cli), command, str(signed)],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=120,
                    check=False,
                )
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr or completed.stdout,
                )
                return json.loads(completed.stdout)

            overview = run_native("overview")
            data_contract = run_native("data-contract")
            lineage = run_native("lineage")
            template_state = run_native("template-state")
            self.assertEqual(
                overview["profile"],
                "org.sqlite-capsule.workspace-overview-response/1",
            )
            self.assertEqual(
                data_contract["profile"],
                "org.sqlite-capsule.workspace-data-contract-response/1",
            )
            self.assertEqual(
                lineage["profile"],
                "org.sqlite-capsule.workspace-lineage-response/1",
            )
            self.assertTrue(overview["ok"])
            self.assertTrue(overview["verified"])
            self.assertTrue(data_contract["ok"])
            self.assertTrue(data_contract["verified_signed_contract"])
            self.assertTrue(lineage["ok"])
            self.assertEqual(
                template_state["profile"],
                "org.sqlite-capsule.workspace-template-state-response/1",
            )
            self.assertTrue(template_state["ok"])
            self.assertTrue(template_state["verified_signed_template_state"])
            self.assertEqual(
                template_state["template_state"]["profile"],
                "org.sqlite-capsule.template-state/1",
            )

            identity = overview["identity"]
            self.assertIsInstance(identity, dict)
            assert isinstance(identity, dict)
            self.assertEqual(identity["format_version"], "0.3")
            self.assertEqual(identity["user_version"], 3)
            nested_overview = identity["overview"]
            self.assertIsInstance(nested_overview, dict)
            assert isinstance(nested_overview, dict)
            application = nested_overview["application"]
            instance = nested_overview["instance"]
            schema_identity = nested_overview["data_schema"]
            self.assertIsInstance(application, dict)
            self.assertIsInstance(instance, dict)
            self.assertIsInstance(schema_identity, dict)
            assert isinstance(application, dict)
            assert isinstance(instance, dict)
            assert isinstance(schema_identity, dict)

            application_projection = {
                "profile": "org.sqlite-capsule.application-profile/0.3",
                "app_id": application["app_id"],
                "app_version": application["app_version"],
                "name": application["name"],
                "description": application["description"],
                "category": application["category"],
                "icon_asset": application["icon_asset"],
                "release_notes_doc": application["release_notes_doc"],
                "data_schema": {
                    "id": schema_identity["data_schema_id"],
                    "version": schema_identity["data_schema_version"],
                },
                "minimum_host_profile": "org.sqlite-capsule.host-profile/0.3",
            }
            instance_projection = {
                "profile": "org.sqlite-capsule.instance-profile/0.3",
                **instance,
            }
            contracts = (
                ROOT / "docs" / "plans" / "capsule-lifecycle" / "contracts"
            )
            for projection, schema_name in (
                (
                    application_projection,
                    "capsule-application-profile-v0.3.schema.json",
                ),
                (instance_projection, "capsule-instance-profile-v0.3.schema.json"),
                (
                    data_contract["data_contract"],
                    "capsule-data-contract-v0.3.schema.json",
                ),
                (lineage["lineage"], "capsule-lineage-v0.3.schema.json"),
                (
                    template_state["template_state"],
                    "template-state-v1.schema.json",
                ),
            ):
                with self.subTest(schema=schema_name):
                    frozen_schema = json.loads(
                        (contracts / schema_name).read_text(encoding="utf-8")
                    )
                    _validate_frozen_schema(projection, frozen_schema)

            projected_contract = data_contract["data_contract"]
            self.assertIsInstance(projected_contract, dict)
            assert isinstance(projected_contract, dict)
            self.assertEqual(projected_contract["app_id"], application["app_id"])
            self.assertEqual(
                projected_contract["data_schema_id"],
                schema_identity["data_schema_id"],
            )
            self.assertEqual(len(projected_contract["datasets"]), 1)
            self.assertEqual(
                projected_contract["datasets"][0]["tables"][0]["name"], "item"
            )

            projected_lineage = lineage["lineage"]
            self.assertIsInstance(projected_lineage, dict)
            assert isinstance(projected_lineage, dict)
            self.assertEqual(projected_lineage["capsule_id"], instance["capsule_id"])
            self.assertEqual(projected_lineage["provenance_status"], "mutable-untrusted")
            serialized_lineage = json.dumps(projected_lineage, sort_keys=True)
            self.assertNotIn("details_json", serialized_lineage)
            self.assertNotIn("publisher_trusted", serialized_lineage)
            self.assertNotIn("publisher_authenticated", serialized_lineage)

    def test_explicit_v03_project_separates_application_instance_and_data_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "v03"
            output = root / "v03.capsule.sqlite"
            created = capsule_project.init_project(
                project,
                title="Lifecycle Notes",
                app_id="org.example.lifecycle-notes",
                format_version="0.3",
            )
            self.assertEqual(created["format_version"], "0.3")
            built = capsule_project.build_project(project, output)
            self.assertEqual(built["manifest"]["format_version"], "0.3")
            self.assertEqual(built["inventory"]["datasets"], 1)
            self.assertEqual(built["inventory"]["dataset_tables"], 1)

            connection = sqlite3.connect(output)
            try:
                application = connection.execute(
                    "SELECT name, category FROM capsule_application WHERE id = 1"
                ).fetchone()
                instance = connection.execute(
                    "SELECT capsule_id, revision_id, document_kind FROM capsule_instance WHERE id = 1"
                ).fetchone()
                classified = connection.execute(
                    "SELECT dataset_id, table_name, primary_key_json "
                    "FROM capsule_dataset_table"
                ).fetchone()
            finally:
                connection.close()
            self.assertEqual(application, ("Lifecycle Notes", "productivity"))
            self.assertRegex(instance[0], r"^[0-9a-f-]{36}$")
            self.assertRegex(instance[1], r"^[0-9a-f-]{36}$")
            self.assertEqual(instance[2], "document")
            self.assertEqual(classified, ("items", "item", '["id"]'))

    def test_v03_authoring_rejects_workspace_incompatible_dataset_contracts(self) -> None:
        mutations = {
            "required-omit": lambda value: value["datasets"][0].update(
                {"required": 1, "fork_policy": "omit"}
            ),
            "three-way-summary": lambda value: value["datasets"][0].update(
                {"reconcile_policy": "three-way", "compare_policy": "summary"}
            ),
            "empty-dataset": lambda value: value["datasets"].append(
                {
                    "id": "empty",
                    "role": "derived",
                    "description": "No classified table.",
                    "fork_policy": "reset",
                    "compare_policy": "ignore",
                    "reconcile_policy": "ignore",
                    "upgrade_policy": "rebuild",
                    "sensitivity": "normal",
                    "required": 0,
                }
            ),
            "ignored-primary-key": lambda value: value["tables"][0].update(
                {"ignored_columns_json": ["id"]}
            ),
            "dependency-cycle": lambda value: value["dependencies"].append(
                {
                    "dataset_id": "items",
                    "depends_on_dataset_id": "items",
                    "reason": "Self cycle is forbidden.",
                }
            ),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                project = root / name
                capsule_project.init_project(
                    project,
                    title="Invalid Contract",
                    app_id=f"org.example.{name}",
                    format_version="0.3",
                )
                contract_path = project / "source" / "data-contract.json"
                contract = json.loads(contract_path.read_text(encoding="utf-8"))
                mutate(contract)
                contract_path.write_text(json.dumps(contract), encoding="utf-8")
                with self.assertRaises(capsule_project.ProjectError):
                    capsule_project.build_project(project, root / f"{name}.sqlite")

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "nullable-composite"
            capsule_project.init_project(
                project,
                title="Nullable Composite",
                app_id="org.example.nullable-composite",
                format_version="0.3",
            )
            with (project / "domain.sql").open("a", encoding="utf-8") as target:
                target.write("\nCREATE TABLE weak_key (a TEXT, b TEXT, PRIMARY KEY (a, b));\n")
            contract_path = project / "source" / "data-contract.json"
            contract = json.loads(contract_path.read_text(encoding="utf-8"))
            contract["tables"].append({
                "dataset_id": "items",
                "table_name": "weak_key",
                "sequence": 20,
                "primary_key_json": ["a", "b"],
                "ignored_columns_json": [],
                "immutable_columns_json": ["a", "b"],
            })
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            with self.assertRaisesRegex(capsule_project.ProjectError, "NOT NULL"):
                capsule_project.build_project(project, root / "nullable.sqlite")

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "cross-dataset-fk"
            capsule_project.init_project(
                project,
                title="Cross Dataset FK",
                app_id="org.example.cross-dataset-fk",
                format_version="0.3",
            )
            domain_path = project / "domain.sql"
            domain = domain_path.read_text(encoding="utf-8").replace(
                "    updated_at  TEXT NOT NULL\n);",
                "    updated_at  TEXT NOT NULL,\n"
                "    setting_key TEXT REFERENCES setting(key) ON DELETE RESTRICT\n"
                ");\n\n"
                "CREATE TABLE setting (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);",
            )
            domain_path.write_text(domain, encoding="utf-8")
            contract_path = project / "source" / "data-contract.json"
            contract = json.loads(contract_path.read_text(encoding="utf-8"))
            contract["datasets"].append({
                "id": "settings",
                "role": "settings",
                "description": "Settings referenced by items.",
                "fork_policy": "copy",
                "compare_policy": "row",
                "reconcile_policy": "manual",
                "upgrade_policy": "copy",
                "sensitivity": "normal",
                "required": 1,
            })
            contract["tables"].append({
                "dataset_id": "settings",
                "table_name": "setting",
                "sequence": 20,
                "primary_key_json": ["key"],
                "ignored_columns_json": [],
                "immutable_columns_json": ["key"],
            })
            dependency = {
                "dataset_id": "items",
                "depends_on_dataset_id": "settings",
                "reason": "Items reference settings.",
            }
            contract["dependencies"].append(dependency)
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            seed_path = project / "source" / "data" / "seed.json"
            seed = json.loads(seed_path.read_text(encoding="utf-8"))
            seed["setting"] = [{"key": "theme", "value": "light"}]
            seed_path.write_text(json.dumps(seed), encoding="utf-8")
            capsule_project.build_project(project, root / "covered.sqlite")

            contract["dependencies"] = []
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            with self.assertRaisesRegex(
                capsule_project.ProjectError, "cross-dataset foreign key"
            ):
                capsule_project.build_project(project, root / "undeclared.sqlite")

            contract["dependencies"] = [dependency]
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            domain_path.write_text(domain.replace("ON DELETE RESTRICT", "ON DELETE CASCADE"), encoding="utf-8")
            with self.assertRaisesRegex(
                capsule_project.ProjectError, "NO ACTION or RESTRICT"
            ):
                capsule_project.build_project(project, root / "cascade.sqlite")

    def test_plugin_versioned_runtime_and_conformance_assets_match_live_sources(self) -> None:
        skill = PLUGIN / "skills" / "create-capsule"
        pairs = (
            (ROOT / "runtime" / "capsule_host.py", skill / "assets" / "runtime" / "capsule_host.py"),
            (
                ROOT / "tools" / "capsule_conformance.py",
                skill / "assets" / "format" / "capsule_conformance.py",
            ),
            (
                ROOT / "runtime" / "browser" / "capsule-client.js",
                skill / "assets" / "runtime" / "browser" / "capsule-client.js",
            ),
            (ROOT / "format" / "capsule-v0.2.sql", skill / "assets" / "format" / "capsule-v0.2.sql"),
            (
                ROOT / "format" / "capsule-v0.2.conformance.json",
                skill / "assets" / "format" / "capsule-v0.2.conformance.json",
            ),
            (ROOT / "format" / "capsule-v0.3.sql", skill / "assets" / "format" / "capsule-v0.3.sql"),
            (
                ROOT / "format" / "capsule-v0.3.conformance.json",
                skill / "assets" / "format" / "capsule-v0.3.conformance.json",
            ),
            (
                ROOT
                / "docs"
                / "plans"
                / "capsule-lifecycle"
                / "contracts"
                / "template-state-v1.schema.json",
                skill / "assets" / "format" / "template-state-v1.schema.json",
            ),
        )
        for live, bundled in pairs:
            with self.subTest(asset=live.name):
                self.assertEqual(bundled.read_bytes(), live.read_bytes())

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

    def test_standalone_authoring_contract_freezes_native_lifecycle_policy_posture(self) -> None:
        skill = PLUGIN / "skills" / "create-capsule"
        contract = (skill / "references" / "authoring-contract.md").read_text(
            encoding="utf-8"
        )
        runtime = (skill / "references" / "format-and-runtime.md").read_text(
            encoding="utf-8"
        )
        skill_text = (skill / "SKILL.md").read_text(encoding="utf-8")
        for marker in (
            "Native copy and fork truth table",
            "complete inventory must be valid and digest-matching",
            "`copy` is copied and cannot be weakened to omit",
            "`forbid` rejects the operation",
            "rejects `reset` for fork and selective-fork",
            "proves zero freelist pages",
            "does not itself execute lifecycle copies",
            "`compare_policy` is a signed disclosure boundary",
            "Application-compartment expansion is separate",
            "`field` alone permits bounded scalar field projections",
            "BLOBs are always shown as length/hash",
            "`reconcile_policy` is a separate signed transformation ceiling",
            "Mutable lineage claims do not prove an ancestor",
            "three-way immutable-field conflict permits keep-target only",
            "has no reconcile",
            "executor or Tauri dependency",
            "Native same-schema application upgrade truth table",
            "SemVer 2.0.0",
            "never select authority",
            "`migrate` | Reject; restricted data-schema migration starts in M08",
            "has no upgrade",
        ):
            self.assertIn(marker, contract)
        self.assertIn("re-derives signed dataset actions", runtime)
        self.assertIn("separately retained clean source", runtime)
        self.assertIn("Reconciliation is also host-only", runtime)
        self.assertIn("value-free canonical plan", runtime)
        self.assertIn("publishes only a new", runtime)
        self.assertIn("Same-schema application upgrade is likewise host-only", runtime)
        self.assertIn("review is not authority", runtime)
        self.assertIn("rejects `migrate` and `forbid`", runtime)
        self.assertIn("does not execute lifecycle copies", skill_text)
        self.assertIn("signed disclosure ceiling", skill_text)
        self.assertIn("Application expansion is value-free", skill_text)
        self.assertIn("signed transformation authority", skill_text)
        self.assertIn("never applies a", skill_text)
        self.assertIn("reconciliation", skill_text)
        self.assertIn("same-schema application-upgrade target", skill_text)
        self.assertIn("strictly newer SemVer version", skill_text)
        self.assertIn("executes an upgrade", skill_text)


if __name__ == "__main__":
    unittest.main()

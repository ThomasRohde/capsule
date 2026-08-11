from __future__ import annotations

import base64
import gzip
import hashlib
import json
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.build_example import build_example  # noqa: E402
from tools.capsule_html import (  # noqa: E402
    HtmlExportError,
    SQLITE_JS_PATH,
    SQLITE_VERSION,
    SQLITE_WASM_PATH,
    export_html,
    inspect_html,
    verify_html,
)


def replace_block(document: str, block_id: str, value: str) -> str:
    pattern = rf'(<script id="{re.escape(block_id)}"[^>]*>).*?(</script>)'
    result, count = re.subn(
        pattern,
        lambda match: match.group(1) + value + match.group(2),
        document,
        count=1,
        flags=re.DOTALL,
    )
    if count != 1:
        raise AssertionError(f"Could not replace {block_id}")
    return result


class HtmlExportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls._class_temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls._class_temp.name)
        cls.capsule = cls.root / "diagram-studio.capsule.sqlite"
        cls.export = cls.root / "diagram-studio-view.html"
        build_example(cls.capsule)
        export_html(cls.capsule, cls.export, profile="view")

    @classmethod
    def tearDownClass(cls) -> None:
        cls._class_temp.cleanup()

    def modified_export(self, transform) -> Path:
        document = self.export.read_text(encoding="utf-8")
        target = self.root / f"modified-{hashlib.sha256(document.encode()).hexdigest()[:8]}-{id(transform)}.html"
        target.write_text(transform(document), encoding="utf-8")
        self.addCleanup(target.unlink, missing_ok=True)
        return target

    def metadata_transform(self, mutate):
        def transform(document: str) -> str:
            metadata = inspect_html(self.export)["metadata"]
            mutate(metadata)
            return replace_block(
                document,
                "sqlite-capsule-export-metadata",
                json.dumps(metadata, sort_keys=True, separators=(",", ":")),
            )

        return transform

    def test_all_profiles_are_deterministic_and_verifiable(self) -> None:
        source_before = self.capsule.read_bytes()
        for profile in ("view", "interactive", "editable"):
            with self.subTest(profile=profile):
                first = self.root / f"{profile}-first.html"
                second = self.root / f"{profile}-second.html"
                export_html(self.capsule, first, profile=profile)
                export_html(self.capsule, second, profile=profile)
                self.assertEqual(first.read_bytes(), second.read_bytes())
                report = verify_html(first)
                self.assertTrue(report["ok"])
                self.assertEqual(report["profile"], profile)
                export_html(self.capsule, first, profile=profile, check=True)
        self.assertEqual(self.capsule.read_bytes(), source_before)

    def test_export_verifies_the_exact_snapshot_read_before_a_source_race(self) -> None:
        source = self.root / "racing-source.capsule.sqlite"
        target = self.root / "racing-source.html"
        shutil.copyfile(self.capsule, source)
        original_read_bytes = Path.read_bytes
        raced = False

        def racing_read_bytes(path: Path) -> bytes:
            nonlocal raced
            data = original_read_bytes(path)
            if path.resolve() == source.resolve() and not raced:
                raced = True
                connection = sqlite3.connect(source)
                connection.execute(
                    "UPDATE capsule_asset SET sha256 = ? WHERE path = 'app/index.html'",
                    ("0" * 64,),
                )
                connection.commit()
                connection.close()
            return data

        with mock.patch.object(Path, "read_bytes", new=racing_read_bytes):
            result = export_html(source, target, profile="view")
        self.assertTrue(raced)
        self.assertTrue(result["ok"])
        self.assertTrue(verify_html(target)["ok"])
        with self.assertRaisesRegex(HtmlExportError, "verification failed"):
            export_html(source, self.root / "racing-invalid.html", profile="view")

    def test_inspection_is_non_executing_and_reports_exact_blocks(self) -> None:
        report = inspect_html(self.export)
        self.assertTrue(report["ok"])
        self.assertEqual(report["external_urls"], [])
        self.assertEqual(len(report["blocks"]), 7)
        self.assertGreater(report["metadata"]["runtime"]["notices_bytes"], 10_000)
        self.assertEqual(report["metadata"]["profile"], "view")
        self.assertEqual(report["metadata"]["runtime"]["sqlite_version"], SQLITE_VERSION)
        connection = sqlite3.connect(self.capsule)
        updated_at = connection.execute(
            "SELECT updated_at FROM capsule_manifest WHERE id = 1"
        ).fetchone()[0]
        connection.close()
        self.assertEqual(report["metadata"]["created_at"], updated_at)

    def test_exportability_report_resolves_static_assets_without_following_dynamic_code(self) -> None:
        target = self.root / "exportability.html"
        result = export_html(self.capsule, target, profile="interactive")
        report = result["exportability"]
        self.assertTrue(report["ok"])
        self.assertEqual(report["entry_asset"], "app/index.html")
        self.assertIn("app/app.js", report["resolved_capsule_assets"])
        self.assertEqual(report["runtime_network_policy"], "blocked-by-export-csp")

    def test_export_rejects_remote_static_entry_dependency(self) -> None:
        hostile = self.root / "remote-entry.capsule.sqlite"
        shutil.copyfile(self.capsule, hostile)
        connection = sqlite3.connect(hostile)
        try:
            row = connection.execute("SELECT content FROM capsule_asset WHERE path = 'app/index.html'").fetchone()
            content = bytes(row[0]).replace(b"</body>", b'<script src="https://example.invalid/x.js"></script></body>')
            connection.execute(
                "UPDATE capsule_asset SET content = ?, sha256 = ? WHERE path = 'app/index.html'",
                (content, hashlib.sha256(content).hexdigest()),
            )
            connection.commit()
        finally:
            connection.close()
        with self.assertRaisesRegex(HtmlExportError, "remote resource"):
            export_html(hostile, self.root / "must-not-exist.html", profile="view")

    def test_refuses_overwrite_without_replace_and_detects_stale_check(self) -> None:
        with self.assertRaisesRegex(HtmlExportError, "pass --replace"):
            export_html(self.capsule, self.export, profile="view")
        stale = self.root / "stale.html"
        stale.write_text("stale", encoding="utf-8")
        with self.assertRaisesRegex(HtmlExportError, "stale"):
            export_html(self.capsule, stale, profile="view", check=True)

    def test_repository_cli_routes_export_inspect_and_verify(self) -> None:
        target = self.root / "cli-interactive.html"
        commands = (
            ["export-html", str(self.capsule), str(target), "--profile", "interactive"],
            ["inspect-html", str(target)],
            ["verify-html", str(target)],
        )
        for command in commands:
            result = subprocess.run(
                [sys.executable, str(ROOT / "tools" / "capsule.py"), *command],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
            self.assertTrue(json.loads(result.stdout)["ok"])

    def test_rejects_duplicate_required_block(self) -> None:
        def transform(document: str) -> str:
            marker = '<script id="sqlite-capsule-database" type="application/octet-stream">'
            return document.replace(marker, marker + "</script>" + marker, 1)

        with self.assertRaisesRegex(HtmlExportError, "exactly one #sqlite-capsule-database"):
            verify_html(self.modified_export(transform))

    def test_rejects_noncanonical_base64(self) -> None:
        target = self.modified_export(
            lambda document: replace_block(document, "sqlite-capsule-database", "not-base64!")
        )
        with self.assertRaisesRegex(HtmlExportError, "canonical base64"):
            verify_html(target)

    def test_rejects_external_shell_url(self) -> None:
        target = self.modified_export(
            lambda document: document.replace("</head>", '<link href="https://example.invalid/x.css"></head>', 1)
        )
        with self.assertRaisesRegex(HtmlExportError, "external URLs"):
            verify_html(target)

    def test_rejects_unknown_metadata_field_and_profile(self) -> None:
        extra = self.modified_export(self.metadata_transform(lambda metadata: metadata.__setitem__("extra", True)))
        with self.assertRaisesRegex(HtmlExportError, "missing or unknown"):
            verify_html(extra)
        unknown = self.modified_export(self.metadata_transform(lambda metadata: metadata.__setitem__("profile", "admin")))
        with self.assertRaisesRegex(HtmlExportError, "Unknown HTML export profile"):
            verify_html(unknown)

    def test_rejects_unpinned_sqlite_version(self) -> None:
        target = self.modified_export(
            self.metadata_transform(lambda metadata: metadata["runtime"].__setitem__("sqlite_version", "latest"))
        )
        with self.assertRaisesRegex(HtmlExportError, "Unexpected SQLite WASM version"):
            verify_html(target)

    def test_rejects_payload_hash_and_decompression_size_tampering(self) -> None:
        metadata = inspect_html(self.export)["metadata"]
        compressed = base64.b64decode(
            "".join(
                re.search(
                    r'<script id="sqlite-capsule-database"[^>]*>(.*?)</script>',
                    self.export.read_text(encoding="utf-8"),
                    re.DOTALL,
                ).group(1).split()
            ),
            validate=True,
        )
        corrupt = bytearray(compressed)
        corrupt[-1] ^= 0x01
        target = self.modified_export(
            lambda document: replace_block(
                document,
                "sqlite-capsule-database",
                base64.encodebytes(bytes(corrupt)).decode("ascii"),
            )
        )
        with self.assertRaisesRegex(HtmlExportError, "compressed SHA-256 mismatch"):
            verify_html(target)

        def wrong_size(current_metadata):
            current_metadata["current"]["uncompressed_bytes"] -= 1

        wrong = self.modified_export(self.metadata_transform(wrong_size))
        with self.assertRaisesRegex(HtmlExportError, "decompressed to"):
            verify_html(wrong)
        self.assertEqual(len(gzip.decompress(compressed)), metadata["current"]["uncompressed_bytes"])

    def test_vendored_sqlite_runtime_is_the_reviewed_exact_build(self) -> None:
        self.assertEqual(
            hashlib.sha256(SQLITE_JS_PATH.read_bytes()).hexdigest(),
            "f80870f0fa03a39a3338d17ed3fbea04808d344c88e724d90d5f37b9b7b83154",
        )
        self.assertEqual(
            hashlib.sha256(SQLITE_WASM_PATH.read_bytes()).hexdigest(),
            "02d7e48164395fa68f81c6ec33e9da5461be397dc57602ac0cd89b4bbba1d312",
        )

    def test_loader_uses_a_shape_checked_classic_worker_for_file_origins(self) -> None:
        loader = (ROOT / "runtime" / "browser" / "loader.js").read_text(encoding="utf-8")
        self.assertIn("function classicSqliteWorkerSource", loader)
        self.assertIn('if (importMetaCount !== 4 || exportFooterCount !== 1)', loader)
        self.assertIn('worker = new Worker(workerUrl, { name: "sqlite-capsule-browser-host" })', loader)
        self.assertNotIn('worker = new Worker(workerUrl, { type: "module"', loader)
        self.assertIn("/<\\/style/i.test(source)", loader)
        self.assertNotIn("appUrl", loader)

    def test_browser_worker_revalidates_endpoint_declarations_at_call_time(self) -> None:
        worker = (ROOT / "runtime" / "browser" / "worker-host.js").read_text(
            encoding="utf-8"
        )
        execute = worker[worker.index("function executeEndpoint") : worker.index("async function initialise")]
        self.assertIn("looksLikeSingleStatement(statement.sql_text)", execute)
        self.assertIn("statementKind(statement.sql_text)", execute)
        self.assertIn("Endpoint parameters do not match SQL placeholders", execute)


if __name__ == "__main__":
    unittest.main()

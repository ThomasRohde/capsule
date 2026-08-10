from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "native" / "tools" / "collect_release_evidence.py"
SPEC = importlib.util.spec_from_file_location("collect_release_evidence", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeReleaseEvidenceTests(unittest.TestCase):
    def fixture(self, directory: Path, platform: str) -> tuple[Path, Path, Path, Path]:
        bundle = directory / "bundle"
        bundle.mkdir()
        artifact = {
            "windows": bundle / "msi" / "SQLiteCapsuleHost.msi",
            "macos": bundle / "dmg" / "SQLiteCapsuleHost.dmg",
            "linux": bundle / "appimage" / "SQLiteCapsuleHost.AppImage",
        }[platform]
        artifact.parent.mkdir()
        artifact.write_bytes(b"deterministic bundle fixture")
        config = directory / "tauri.conf.json"
        config.write_text(
            json.dumps(
                {
                    "identifier": "org.sqlite-capsule.host",
                    "productName": "SQLite Capsule Host",
                    "version": "0.2.0",
                }
            ),
            encoding="utf-8",
        )
        sbom = directory / "sbom.cdx.json"
        sbom.write_text("{}\n", encoding="utf-8")
        licenses = directory / "THIRD_PARTY_LICENSES.md"
        licenses.write_text("# fixture\n", encoding="utf-8")
        return bundle, config, sbom, licenses

    def evidence(self, directory: Path, platform: str = "windows") -> dict[str, object]:
        bundle, config, sbom, licenses = self.fixture(directory, platform)
        return MODULE.build_evidence(
            bundle_root=bundle,
            platform=platform,
            target={
                "windows": "x86_64-pc-windows-msvc",
                "macos": "aarch64-apple-darwin",
                "linux": "x86_64-unknown-linux-gnu",
            }[platform],
            source_revision="a" * 40,
            tauri_config=config,
            sbom=sbom,
            licenses=licenses,
            development_unsigned=True,
        )

    def test_development_inventory_is_deterministic_and_explicitly_unsigned(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            first = self.evidence(directory)
            encoded = MODULE.encoded_evidence(first)
            self.assertEqual(first["format"], "org.sqlite-capsule.host-build-evidence/0.2")
            self.assertTrue(first["build"]["development_unsigned"])
            self.assertFalse(first["build"]["source_dirty"])
            self.assertEqual(first["build"]["platform_signing"], "not_claimed")
            self.assertEqual(first["artifacts"][0]["kind"], "windows-msi")
            self.assertEqual(encoded, MODULE.encoded_evidence(first))

    def test_dirty_worktree_is_recorded_without_changing_unsigned_claim(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle, config, sbom, licenses = self.fixture(directory, "windows")
            evidence = MODULE.build_evidence(
                bundle_root=bundle,
                platform="windows",
                target="x86_64-pc-windows-msvc",
                source_revision="d" * 40,
                tauri_config=config,
                sbom=sbom,
                licenses=licenses,
                development_unsigned=True,
                source_dirty=True,
            )
            self.assertTrue(evidence["build"]["source_dirty"])
            self.assertTrue(evidence["build"]["development_unsigned"])
            self.assertEqual(evidence["build"]["platform_signing"], "not_claimed")

    def test_artifact_classifier_rejects_cross_platform_bundle_mismatches(self) -> None:
        for platform in ("windows", "macos", "linux"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as temporary:
                evidence = self.evidence(Path(temporary), platform)
                self.assertEqual(evidence["build"]["platform"], platform)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle, config, sbom, licenses = self.fixture(directory, "windows")
            with self.assertRaises(MODULE.EvidenceError):
                MODULE.build_evidence(
                    bundle_root=bundle,
                    platform="linux",
                    target="x86_64-unknown-linux-gnu",
                    source_revision="b" * 40,
                    tauri_config=config,
                    sbom=sbom,
                    licenses=licenses,
                    development_unsigned=True,
                )

    def test_signed_claims_and_symlinked_artifacts_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle, config, sbom, licenses = self.fixture(directory, "windows")
            with self.assertRaises(MODULE.EvidenceError):
                MODULE.build_evidence(
                    bundle_root=bundle,
                    platform="windows",
                    target="x86_64-pc-windows-msvc",
                    source_revision="c" * 40,
                    tauri_config=config,
                    sbom=sbom,
                    licenses=licenses,
                    development_unsigned=False,
                )
            if hasattr(Path, "symlink_to"):
                link = bundle / "nsis" / "linked.exe"
                link.parent.mkdir()
                try:
                    link.symlink_to(bundle / "msi" / "SQLiteCapsuleHost.msi")
                except OSError:
                    self.skipTest("symlink creation is unavailable")
                with self.assertRaises(MODULE.EvidenceError):
                    MODULE.discover_artifacts(bundle)

    def test_ci_is_lean_and_installer_qualification_is_release_only(self) -> None:
        workflows = ROOT / ".github/workflows"
        ci = (workflows / "ci.yml").read_text(encoding="utf-8")
        release = (workflows / "release.yml").read_text(encoding="utf-8")
        self.assertFalse((workflows / "native-matrix.yml").exists())

        self.assertIn("pull_request:", ci)
        self.assertIn("branches: [main, master]", ci)
        self.assertIn("contents: read", ci)
        self.assertNotIn("contents: write", ci)
        self.assertIn("python tools/build_exports.py --check", ci)
        self.assertIn("cargo clippy --workspace --all-targets", ci)
        self.assertNotIn("build_installers.py", ci)
        self.assertNotIn("windows-nsis-installer.ps1", ci)
        self.assertNotIn("actions/upload-artifact", ci)

        self.assertIn('tags: ["v*"]', release)
        self.assertIn("workflow_dispatch:", release)
        self.assertNotIn("pull_request:", release)
        self.assertIn("python native/tools/build_installers.py", release)
        self.assertIn("tests/native/windows-nsis-installer.ps1", release)
        self.assertIn("npm run test:native", release)
        self.assertIn("npm run test:native:raw", release)
        self.assertNotIn("npm run test:native:window", release)
        self.assertIn("actions/upload-artifact@v7", release)
        self.assertIn("actions/download-artifact@v8", release)
        self.assertIn("contents: write", release)
        self.assertIn("gh release create", release)
        self.assertIn("--development-unsigned", release)
        self.assertNotIn("tauri-apps/tauri-action", release)
        self.assertNotIn("TAURI_SIGNING_PRIVATE_KEY", release)

    def test_windows_installers_retain_one_fixed_bootstrap_package(self) -> None:
        source = ROOT / "native" / "desktop" / "src-tauri"
        config = json.loads((source / "tauri.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(
            config["plugins"]["updater"],
            {"endpoints": [], "pubkey": ""},
            "the plugin must deserialize at runtime but remain inert until the Rust-only "
            "compiled updater builder supplies complete pinned configuration",
        )
        windows = config["bundle"]["windows"]
        self.assertTrue(windows["allowDowngrades"])
        self.assertEqual(
            windows["wix"]["fragmentPaths"],
            ["./windows/fragments/installer-cache.wxs"],
        )
        self.assertEqual(
            windows["wix"]["componentRefs"], ["SQLiteCapsuleInstallerCache"]
        )
        self.assertEqual(
            windows["nsis"]["installerHooks"], "./windows/installer-hooks.nsh"
        )

        hooks = (source / "windows" / "installer-hooks.nsh").read_text(
            encoding="utf-8"
        )
        self.assertIn('CopyFiles /SILENT "$EXEPATH"', hooks)
        self.assertIn("sqlite-capsule-host-current.exe", hooks)
        self.assertIn("NSIS_HOOK_PREUNINSTALL", hooks)
        self.assertIn("NSIS_HOOK_POSTUNINSTALL", hooks)
        self.assertIn('"AssociationBackupWasPresent"', hooks)
        self.assertIn(
            'DeleteRegValue SHCTX "Software\\Classes\\.sqlitecapsule" '
            '"SQLite Capsule_backup"',
            hooks,
        )
        self.assertIn('DeleteRegKey SHCTX "${MANUPRODUCTKEY}"', hooks)
        self.assertIn("!insertmacro UPDATEFILEASSOC", hooks)
        self.assertIn(
            'WriteRegStr SHCTX "Software\\Classes\\SQLite Capsule\\shell\\open\\command"',
            hooks,
        )
        self.assertIn('$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\" $\\"%1$\\"', hooks)
        self.assertIn("Abort", hooks)

        installer_acceptance = (
            ROOT / "tests" / "native" / "windows-nsis-installer.ps1"
        ).read_text(encoding="utf-8")
        self.assertIn("Assert-NoHostProductState", installer_acceptance)
        self.assertIn("AssociationBackupWasPresent", installer_acceptance)
        self.assertIn("same-version reinstall", installer_acceptance)
        self.assertIn("preserved_post_install_user_choice", installer_acceptance)
        self.assertIn("The open command does not quote both", installer_acceptance)
        self.assertIn("application_data_before", installer_acceptance)
        self.assertNotIn("SQLITE_CAPSULE_NATIVE_E2E", installer_acceptance)
        self.assertNotIn("Stop-Process", installer_acceptance)
        self.assertNotIn("taskkill", installer_acceptance.lower())
        self.assertNotIn("Remove-Item -LiteralPath $installDir", installer_acceptance)

        fragment = (
            source / "windows" / "fragments" / "installer-cache.wxs"
        ).read_text(encoding="utf-8")
        self.assertIn('SourceProperty="OriginalDatabase"', fragment)
        self.assertIn('DestinationName="sqlite-capsule-host-current.msi"', fragment)
        self.assertIn('On="uninstall"', fragment)


if __name__ == "__main__":
    unittest.main()

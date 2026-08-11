from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "native" / "tools" / "build_installers.py"
SPEC = importlib.util.spec_from_file_location("build_installers", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeInstallerBuildTests(unittest.TestCase):
    def test_cli_version_is_exactly_pinned(self) -> None:
        self.assertEqual(MODULE.PINNED_TAURI_CLI_VERSION, "2.11.4")
        self.assertEqual(
            MODULE.VERSION_PATTERN.fullmatch("tauri-cli 2.11.4").group(1),
            "2.11.4",
        )
        other = MODULE.VERSION_PATTERN.fullmatch("tauri-cli 2.11.5")
        self.assertIsNotNone(other)
        self.assertNotEqual(other.group(1), MODULE.PINNED_TAURI_CLI_VERSION)

    def test_default_build_is_debug_and_nsis_only(self) -> None:
        self.assertEqual(MODULE.DEFAULT_BUNDLES, "nsis")
        self.assertFalse(MODULE.DEFAULT_RELEASE)
        self.assertEqual(
            MODULE.bundle_root("x86_64-pc-windows-msvc"),
            MODULE.NATIVE_ROOT / "target/x86_64-pc-windows-msvc/debug/bundle",
        )
        self.assertEqual(
            MODULE.bundle_root("x86_64-pc-windows-msvc", release=True),
            MODULE.NATIVE_ROOT / "target/x86_64-pc-windows-msvc/release/bundle",
        )

    def test_commands_separate_build_bundle_and_profile_modes(self) -> None:
        cli = Path("cargo-tauri.exe")
        debug_build = MODULE.build_command(cli, "target", ("nsis",))
        self.assertEqual(debug_build[1], "build")
        self.assertIn("--debug", debug_build)

        debug_bundle = MODULE.build_command(
            cli, "target", ("nsis",), bundle_only=True
        )
        self.assertEqual(debug_bundle[1], "bundle")
        self.assertIn("--debug", debug_bundle)

        release_build = MODULE.build_command(
            cli, "target", ("nsis",), release=True
        )
        self.assertEqual(release_build[1], "build")
        self.assertNotIn("--debug", release_build)

    def test_bundle_only_requires_a_regular_existing_executable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "host.exe"
            with self.assertRaisesRegex(RuntimeError, "requires an existing executable"):
                MODULE.require_built_executable(executable)
            executable.write_bytes(b"built host")
            MODULE.require_built_executable(executable)

    def test_cleanup_removes_only_generated_installer_candidates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "bundle"
            msi = root / "msi"
            nsis = root / "nsis"
            msi.mkdir(parents=True)
            nsis.mkdir()
            stale_msi = msi / "old.msi"
            stale_nsis = nsis / "old-setup.exe"
            keep_msi = msi / "old.wixpdb"
            keep_nsis = nsis / "helper.exe"
            for path in (stale_msi, stale_nsis, keep_msi, keep_nsis):
                path.write_bytes(path.name.encode("ascii"))

            removed = MODULE.clean_generated_installers(root, ("msi", "nsis"))

            self.assertEqual(set(removed), {stale_msi, stale_nsis})
            self.assertFalse(stale_msi.exists())
            self.assertFalse(stale_nsis.exists())
            self.assertTrue(keep_msi.exists())
            self.assertTrue(keep_nsis.exists())

    def test_documentation_uses_the_reproducible_wrapper(self) -> None:
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        native_readme = (ROOT / "native" / "README.md").read_text(encoding="utf-8")
        release_workflow = (ROOT / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("python native\\tools\\build_installers.py", readme)
        self.assertIn("python native/tools/build_installers.py", native_readme)
        self.assertIn("--bundle-only", readme)
        self.assertIn("--release", native_readme)
        self.assertIn("--release", release_workflow)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "native" / "tools" / "export_installers.py"
SPEC = importlib.util.spec_from_file_location("export_installers", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeInstallerExportTests(unittest.TestCase):
    def bundle_fixture(self, directory: Path) -> Path:
        bundle_root = directory / "bundle"
        nsis = bundle_root / "nsis" / "SQLite Capsule Host_0.3.0_x64-setup.exe"
        msi = bundle_root / "msi" / "SQLite Capsule Host_0.3.0_x64_en-US.msi"
        nsis.parent.mkdir(parents=True)
        msi.parent.mkdir(parents=True)
        nsis.write_bytes(b"nsis fixture")
        msi.write_bytes(b"msi fixture")
        return bundle_root

    def test_exports_stable_names_without_disturbing_capsules(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle_root = self.bundle_fixture(directory)
            output = directory / "capsules"
            output.mkdir()
            capsule = output / "diagram-studio.capsule.sqlite"
            capsule.write_bytes(b"capsule fixture")

            exported = MODULE.export_installers(bundle_root, output)

            self.assertEqual(
                [Path(item["output"]).name for item in exported],
                ["sqlite-capsule-host-setup.exe"],
            )
            self.assertEqual(
                (output / "sqlite-capsule-host-setup.exe").read_bytes(),
                b"nsis fixture",
            )
            self.assertFalse((output / "sqlite-capsule-host.msi").exists())
            self.assertEqual(capsule.read_bytes(), b"capsule fixture")

    def test_explicit_msi_and_nsis_export_both_stable_names(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle_root = self.bundle_fixture(directory)
            output = directory / "capsules"

            exported = MODULE.export_installers(
                bundle_root, output, ("msi", "nsis")
            )

            self.assertEqual(
                [Path(item["output"]).name for item in exported],
                ["sqlite-capsule-host-setup.exe", "sqlite-capsule-host.msi"],
            )

    def test_default_refreshes_only_the_nsis_stable_installer(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle_root = self.bundle_fixture(directory)
            output = directory / "capsules"
            output.mkdir()
            (output / "sqlite-capsule-host-setup.exe").write_bytes(b"old exe")
            (output / "sqlite-capsule-host.msi").write_bytes(b"old msi")
            other = output / "keep.txt"
            other.write_text("keep", encoding="utf-8")

            MODULE.export_installers(bundle_root, output)

            self.assertEqual(
                (output / "sqlite-capsule-host-setup.exe").read_bytes(),
                b"nsis fixture",
            )
            self.assertEqual(
                (output / "sqlite-capsule-host.msi").read_bytes(), b"old msi"
            )
            self.assertEqual(other.read_text(encoding="utf-8"), "keep")

    def test_missing_or_ambiguous_bundle_fails_before_export(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            bundle_root = self.bundle_fixture(directory)
            duplicate = bundle_root / "nsis" / "another-setup.exe"
            duplicate.write_bytes(b"ambiguous")
            output = directory / "capsules"

            with self.assertRaises(MODULE.ExportError):
                MODULE.export_installers(bundle_root, output)

            self.assertFalse(output.exists())

    def test_repository_surfaces_the_capsules_destination(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertIn("python native/tools/build_installers.py", workflow)
        self.assertIn("python native\\tools\\build_installers.py", readme)
        self.assertIn("capsules\\sqlite-capsule-host-setup.exe", readme)
        self.assertIn("--bundles msi", readme)


if __name__ == "__main__":
    unittest.main()

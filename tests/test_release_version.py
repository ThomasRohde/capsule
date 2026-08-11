from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "native" / "tools" / "check_release_version.py"
SPEC = importlib.util.spec_from_file_location("check_release_version", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseVersionTests(unittest.TestCase):
    def test_current_package_versions_and_tag_match(self) -> None:
        version, sources = MODULE.verify_release_version("v0.3.0")
        self.assertEqual(version, "0.3.0")
        self.assertGreaterEqual(len(sources), 18)
        self.assertEqual(set(sources.values()), {"0.3.0"})

    def test_mismatched_or_noncanonical_tag_is_rejected(self) -> None:
        with self.assertRaises(MODULE.ReleaseVersionError):
            MODULE.verify_release_version("v0.3.1")
        with self.assertRaises(MODULE.ReleaseVersionError):
            MODULE.verify_release_version("0.3.0")


if __name__ == "__main__":
    unittest.main()

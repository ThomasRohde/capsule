from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class CompactStateVectorTests(unittest.TestCase):
    def test_independent_compact_logical_state_vectors_pass(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(ROOT / "tools" / "check_compact_state_vectors.py")],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
        self.assertIn("compact logical-state vectors: PASS", completed.stdout)

    def test_compact_plan_vector_passes_the_independent_checker(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools" / "check_lifecycle_plan_vectors.py"),
                "--vector-dir",
                str(ROOT / "compatibility" / "compact-copy-plan-v1"),
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
        self.assertIn("lifecycle plan vectors: PASS", completed.stdout)


if __name__ == "__main__":
    unittest.main()

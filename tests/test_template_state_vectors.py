from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class TemplateStateVectorTests(unittest.TestCase):
    def test_independent_dataset_state_vectors_pass(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(ROOT / "tools" / "check_template_state_vectors.py")],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
        self.assertIn("template-state vectors: PASS", completed.stdout)

    def test_vectors_cover_all_sqlite_storage_class_families(self) -> None:
        vectors = json.loads(
            (ROOT / "compatibility" / "template-state-v1" / "vectors.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(vectors["profile"], "org.sqlite-capsule.template-state-vectors/1")
        self.assertIn("type-spectrum", vectors["datasets"])
        spectrum_sql = (
            ROOT / "compatibility" / "template-state-v1" / "type-spectrum.sql"
        ).read_text(encoding="utf-8")
        for marker in (
            "GENERATED ALWAYS",
            "VIRTUAL",
            "NULL",
            "-42",
            "WITHOUT ROWID",
            "PRIMARY KEY (part_a, part_b)",
            "vector_empty_state",
            "é",
            "é",
        ):
            self.assertIn(marker, spectrum_sql)
        self.assertNotEqual("é".encode("utf-8"), "é".encode("utf-8"))


if __name__ == "__main__":
    unittest.main()

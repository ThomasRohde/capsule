from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VECTORS = ROOT / "compatibility" / "compare-row-v1" / "vectors.json"


class CompareRowVectorTests(unittest.TestCase):
    def test_independent_compare_row_vectors_pass(self) -> None:
        result = subprocess.run(
            [sys.executable, "tools/check_compare_row_vectors.py"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("3 compare rows and 4 hostile values", result.stdout)

    def test_vectors_freeze_required_type_and_identity_edges(self) -> None:
        document = json.loads(VECTORS.read_text(encoding="utf-8"))
        encoded = json.dumps(document, ensure_ascii=False)
        for marker in (
            '"decimal": "-9223372036854775808"',
            '"decimal": "9223372036854775807"',
            '"hex": "8000000000000000"',
            '"hex": "0000000000000000"',
            '"utf8_hex": "c3a9"',
            '"utf8_hex": "65cc81"',
            '"type": "blob"',
            '"type": "null"',
        ):
            self.assertIn(marker, encoded)
        self.assertTrue(all(case["key_bytes_hex"] for case in document["cases"]))
        self.assertTrue(all(case["row_sha256"] for case in document["cases"]))


if __name__ == "__main__":
    unittest.main()

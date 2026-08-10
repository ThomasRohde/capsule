from __future__ import annotations

import unittest

from tools.check_signed_app_vectors import verify_vectors


class SignedApplicationVectorTests(unittest.TestCase):
    def test_independent_canonical_vectors_and_mutation_contract(self) -> None:
        result = verify_vectors()
        self.assertEqual(result["profile"], "org.sqlite-capsule.signed-app/0.2")
        self.assertEqual(
            [fixture["name"] for fixture in result["fixtures"]],
            ["format-v0.2"],
        )


if __name__ == "__main__":
    unittest.main()

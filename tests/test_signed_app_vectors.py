from __future__ import annotations

import unittest

from tools.check_signed_app_vectors import (
    V03_VECTOR_FILE,
    VectorError,
    build_fixture,
    canonical_json,
    canonical_stream,
    verify_all_vectors,
    verify_vectors,
)


class SignedApplicationVectorTests(unittest.TestCase):
    def test_independent_canonical_vectors_and_mutation_contract(self) -> None:
        result = verify_vectors()
        self.assertEqual(result["profile"], "org.sqlite-capsule.signed-app/0.2")
        self.assertEqual(
            [fixture["name"] for fixture in result["fixtures"]],
            ["format-v0.2"],
        )

    def test_v02_and_v03_vectors_are_checked_together(self) -> None:
        result = verify_all_vectors()
        self.assertEqual(
            [profile["profile"] for profile in result["profiles"]],
            [
                "org.sqlite-capsule.signed-app/0.2",
                "org.sqlite-capsule.signed-app/0.3",
            ],
        )
        self.assertEqual(
            result["profiles"][1]["fixtures"][0]["name"],
            "format-v0.3",
        )

    def test_independent_jcs_number_serialization_matches_rfc_8785(self) -> None:
        source = "[333333333.33333329,1E30,4.50,2e-3,0.000000000000000000000000001,-0.0]"
        self.assertEqual(
            canonical_json(source),
            b"[333333333.3333333,1e+30,4.5,0.002,1e-27,0]",
        )

    def test_independent_jcs_number_boundaries_match_ecmascript(self) -> None:
        source = (
            "[0.000001,0.0000001,1e20,1e21,5e-324,"
            "1.7976931348623157e308,2.2250738585072014e-308,-0.0]"
        )
        self.assertEqual(
            canonical_json(source),
            (
                b"[0.000001,1e-7,100000000000000000000,1e+21,5e-324,"
                b"1.7976931348623157e+308,2.2250738585072014e-308,0]"
            ),
        )

    def test_independent_jcs_orders_names_by_utf16_and_rejects_surrogates(self) -> None:
        self.assertEqual(
            canonical_json(r'{"\r":1,"\u20ac":2,"a":3,"\ud83d\ude00":4}'),
            b'{"\\r":1,"a":3,"\xe2\x82\xac":2,"\xf0\x9f\x98\x80":4}',
        )
        with self.assertRaises(VectorError):
            canonical_json(r'{"unpaired":"\ud800"}')

    def test_independent_jcs_rejects_duplicate_keys_and_non_finite_numbers(self) -> None:
        for source in ('{"a":1,"a":2}', '{"a":NaN}'):
            with self.subTest(source=source), self.assertRaises(VectorError):
                canonical_json(source)

    def test_canonical_dispatch_rejects_every_v03_tuple_mismatch(self) -> None:
        vector = __import__("json").loads(V03_VECTOR_FILE.read_text(encoding="utf-8"))[
            "fixtures"
        ][0]
        for name, sql in {
            "application_id": "PRAGMA application_id = 1",
            "format_id": "UPDATE capsule_manifest SET format_id = 'wrong' WHERE id = 1",
            "format_version": "UPDATE capsule_manifest SET format_version = '9.9' WHERE id = 1",
            "runtime_protocol": "UPDATE capsule_manifest SET runtime_protocol = 'wrong' WHERE id = 1",
            "minimum_host_profile": "UPDATE capsule_manifest SET minimum_host_profile = 'wrong' WHERE id = 1",
        }.items():
            with self.subTest(name=name):
                connection = build_fixture(vector)
                connection.execute("PRAGMA ignore_check_constraints=ON")
                connection.execute(sql)
                with self.assertRaises(VectorError):
                    canonical_stream(connection)
                connection.close()


if __name__ == "__main__":
    unittest.main()

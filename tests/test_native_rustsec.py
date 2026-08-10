import copy
import datetime as dt
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "native" / "tools" / "check_rustsec.py"
SPEC = importlib.util.spec_from_file_location("check_rustsec", MODULE_PATH)
check_rustsec = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(check_rustsec)


class RustSecGateTests(unittest.TestCase):
    def setUp(self):
        self.exception = {
            "id": "RUSTSEC-2026-0001",
            "package": "example-crate",
            "version": "1.2.3",
            "kind": "unmaintained",
            "targets": ["linux"],
            "dependency_path": "host -> example-crate 1.2.3 on Linux.",
            "api_reachability": "The package is linked, but no vulnerable API is reported.",
            "rationale": "This fixture models a reviewed maintenance warning only.",
            "compensating_controls": ["The exact dependency graph is pinned and tested."],
            "removal_condition": "Remove the exception when the dependency disappears.",
            "owner": "Milestone test owner",
            "review_by": "2026-09-30",
        }
        self.report = {
            "vulnerabilities": {"found": False, "count": 0, "list": []},
            "warnings": {
                "unmaintained": [
                    {
                        "kind": "unmaintained",
                        "package": {"name": "example-crate", "version": "1.2.3"},
                        "advisory": {"id": "RUSTSEC-2026-0001"},
                    }
                ]
            },
        }

    def load(self, exception=None, today=dt.date(2026, 8, 9)):
        document = {
            "profile": check_rustsec.PROFILE,
            "cargo_audit_version": "0.22.2",
            "exceptions": [exception or self.exception],
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "exceptions.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            return check_rustsec.load_exceptions(path, today)

    def test_exact_reviewed_warning_passes(self):
        accepted = check_rustsec.evaluate_report(self.report, self.load())
        self.assertEqual(["RUSTSEC-2026-0001"], [item["id"] for item in accepted])

    def test_new_warning_fails(self):
        report = copy.deepcopy(self.report)
        report["warnings"]["unmaintained"].append(
            {
                "kind": "unmaintained",
                "package": {"name": "new-crate", "version": "9.9.9"},
                "advisory": {"id": "RUSTSEC-2026-0002"},
            }
        )
        with self.assertRaisesRegex(check_rustsec.GateError, "unreviewed"):
            check_rustsec.evaluate_report(report, self.load())

    def test_package_or_version_drift_fails(self):
        report = copy.deepcopy(self.report)
        report["warnings"]["unmaintained"][0]["package"]["version"] = "1.2.4"
        with self.assertRaisesRegex(check_rustsec.GateError, "finding changed"):
            check_rustsec.evaluate_report(report, self.load())

    def test_stale_exception_fails(self):
        report = copy.deepcopy(self.report)
        report["warnings"] = {}
        with self.assertRaisesRegex(check_rustsec.GateError, "stale"):
            check_rustsec.evaluate_report(report, self.load())

    def test_expired_exception_fails(self):
        with self.assertRaisesRegex(check_rustsec.GateError, "expired"):
            self.load(today=dt.date(2026, 10, 1))

    def test_vulnerability_is_never_excepted(self):
        report = copy.deepcopy(self.report)
        report["vulnerabilities"] = {
            "found": True,
            "count": 1,
            "list": [{"advisory": {"id": "RUSTSEC-2026-9999"}}],
        }
        with self.assertRaisesRegex(check_rustsec.GateError, "never excepted"):
            check_rustsec.evaluate_report(report, self.load())

    def test_target_reachability_drift_fails(self):
        exceptions = self.load()
        actual = {("example-crate", "1.2.3"): {"linux", "windows"}}
        with self.assertRaisesRegex(check_rustsec.GateError, "target reachability"):
            check_rustsec.evaluate_target_reachability(exceptions, actual)

    def test_unknown_exception_field_fails(self):
        exception = copy.deepcopy(self.exception)
        exception["silent_ignore"] = True
        with self.assertRaisesRegex(check_rustsec.GateError, "unknown"):
            self.load(exception=exception)


if __name__ == "__main__":
    unittest.main()

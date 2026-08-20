from __future__ import annotations

import json
import shutil
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = ROOT / "docs" / "plans" / "capsule-lifecycle" / "contracts"
EXAMPLES = ROOT / "docs" / "plans" / "capsule-lifecycle" / "examples"
SCHEMA = CONTRACTS / "cli-reconcile-v1.schema.json"
FILES = [
    EXAMPLES / "cli-reconcile-candidates.json",
    EXAMPLES / "cli-reconcile-three-way-candidates.json",
    EXAMPLES / "cli-reconcile-review.json",
    EXAMPLES / "cli-reconcile-result.json",
]


class CliReconcileContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.schema = json.loads(SCHEMA.read_text(encoding="utf-8"))

    def test_roots_are_closed_bounded_and_value_free(self) -> None:
        definitions = self.schema["$defs"]
        for name in ("candidates", "three_way_candidates", "review", "result", "row", "field", "conflict"):
            self.assertFalse(definitions[name]["additionalProperties"], name)
        self.assertEqual(definitions["candidates"]["properties"]["rows"]["maxItems"], 100)
        self.assertEqual(definitions["three_way_candidates"]["properties"]["conflicts"]["maxItems"], 10_000)
        self.assertEqual(definitions["review"]["properties"]["operation_count"]["maximum"], 10_000)
        candidate_text = FILES[0].read_text(encoding="utf-8")
        three_way_text = FILES[1].read_text(encoding="utf-8")
        for secret in ("source insert", "target delete", "C:\\", "/tmp/"):
            self.assertNotIn(secret, candidate_text)
            self.assertNotIn(secret, three_way_text)

    def test_review_embeds_the_frozen_plan_and_payload_profiles(self) -> None:
        review = json.loads(FILES[2].read_text(encoding="utf-8"))
        self.assertEqual(review["plan"]["profile"], "org.sqlite-capsule.lifecycle-plan/1")
        self.assertEqual(review["payload"]["profile"], "org.sqlite-capsule.reconcile-payload/1")
        self.assertFalse(review["executable_authority"])
        self.assertEqual(review["operation_count"], len(review["payload"]["operations"]))

    def test_review_plan_and_payload_validate_against_dedicated_contracts(self) -> None:
        powershell = shutil.which("pwsh")
        if powershell is None:
            self.skipTest("PowerShell Test-Json is unavailable")
        review = json.loads(FILES[2].read_text(encoding="utf-8"))
        for member, schema_name in (
            ("plan", "lifecycle-plan-v1.schema.json"),
            ("payload", "reconcile-plan-v1.schema.json"),
        ):
            schema = str(CONTRACTS / schema_name).replace("'", "''")
            encoded = json.dumps(review[member], separators=(",", ":")).replace("'", "''")
            command = (
                f"$json='{encoded}'; "
                f"if (-not ($json | Test-Json -SchemaFile '{schema}' -ErrorAction SilentlyContinue)) "
                "{ exit 1 }"
            )
            completed = subprocess.run(
                [powershell, "-NoProfile", "-Command", command],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr or member)

    def test_all_examples_validate_with_draft_2020_12(self) -> None:
        powershell = shutil.which("pwsh")
        if powershell is None:
            self.skipTest("PowerShell Test-Json is unavailable")
        schema = str(SCHEMA).replace("'", "''")
        for example in FILES:
            source = str(example).replace("'", "''")
            command = (
                f"$json=Get-Content -Raw -LiteralPath '{source}'; "
                f"if (-not ($json | Test-Json -SchemaFile '{schema}' -ErrorAction SilentlyContinue)) "
                "{ exit 1 }"
            )
            completed = subprocess.run(
                [powershell, "-NoProfile", "-Command", command],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr or example.name)


if __name__ == "__main__":
    unittest.main()

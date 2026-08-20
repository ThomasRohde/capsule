from __future__ import annotations

import json
import shutil
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = (
    ROOT
    / "docs"
    / "plans"
    / "capsule-lifecycle"
    / "contracts"
    / "tauri-reconcile-v1.schema.json"
)
EXAMPLE_DIR = ROOT / "docs" / "plans" / "capsule-lifecycle" / "examples"
EXAMPLES = [
    EXAMPLE_DIR / "tauri-reconcile-choose-ancestor.json",
    EXAMPLE_DIR / "tauri-reconcile-prepare-two-way.json",
    EXAMPLE_DIR / "tauri-reconcile-prepare-three-way.json",
    EXAMPLE_DIR / "tauri-reconcile-three-way.json",
    EXAMPLE_DIR / "tauri-reconcile-options.json",
    EXAMPLE_DIR / "tauri-reconcile-session.json",
    EXAMPLE_DIR / "tauri-reconcile-review.json",
    EXAMPLE_DIR / "tauri-reconcile-progress.json",
    EXAMPLE_DIR / "tauri-reconcile-status.json",
]


class TauriReconcileContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.schema = json.loads(SCHEMA.read_text(encoding="utf-8"))

    def test_every_serialized_object_is_closed_and_bounded(self) -> None:
        definitions = self.schema["$defs"]
        for name, definition in definitions.items():
            if definition.get("type") == "object":
                self.assertFalse(definition.get("additionalProperties"), name)
        self.assertEqual(definitions["token"]["pattern"], "^[A-Za-z0-9_-]{43}$")
        self.assertEqual(
            definitions["prepare_two_way_request"]["properties"]["selection_tokens"]["maxItems"],
            10_000,
        )
        self.assertEqual(
            definitions["prepare_three_way_request"]["properties"]["resolution_tokens"]["maxItems"],
            10_000,
        )
        self.assertEqual(definitions["prepared"]["properties"]["lineage_parent_count"]["const"], 2)

    def test_prepare_examples_are_token_only_and_mutually_exclusive(self) -> None:
        choose_ancestor = json.loads(EXAMPLES[0].read_text(encoding="utf-8"))
        two_way = json.loads(EXAMPLES[1].read_text(encoding="utf-8"))
        three_way = json.loads(EXAMPLES[2].read_text(encoding="utf-8"))
        allowed = {
            "review_token",
            "destination_token",
            "selection_tokens",
            "ancestor_token",
            "resolution_tokens",
        }
        self.assertEqual(set(two_way), allowed)
        self.assertEqual(set(three_way), allowed)
        self.assertEqual(set(choose_ancestor), {"review_token", "destination_token"})
        self.assertTrue(two_way["selection_tokens"])
        self.assertIsNone(two_way["ancestor_token"])
        self.assertEqual(two_way["resolution_tokens"], [])
        self.assertEqual(three_way["selection_tokens"], [])
        self.assertIsInstance(three_way["ancestor_token"], str)
        self.assertTrue(three_way["resolution_tokens"])
        forbidden = {"path", "sql", "value", "key", "digest", "index", "table", "column"}
        self.assertTrue(forbidden.isdisjoint(two_way))
        self.assertTrue(forbidden.isdisjoint(three_way))

    def test_three_way_projection_exposes_only_opaque_resolution_authority(self) -> None:
        projection = json.loads(EXAMPLES[3].read_text(encoding="utf-8"))
        self.assertEqual(projection["profile"], "org.sqlite-capsule.tauri-reconcile-three-way/1")
        self.assertEqual(
            set(projection),
            {"profile", "ancestor_token", "ancestor", "clean_change_count", "conflicts", "expires_at"},
        )
        for conflict in projection["conflicts"]:
            self.assertLessEqual(
                set(conflict),
                {"conflict_token", "dataset_label", "table_label", "kind", "deleted_side", "choices"},
            )
            self.assertNotIn("conflict_id", conflict)
            for choice in conflict["choices"]:
                self.assertEqual(set(choice), {"resolution_token", "choice"})
        immutable = next(
            conflict for conflict in projection["conflicts"] if conflict["kind"] == "immutable-field"
        )
        self.assertEqual(
            [choice["choice"] for choice in immutable["choices"]],
            ["keep-target"],
        )

    def test_examples_validate_against_draft_2020_12_schema(self) -> None:
        powershell = shutil.which("pwsh")
        if powershell is None:
            self.skipTest("PowerShell Test-Json is unavailable")
        schema = str(SCHEMA).replace("'", "''")
        for example in EXAMPLES:
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

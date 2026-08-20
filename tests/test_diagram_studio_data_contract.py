import json
import sqlite3
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "examples" / "diagram-studio" / "source" / "data-contract.json"
DOMAIN_SQL = ROOT / "examples" / "diagram-studio" / "domain.sql"
PROJECTION = ROOT / "docs" / "plans" / "capsule-lifecycle" / "examples" / "diagram-studio-data-contract.json"
COPY_FIXTURES = ROOT / "examples" / "diagram-studio" / "source" / "copy-fixtures.json"
RECONCILE_FIXTURES = ROOT / "examples" / "diagram-studio" / "source" / "reconcile-fixtures.json"


class DiagramStudioDataContractTests(unittest.TestCase):
    def test_reviewable_v03_contract_exactly_covers_domain_tables_and_keys(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        connection = sqlite3.connect(":memory:")
        connection.executescript(DOMAIN_SQL.read_text(encoding="utf-8"))
        actual_tables = {
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_schema "
                "WHERE type='table' AND name NOT LIKE 'sqlite_%'"
            )
        }
        datasets = {dataset["id"]: dataset for dataset in contract["datasets"]}
        declarations = contract["tables"]
        declared_names = [table["table_name"] for table in declarations]
        self.assertEqual(len(declared_names), len(set(declared_names)))
        self.assertEqual(set(declared_names), actual_tables)

        for table in declarations:
            quoted = table["table_name"].replace('"', '""')
            columns = list(connection.execute(f'PRAGMA table_info("{quoted}")'))
            actual_pk = [
                row[1]
                for row in sorted(columns, key=lambda row: row[5])
                if row[5] > 0
            ]
            self.assertEqual(table["primary_key_json"], actual_pk, table["table_name"])
            column_names = {row[1] for row in columns}
            self.assertLessEqual(set(table["ignored_columns_json"]), column_names)
            self.assertLessEqual(set(table["immutable_columns_json"]), column_names)
            self.assertTrue(set(table["primary_key_json"]).isdisjoint(table["ignored_columns_json"]))

        self.assertEqual(datasets["edit-history"]["sensitivity"], "sensitive")
        self.assertEqual(datasets["history-state"]["role"], "derived")
        projection = json.loads(PROJECTION.read_text(encoding="utf-8"))
        projected = {
            "profile": "org.sqlite-capsule.data-contract/0.3",
            "app_id": "org.sqlite-capsule.diagram-studio",
            "data_schema_id": "org.sqlite-capsule.diagram-studio-data",
            "data_schema_version": 1,
            "datasets": [],
        }
        dependencies = {}
        for dependency in contract["dependencies"]:
            dependencies.setdefault(dependency["dataset_id"], []).append({
                "dataset_id": dependency["depends_on_dataset_id"],
                "reason": dependency["reason"],
            })
        for dataset in contract["datasets"]:
            projected["datasets"].append({
                "id": dataset["id"],
                "role": dataset["role"],
                "description": dataset["description"],
                "sensitivity": dataset["sensitivity"],
                "required": bool(dataset["required"]),
                "fork": dataset["fork_policy"],
                "compare": dataset["compare_policy"],
                "reconcile": dataset["reconcile_policy"],
                "upgrade": dataset["upgrade_policy"],
                "tables": sorted([
                    {
                        "name": table["table_name"],
                        "sequence": table["sequence"],
                        "primary_key": table["primary_key_json"],
                        "ignored_columns": table["ignored_columns_json"],
                        "immutable_columns": table["immutable_columns_json"],
                    }
                    for table in contract["tables"]
                    if table["dataset_id"] == dataset["id"]
                ], key=lambda table: (table["sequence"], table["name"])),
                "dependencies": sorted(
                    dependencies.get(dataset["id"], []),
                    key=lambda dependency: dependency["dataset_id"],
                ),
            })
        projected["datasets"].sort(key=lambda dataset: dataset["id"])
        projection["datasets"].sort(key=lambda dataset: dataset["id"])
        self.assertEqual(projected, projection)

    def test_selective_and_template_review_fixtures_follow_signed_policy(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        fixture = json.loads(COPY_FIXTURES.read_text(encoding="utf-8"))
        self.assertEqual(fixture["profile"], "org.sqlite-capsule.example-copy-fixtures/1")
        policies = {
            dataset["id"]: dataset["fork_policy"] for dataset in contract["datasets"]
        }
        choices = {
            choice["dataset_id"]: choice
            for choice in fixture["selective_fork"]["choices"]
        }
        self.assertEqual(set(choices), set(policies))
        self.assertEqual(choices["diagram-content"]["disposition"], "include")
        for dataset_id in ("edit-history", "history-state"):
            self.assertEqual(policies[dataset_id], "prompt" if dataset_id == "edit-history" else "omit")
            self.assertEqual(choices[dataset_id]["disposition"], "omit")
            self.assertFalse(choices[dataset_id]["sensitive_confirmed"])
        self.assertEqual(
            fixture["selective_fork"]["expected"]["sensitive_source_rows"],
            "absent-after-vacuum",
        )
        self.assertEqual(
            fixture["template"]["authority"],
            "signed-and-reproduced-template-state/1",
        )
        self.assertEqual(fixture["template"]["choices"], [])

    def test_reconcile_fixture_covers_truth_table_and_expected_target_copy(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        fixture = json.loads(RECONCILE_FIXTURES.read_text(encoding="utf-8"))
        policies = {dataset["id"]: dataset for dataset in contract["datasets"]}
        self.assertEqual(
            fixture["profile"],
            "org.sqlite-capsule.diagram-studio-reconcile-fixtures/1",
        )
        self.assertEqual(fixture["app_id"], "org.sqlite-capsule.diagram-studio")
        dataset = policies[fixture["dataset_id"]]
        self.assertEqual(dataset["reconcile_policy"], "three-way")
        self.assertIn(dataset["compare_policy"], {"row", "field"})

        table = next(
            table
            for table in contract["tables"]
            if table["table_name"] == fixture["table"]
        )
        self.assertEqual(table["primary_key_json"], ["id"])
        connection = sqlite3.connect(":memory:")
        connection.executescript(DOMAIN_SQL.read_text(encoding="utf-8"))
        columns = {
            row[1]
            for row in connection.execute(f'PRAGMA table_info("{fixture["table"]}")')
        }
        for change in fixture["clean_changes"]:
            self.assertEqual(change["expected_state"], change["source_state"])
            self.assertLessEqual(set(change["expected_state"]), columns)

        conflicts = fixture["conflicts"]
        self.assertEqual(
            {conflict["kind"] for conflict in conflicts},
            {"insert-insert", "update-update", "delete-update", "immutable-field"},
        )
        for conflict in conflicts:
            self.assertIn(conflict["resolution"], conflict["allowed_choices"])
            selected = (
                conflict["target_state"]
                if conflict["resolution"] == "keep-target"
                else conflict["source_state"]
            )
            self.assertEqual(conflict["expected_state"], selected)
            if conflict["kind"] == "immutable-field":
                self.assertEqual(conflict["allowed_choices"], ["keep-target"])
                self.assertLessEqual(set(conflict["expected_state"]), set(table["immutable_columns_json"]))
        self.assertEqual(
            set(fixture["two_way_actions"]),
            {
                "insert-from-source",
                "delete-from-target",
                "replace-row-from-source",
                "set-fields",
            },
        )
        self.assertEqual(fixture["expected_output"]["derives_from"], "target")
        self.assertEqual(
            fixture["expected_output"]["lineage_parents"],
            ["target-derived-from", "changes-applied-from"],
        )


if __name__ == "__main__":
    unittest.main()

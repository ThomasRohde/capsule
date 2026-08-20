from __future__ import annotations

import copy
import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER_PATH = ROOT / "tools" / "check_reconcile_plan_vectors.py"
VECTOR_DIR = ROOT / "compatibility" / "reconcile-plan-v1"
SPEC = importlib.util.spec_from_file_location("check_reconcile_plan_vectors", CHECKER_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class ReconcilePlanVectorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.payload = json.loads((VECTOR_DIR / "vector-payload.json").read_text(encoding="utf-8"))
        self.plan = json.loads((VECTOR_DIR / "vector-plan.json").read_text(encoding="utf-8"))

    def assert_payload_rejected(self, mutate) -> None:  # type: ignore[no-untyped-def]
        payload = copy.deepcopy(self.payload)
        mutate(payload)
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.validate_payload(payload)

    def assert_plan_rejected(self, mutate) -> None:  # type: ignore[no-untyped-def]
        plan = copy.deepcopy(self.plan)
        mutate(plan)
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.validate_plan(plan, self.payload)

    def test_independent_checker_accepts_frozen_pair(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(CHECKER_PATH)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)
        self.assertIn("4 operations and 2 resolved conflicts", completed.stdout)
        self.assertIn("reconcile plan vectors: PASS", completed.stdout)

    def test_json_schema_accepts_payload_and_rejects_raw_or_unresolved_shape(self) -> None:
        try:
            import jsonschema  # type: ignore
        except ImportError:
            self.skipTest("jsonschema is not installed")
        schema = json.loads(
            (ROOT / "docs" / "plans" / "capsule-lifecycle" / "contracts" / "reconcile-plan-v1.schema.json").read_text(encoding="utf-8")
        )
        jsonschema.Draft202012Validator.check_schema(schema)
        validator = jsonschema.Draft202012Validator(schema)
        validator.validate(self.payload)
        validator.validate(
            json.loads(
                (
                    ROOT
                    / "docs"
                    / "plans"
                    / "capsule-lifecycle"
                    / "examples"
                    / "reconcile-payload-two-way.json"
                ).read_text(encoding="utf-8")
            )
        )

        lifecycle_schema = json.loads(
            (
                ROOT
                / "docs"
                / "plans"
                / "capsule-lifecycle"
                / "contracts"
                / "lifecycle-plan-v1.schema.json"
            ).read_text(encoding="utf-8")
        )
        jsonschema.Draft202012Validator.check_schema(lifecycle_schema)
        jsonschema.Draft202012Validator(lifecycle_schema).validate(
            json.loads(
                (
                    ROOT
                    / "docs"
                    / "plans"
                    / "capsule-lifecycle"
                    / "examples"
                    / "reconcile-lifecycle-plan.json"
                ).read_text(encoding="utf-8")
            )
        )

        raw_key = copy.deepcopy(self.payload)
        raw_key["operations"][0]["key"] = {"id": "secret"}
        self.assertFalse(validator.is_valid(raw_key))

        unresolved = copy.deepcopy(self.payload)
        unresolved["resolved_conflicts"][0]["resolution"] = "unresolved"
        self.assertFalse(validator.is_valid(unresolved))

    def test_payload_contains_no_raw_key_value_path_or_sql_members(self) -> None:
        forbidden = {"key", "value", "path", "sql", "source_value", "target_value"}

        def visit(value: object) -> None:
            if isinstance(value, dict):
                self.assertTrue(forbidden.isdisjoint(value), f"unsafe member in {value.keys()}")
                for child in value.values():
                    visit(child)
            elif isinstance(value, list):
                for child in value:
                    visit(child)

        visit(self.payload)

    def test_tagged_row_states_distinguish_absence_from_sql_null_digest(self) -> None:
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0]["target_state"].update(
                {"row_digest": "aa" * 32}
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0]["source_state"].pop("row_digest")
        )
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0]["source_state"].update(
                {"row_digest": None}
            )
        )

    def test_sequence_fields_and_conflict_ids_are_deterministic(self) -> None:
        self.assert_payload_rejected(lambda payload: payload["operations"][1].update({"sequence": 4}))
        self.assert_payload_rejected(
            lambda payload: payload["operations"][3]["action"]["fields"].append(
                copy.deepcopy(payload["operations"][3]["action"]["fields"][0])
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["resolved_conflicts"][0].update({"id": "ff" * 32})
        )

    def test_conflict_vocabulary_is_closed_and_all_conflicts_are_resolved(self) -> None:
        self.assert_payload_rejected(
            lambda payload: payload["resolved_conflicts"][0].update({"kind": "policy-forbidden"})
        )
        self.assert_payload_rejected(
            lambda payload: payload["resolved_conflicts"][0].update({"resolution": "unresolved"})
        )
        self.assert_payload_rejected(
            lambda payload: payload["resolved_conflicts"][0].update(
                {"allowed_choices": ["keep-target", "take-source"]}
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["operations"][2].pop("conflict_id")
        )
        self.assert_payload_rejected(
            lambda payload: payload["resolved_conflicts"][1].update(
                {"resolution": "keep-target"}
            )
        )

    def test_sensitive_confirmation_and_exhaustive_dataset_binding_fail_closed(self) -> None:
        self.assert_payload_rejected(
            lambda payload: payload["sensitive_confirmation"].update({"confirmed_dataset_ids": []})
        )
        self.assert_payload_rejected(lambda payload: payload["expected_dataset_states"].pop(0))
        self.assert_payload_rejected(
            lambda payload: payload["expected_dataset_states"].reverse()
        )

    def test_two_way_mode_cannot_smuggle_ancestor_or_automatic_claims(self) -> None:
        def mutate(payload):  # type: ignore[no-untyped-def]
            payload["mode"] = "two-way-explicit"
            payload["resolved_conflicts"] = []
            payload["signature_inventories"].pop("ancestor")
            payload["operations"][0]["basis"] = "three-way-clean"

        self.assert_payload_rejected(mutate)

    def test_lifecycle_envelope_is_the_only_input_and_destination_authority(self) -> None:
        self.assert_plan_rejected(lambda plan: plan["decisions"].append(copy.deepcopy(plan["decisions"][0])))
        self.assert_plan_rejected(lambda plan: plan["inputs"][0].update({"role": "target"}))
        self.assert_plan_rejected(lambda plan: plan["output"].update({"publish_mode": "replace"}))
        self.assert_plan_rejected(lambda plan: plan["expected"].update({"capsule_id": "new-id"}))
        self.assert_plan_rejected(
            lambda plan: plan["decisions"][0]["parameters"].update(
                {"reconcile_payload_digest": "ff" * 32}
            )
        )

    def test_embedded_digests_cover_every_mutation(self) -> None:
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0].update({"dataset_id": "changed"})
        )
        self.assert_plan_rejected(lambda plan: plan["expected"].update({"revision_id": "changed"}))

    def test_strict_json_rejects_duplicate_keys_floats_unknown_members_and_depth(self) -> None:
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.strict_loads(b'{"profile":"a","profile":"b"}')
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.strict_loads(b'{"number":1.5}')
        deep: object = None
        for _ in range(CHECKER.MAX_JSON_DEPTH + 1):
            deep = [deep]
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.strict_loads(json.dumps(deep).encode("utf-8"))
        self.assert_payload_rejected(lambda payload: payload.update({"raw_values": []}))
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0]["action"].update(
                {"sql": "INSERT INTO items VALUES (?)"}
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["operations"][0]["action"].update(
                {"kind": "run-sql"}
            )
        )

    def test_source_byte_and_count_ceilings_are_explicit(self) -> None:
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.strict_loads(b" " * (CHECKER.MAX_PAYLOAD_BYTES + 1))
        payload = copy.deepcopy(self.payload)
        payload["operations"] = payload["operations"] * (CHECKER.MAX_OPERATIONS // 4 + 1)
        with self.assertRaises(CHECKER.VectorError):
            CHECKER.validate_payload(payload, verify_digest=False)

    def test_lifecycle_plan_cannot_embed_the_payload(self) -> None:
        self.assert_plan_rejected(
            lambda plan: plan["decisions"][0]["parameters"].update(
                {"reconcile_payload": copy.deepcopy(self.payload)}
            )
        )

    def test_lineage_is_exactly_two_parents_and_cross_bound_to_plan(self) -> None:
        self.assert_payload_rejected(
            lambda payload: payload["lineage"]["parents"].append(
                copy.deepcopy(payload["lineage"]["parents"][0])
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["lineage"]["parents"].reverse()
        )
        self.assert_payload_rejected(
            lambda payload: payload["lineage"]["details"].update(
                {"operation_count": 3}
            )
        )
        self.assert_payload_rejected(
            lambda payload: payload["lineage"]["details"].update(
                {"payload_digest": "ff" * 32}
            )
        )

        for mutate in (
            lambda payload: payload["lineage"].update({"occurred_at": "2026-08-13T08:00:01Z"}),
            lambda payload: payload["lineage"]["result"].update({"revision_id": "different"}),
            lambda payload: payload["lineage"]["parents"][0].update({"file_sha256": "ff" * 32}),
            lambda payload: payload["lineage"]["details"]["ancestor_evidence"].update(
                {"revision_id": "different"}
            ),
        ):
            payload = copy.deepcopy(self.payload)
            mutate(payload)
            payload = CHECKER.seal_payload(payload)
            plan = CHECKER.seal_plan(self.plan, payload["payload_digest"])
            CHECKER.validate_payload(payload)
            with self.assertRaises(CHECKER.VectorError):
                CHECKER.validate_plan(plan, payload)


if __name__ == "__main__":
    unittest.main()

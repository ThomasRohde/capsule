#!/usr/bin/env python3
"""Verify reconcile-payload/1 and its lifecycle-plan/1 binding with stdlib only."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any, NoReturn


ROOT = Path(__file__).resolve().parents[1]
VECTOR_DIR = ROOT / "compatibility" / "reconcile-plan-v1"
MAX_PAYLOAD_BYTES = 16 * 1024 * 1024
MAX_PLAN_BYTES = 1024 * 1024
MAX_JSON_DEPTH = 32
MAX_OPERATIONS = 10_000
MAX_CONFLICTS = 10_000
MAX_DATASETS = 256
class VectorError(ValueError):
    pass


def _object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise VectorError(f"duplicate JSON member: {key}")
        value[key] = item
    return value


def _integer(text: str) -> int:
    value = int(text)
    if not -(2**63) <= value <= 2**64 - 1:
        raise VectorError("integer is outside the lifecycle JSON range")
    return value


def strict_loads(data: bytes) -> Any:
    if not data or len(data) > MAX_PAYLOAD_BYTES:
        raise VectorError("JSON exceeds the 16 MiB reconciliation byte ceiling")
    try:
        value = json.loads(
            data.decode("utf-8"),
            object_pairs_hook=_object,
            parse_int=_integer,
            parse_float=lambda _text: _fail("floating-point JSON is forbidden"),
            parse_constant=lambda _text: _fail("non-finite JSON is forbidden"),
        )
        _validate_depth(value)
        return value
    except UnicodeDecodeError as error:
        raise VectorError("JSON is not UTF-8") from error


def _fail(message: str) -> NoReturn:
    raise VectorError(message)


def _validate_depth(value: Any, depth: int = 1) -> None:
    if depth > MAX_JSON_DEPTH:
        raise VectorError("JSON exceeds the reconciliation nesting-depth ceiling")
    if isinstance(value, dict):
        for child in value.values():
            _validate_depth(child, depth + 1)
    elif isinstance(value, list):
        for child in value:
            _validate_depth(child, depth + 1)


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _uuid4(value: Any, label: str) -> str:
    if not isinstance(value, str) or re.fullmatch(
        r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}",
        value,
    ) is None:
        raise VectorError(f"{label} is not a lowercase UUIDv4")
    return value


def _is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def _object_keys(value: Any, required: set[str], optional: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise VectorError(f"{label} must be an object")
    missing = required - value.keys()
    extra = value.keys() - required - optional
    if missing or extra:
        raise VectorError(f"{label} members: missing={sorted(missing)} extra={sorted(extra)}")
    return value


def _bounded_text(value: Any, label: str, maximum: int = 512) -> str:
    if not isinstance(value, str) or not value or len(value) > maximum or len(value.encode("utf-8")) > maximum * 4:
        raise VectorError(f"{label} is not bounded text")
    return value


def _digest(value: Any, label: str) -> str:
    if not _is_sha256(value):
        raise VectorError(f"{label} is not lowercase SHA-256")
    return value


def _row_state(value: Any, label: str) -> dict[str, Any]:
    state = _object_keys(value, {"state"}, {"row_digest"}, label)
    if state["state"] == "absent":
        if "row_digest" in state:
            raise VectorError(f"{label}: absent state cannot carry row_digest")
    elif state["state"] == "present":
        if "row_digest" not in state:
            raise VectorError(f"{label}: present state requires row_digest")
        _digest(state["row_digest"], f"{label}.row_digest")
    else:
        raise VectorError(f"{label}: invalid state tag")
    return state


def _inventory(value: Any, label: str) -> None:
    item = _object_keys(value, {"count", "sha256"}, set(), label)
    if not isinstance(item["count"], int) or isinstance(item["count"], bool) or not 1 <= item["count"] <= 4096:
        raise VectorError(f"{label}.count is invalid")
    _digest(item["sha256"], f"{label}.sha256")


def _conflict_material(conflict: dict[str, Any]) -> dict[str, Any]:
    material = {
        "ancestor_state": conflict["ancestor_state"],
        "dataset_id": conflict["dataset_id"],
        "key_digest": conflict["key_digest"],
        "kind": conflict["kind"],
        "profile": "org.sqlite-capsule.reconcile-conflict-id/1",
        "source_state": conflict["source_state"],
        "table": conflict["table"],
        "target_state": conflict["target_state"],
    }
    if "deleted_side" in conflict:
        material["deleted_side"] = conflict["deleted_side"]
    return material


def conflict_id(conflict: dict[str, Any]) -> str:
    return sha256(canonical_json(_conflict_material(conflict)))


def ancestor_evidence_digest(evidence: dict[str, Any]) -> str:
    material = dict(evidence)
    material.pop("evidence_digest", None)
    return sha256(canonical_json(material))


def payload_digest(payload: dict[str, Any]) -> str:
    """Digest payload content while omitting both embedded aliases of that digest."""
    material = copy_json(payload)
    material.pop("payload_digest", None)
    lineage = material.get("lineage")
    if isinstance(lineage, dict):
        details = lineage.get("details")
        if isinstance(details, dict):
            details.pop("payload_digest", None)
    return sha256(canonical_json(material))


def copy_json(value: Any) -> Any:
    return json.loads(json.dumps(value, ensure_ascii=False, allow_nan=False))


def _validate_action(operation: dict[str, Any], label: str) -> str:
    action = _object_keys(
        operation["action"],
        {"kind"},
        {"source_write_set_digest", "fields"},
        f"{label}.action",
    )
    kind = action["kind"]
    source_state = operation["source_state"]["state"]
    target_state = operation["target_state"]["state"]
    if kind == "insert-source-row":
        if set(action) != {"kind", "source_write_set_digest"} or source_state != "present" or target_state != "absent":
            raise VectorError(f"{label}: invalid insert-source-row states or members")
        _digest(action["source_write_set_digest"], f"{label}.action.source_write_set_digest")
    elif kind == "delete-target-row":
        if set(action) != {"kind"} or source_state != "absent" or target_state != "present":
            raise VectorError(f"{label}: invalid delete-target-row states or members")
    elif kind == "replace-target-row-from-source":
        if set(action) != {"kind", "source_write_set_digest"} or source_state != "present" or target_state != "present":
            raise VectorError(f"{label}: invalid replace-target-row-from-source states or members")
        _digest(action["source_write_set_digest"], f"{label}.action.source_write_set_digest")
    elif kind == "set-target-fields-from-source":
        if set(action) != {"kind", "fields"} or source_state != "present" or target_state != "present":
            raise VectorError(f"{label}: invalid set-target-fields-from-source states or members")
        fields = action["fields"]
        if not isinstance(fields, list) or not 1 <= len(fields) <= 256:
            raise VectorError(f"{label}: fields must be a non-empty bounded array")
        columns: list[str] = []
        for index, raw_field in enumerate(fields):
            field = _object_keys(
                raw_field,
                {"column", "source_value_digest", "target_value_digest"},
                set(),
                f"{label}.fields[{index}]",
            )
            columns.append(_bounded_text(field["column"], f"{label}.fields[{index}].column"))
            _digest(field["source_value_digest"], f"{label}.fields[{index}].source_value_digest")
            _digest(field["target_value_digest"], f"{label}.fields[{index}].target_value_digest")
        if len(columns) != len(set(columns)):
            raise VectorError(f"{label}: duplicate selected field")
    else:
        raise VectorError(f"{label}: unsupported action {kind!r}")
    return kind


def _validate_dataset_state(value: Any, label: str) -> None:
    state = _object_keys(value, {"profile", "row_count", "sha256"}, set(), label)
    if state["profile"] != "org.sqlite-capsule.dataset-state/1":
        raise VectorError(f"{label}: invalid dataset-state profile")
    if not isinstance(state["row_count"], int) or isinstance(state["row_count"], bool) or not 0 <= state["row_count"] <= 2**63 - 1:
        raise VectorError(f"{label}.row_count is invalid")
    _digest(state["sha256"], f"{label}.sha256")


def seal_payload(payload: dict[str, Any]) -> dict[str, Any]:
    sealed = copy_json(payload)
    for conflict in sealed.get("resolved_conflicts", []):
        conflict["id"] = conflict_id(conflict)
    lineage = sealed.get("lineage")
    if isinstance(lineage, dict) and isinstance(lineage.get("details"), dict):
        details = lineage["details"]
        details["compare_report_digest"] = sealed["compare_report_digest"]
        details["operation_count"] = len(sealed.get("operations", []))
        details["resolved_conflict_count"] = len(sealed.get("resolved_conflicts", []))
        ancestor = details.get("ancestor_evidence")
        if isinstance(ancestor, dict):
            ancestor["evidence_digest"] = ancestor_evidence_digest(ancestor)
    digest = payload_digest(sealed)
    sealed["payload_digest"] = digest
    if isinstance(lineage, dict) and isinstance(lineage.get("details"), dict):
        lineage["details"]["payload_digest"] = digest
    return sealed


def validate_payload(payload: Any, *, verify_digest: bool = True) -> dict[str, Any]:
    if len(canonical_json(payload)) > MAX_PAYLOAD_BYTES:
        raise VectorError("canonical payload exceeds the 16 MiB byte ceiling")
    root = _object_keys(
        payload,
        {
            "profile", "compare_report_digest", "source_side", "target_side", "mode",
            "signature_inventories", "lineage", "operations", "resolved_conflicts",
            "expected_dataset_states", "sensitive_confirmation", "payload_digest",
        },
        set(),
        "payload",
    )
    if root["profile"] != "org.sqlite-capsule.reconcile-payload/1":
        raise VectorError("invalid reconcile payload profile")
    _digest(root["compare_report_digest"], "compare_report_digest")
    _digest(root["payload_digest"], "payload_digest")
    if root["source_side"] != "source" or root["target_side"] != "target":
        raise VectorError("source_side and target_side must be explicit and fixed")
    mode = root["mode"]
    if mode not in {"two-way-explicit", "three-way"}:
        raise VectorError("invalid reconciliation mode")

    inventories = _object_keys(root["signature_inventories"], {"source", "target"}, {"ancestor"}, "signature_inventories")
    for role, inventory in inventories.items():
        _inventory(inventory, f"signature_inventories.{role}")
    if (mode == "three-way") != ("ancestor" in inventories):
        raise VectorError("ancestor signature inventory must match reconciliation mode")

    lineage = _object_keys(
        root["lineage"],
        {"event_id", "occurred_at", "operation", "result", "parents", "details"},
        set(),
        "lineage",
    )
    _uuid4(lineage["event_id"], "lineage.event_id")
    if not isinstance(lineage["occurred_at"], str) or re.fullmatch(
        r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",
        lineage["occurred_at"],
    ) is None:
        raise VectorError("lineage.occurred_at is not exact UTC seconds")
    if lineage["operation"] != "reconcile-to-copy":
        raise VectorError("lineage operation is not reconcile-to-copy")
    result = _object_keys(lineage["result"], {"capsule_id", "revision_id"}, set(), "lineage.result")
    _bounded_text(result["capsule_id"], "lineage.result.capsule_id")
    _bounded_text(result["revision_id"], "lineage.result.revision_id")
    parents = lineage["parents"]
    if not isinstance(parents, list) or len(parents) != 2:
        raise VectorError("lineage must have exactly two parents")
    parent_specs = ((1, "target-derived-from"), (2, "changes-applied-from"))
    for index, (raw_parent, (ordinal, relation)) in enumerate(zip(parents, parent_specs, strict=True)):
        parent = _object_keys(
            raw_parent,
            {"ordinal", "relation", "file_sha256", "capsule_id", "revision_id"},
            set(),
            f"lineage.parents[{index}]",
        )
        if parent["ordinal"] != ordinal or parent["relation"] != relation:
            raise VectorError("lineage parents have invalid order or relation")
        _digest(parent["file_sha256"], f"lineage.parents[{index}].file_sha256")
        _bounded_text(parent["capsule_id"], f"lineage.parents[{index}].capsule_id")
        _bounded_text(parent["revision_id"], f"lineage.parents[{index}].revision_id")
    details = _object_keys(
        lineage["details"],
        {"profile", "compare_report_digest", "payload_digest", "operation_count", "resolved_conflict_count"},
        {"ancestor_evidence"},
        "lineage.details",
    )
    if details["profile"] != "org.sqlite-capsule.reconcile-lineage-details/1":
        raise VectorError("invalid reconcile lineage details profile")
    if details["compare_report_digest"] != root["compare_report_digest"]:
        raise VectorError("lineage details do not bind the compare report")
    if details["payload_digest"] != root["payload_digest"]:
        raise VectorError("lineage details payload digest alias mismatch")
    for member in ("operation_count", "resolved_conflict_count"):
        if not isinstance(details[member], int) or isinstance(details[member], bool) or not 0 <= details[member] <= MAX_OPERATIONS:
            raise VectorError(f"lineage.details.{member} is invalid")
    has_ancestor_evidence = "ancestor_evidence" in details
    if has_ancestor_evidence != (mode == "three-way"):
        raise VectorError("ancestor lineage evidence must match reconciliation mode")
    if has_ancestor_evidence:
        evidence = _object_keys(
            details["ancestor_evidence"],
            {"profile", "file_sha256", "capsule_id", "revision_id", "evidence_digest"},
            set(),
            "lineage.details.ancestor_evidence",
        )
        if evidence["profile"] != "org.sqlite-capsule.reconcile-ancestor-evidence/1":
            raise VectorError("invalid ancestor evidence profile")
        _digest(evidence["file_sha256"], "ancestor evidence file_sha256")
        _bounded_text(evidence["capsule_id"], "ancestor evidence capsule_id")
        _bounded_text(evidence["revision_id"], "ancestor evidence revision_id")
        if evidence["evidence_digest"] != ancestor_evidence_digest(evidence):
            raise VectorError("ancestor evidence digest mismatch")

    operations = root["operations"]
    if not isinstance(operations, list) or len(operations) > MAX_OPERATIONS:
        raise VectorError("operations must be a bounded array")
    operation_conflicts: dict[str, int] = {}
    touched_datasets: set[str] = set()
    for index, raw_operation in enumerate(operations):
        label = f"operations[{index}]"
        operation = _object_keys(
            raw_operation,
            {"sequence", "dataset_id", "table", "key_digest", "basis", "source_state", "target_state", "action"},
            {"ancestor_state", "conflict_id"},
            label,
        )
        if operation["sequence"] != index + 1:
            raise VectorError("operation sequence must be deterministic and contiguous from 1")
        dataset_id = _bounded_text(operation["dataset_id"], f"{label}.dataset_id")
        touched_datasets.add(dataset_id)
        _bounded_text(operation["table"], f"{label}.table")
        _digest(operation["key_digest"], f"{label}.key_digest")
        _row_state(operation["source_state"], f"{label}.source_state")
        _row_state(operation["target_state"], f"{label}.target_state")
        if mode == "three-way":
            if "ancestor_state" not in operation:
                raise VectorError(f"{label}: three-way operation requires ancestor_state")
            _row_state(operation["ancestor_state"], f"{label}.ancestor_state")
        elif "ancestor_state" in operation:
            raise VectorError(f"{label}: two-way operation cannot carry ancestor_state")
        basis = operation["basis"]
        if basis not in {"user-selected", "three-way-clean", "conflict-resolution"}:
            raise VectorError(f"{label}: invalid basis")
        if mode == "two-way-explicit" and basis != "user-selected":
            raise VectorError(f"{label}: two-way operations are explicit user selections")
        has_conflict = "conflict_id" in operation
        if has_conflict != (basis == "conflict-resolution"):
            raise VectorError(f"{label}: conflict_id is exclusive to conflict-resolution")
        if has_conflict:
            conflict = _digest(operation["conflict_id"], f"{label}.conflict_id")
            operation_conflicts[conflict] = operation_conflicts.get(conflict, 0) + 1
        _validate_action(operation, label)

    conflicts = root["resolved_conflicts"]
    if not isinstance(conflicts, list) or len(conflicts) > MAX_CONFLICTS:
        raise VectorError("resolved_conflicts must be a bounded array")
    if mode == "two-way-explicit" and conflicts:
        raise VectorError("two-way payload cannot claim three-way conflict evidence")
    prior_id = ""
    known_conflicts: set[str] = set()
    for index, raw_conflict in enumerate(conflicts):
        label = f"resolved_conflicts[{index}]"
        conflict = _object_keys(
            raw_conflict,
            {"id", "dataset_id", "table", "key_digest", "kind", "source_state", "target_state", "ancestor_state", "allowed_choices", "resolution"},
            {"deleted_side"},
            label,
        )
        identifier = _digest(conflict["id"], f"{label}.id")
        if identifier != conflict_id(conflict):
            raise VectorError(f"{label}: conflict id does not match canonical conflict evidence")
        if identifier <= prior_id or identifier in known_conflicts:
            raise VectorError("resolved conflicts must be uniquely sorted by deterministic id")
        prior_id = identifier
        known_conflicts.add(identifier)
        _bounded_text(conflict["dataset_id"], f"{label}.dataset_id")
        _bounded_text(conflict["table"], f"{label}.table")
        _digest(conflict["key_digest"], f"{label}.key_digest")
        for side in ("source_state", "target_state", "ancestor_state"):
            _row_state(conflict[side], f"{label}.{side}")
        kind = conflict["kind"]
        if kind not in {"insert-insert", "update-update", "delete-update", "immutable-field"}:
            raise VectorError(f"{label}: policy-forbidden and unknown conflicts are admission errors")
        if (kind == "delete-update") != ("deleted_side" in conflict):
            raise VectorError(f"{label}: deleted_side is exclusive and required for delete-update")
        if "deleted_side" in conflict and conflict["deleted_side"] not in {"source", "target"}:
            raise VectorError(f"{label}: invalid deleted_side")
        choices = conflict["allowed_choices"]
        expected_choices = ["keep-target"] if kind == "immutable-field" else ["keep-target", "take-source"]
        if choices != expected_choices or conflict["resolution"] not in choices:
            raise VectorError(f"{label}: invalid closed conflict choices/resolution")
        references = operation_conflicts.get(identifier, 0)
        if conflict["resolution"] == "take-source" and references != 1:
            raise VectorError(f"{label}: take-source requires exactly one bound operation")
        if conflict["resolution"] == "keep-target" and references != 0:
            raise VectorError(f"{label}: keep-target must not produce an operation")
    if set(operation_conflicts) != {item["id"] for item in conflicts if item["resolution"] == "take-source"}:
        raise VectorError("operation references unknown or non-take-source conflict")
    if details["operation_count"] != len(operations) or details["resolved_conflict_count"] != len(conflicts):
        raise VectorError("lineage detail counts do not match payload arrays")

    expected_states = root["expected_dataset_states"]
    if not isinstance(expected_states, list) or not 1 <= len(expected_states) <= MAX_DATASETS:
        raise VectorError("expected_dataset_states must be a non-empty bounded array")
    dataset_ids: list[str] = []
    for index, raw_state in enumerate(expected_states):
        label = f"expected_dataset_states[{index}]"
        state = _object_keys(raw_state, {"dataset_id", "source", "target", "output"}, set(), label)
        dataset_ids.append(_bounded_text(state["dataset_id"], f"{label}.dataset_id"))
        for side in ("source", "target", "output"):
            _validate_dataset_state(state[side], f"{label}.{side}")
    if dataset_ids != sorted(set(dataset_ids), key=lambda value: value.encode("utf-8")):
        raise VectorError("expected dataset states must be unique and BINARY sorted")
    if not touched_datasets.issubset(dataset_ids):
        raise VectorError("every operation dataset requires an expected dataset state")

    confirmation = _object_keys(root["sensitive_confirmation"], {"required_dataset_ids", "confirmed_dataset_ids"}, set(), "sensitive_confirmation")
    required = confirmation["required_dataset_ids"]
    confirmed = confirmation["confirmed_dataset_ids"]
    for label, items in (("required_dataset_ids", required), ("confirmed_dataset_ids", confirmed)):
        if not isinstance(items, list) or len(items) > MAX_DATASETS or items != sorted(set(items), key=lambda value: value.encode("utf-8")):
            raise VectorError(f"{label} must be unique and BINARY sorted")
        for value in items:
            _bounded_text(value, label)
    if required != confirmed:
        raise VectorError("every required sensitive dataset must be explicitly confirmed")
    if not set(required).issubset(dataset_ids):
        raise VectorError("sensitive confirmation names an unknown dataset")

    if verify_digest:
        embedded = root["payload_digest"]
        if payload_digest(root) != embedded:
            raise VectorError("payload digest mismatch")
    return root


def seal_plan(plan: dict[str, Any], payload_digest: str) -> dict[str, Any]:
    sealed = json.loads(json.dumps(plan))
    decisions = sealed.get("decisions")
    if isinstance(decisions, list) and len(decisions) == 1:
        parameters = decisions[0].setdefault("parameters", {})
        parameters["reconcile_payload_digest"] = payload_digest
    unsigned = dict(sealed)
    unsigned.pop("plan_digest", None)
    sealed["plan_digest"] = sha256(canonical_json(unsigned))
    return sealed


def validate_plan(plan: Any, payload: dict[str, Any], *, verify_digest: bool = True) -> dict[str, Any]:
    root = _object_keys(
        plan,
        {"profile", "plan_id", "operation", "created_at", "expires_at", "inputs", "output", "decisions", "limits", "expected", "plan_digest"},
        set(),
        "lifecycle plan",
    )
    if root["profile"] != "org.sqlite-capsule.lifecycle-plan/1" or root["operation"] != "reconcile-to-copy":
        raise VectorError("reconciliation authority must be a lifecycle-plan/1 reconcile-to-copy")
    _digest(root["plan_digest"], "plan_digest")
    inputs = root["inputs"]
    expected_roles = ["source", "target"] + (["ancestor"] if payload["mode"] == "three-way" else [])
    if not isinstance(inputs, list) or [item.get("role") if isinstance(item, dict) else None for item in inputs] != expected_roles:
        raise VectorError("lifecycle input roles/order must be exactly source, target, optional ancestor")
    capsules: dict[str, dict[str, Any]] = {}
    for role, raw_input in zip(expected_roles, inputs, strict=True):
        item = _object_keys(raw_input, {"role", "path_hint", "file_sha256", "snapshot_sha256", "size_bytes", "filesystem_identity", "capsule"}, set(), f"input {role}")
        _digest(item["file_sha256"], f"input {role}.file_sha256")
        _digest(item["snapshot_sha256"], f"input {role}.snapshot_sha256")
        if not isinstance(item["size_bytes"], int) or isinstance(item["size_bytes"], bool) or item["size_bytes"] < 0:
            raise VectorError(f"input {role}.size_bytes is invalid")
        identity = _object_keys(item["filesystem_identity"], {"platform", "volume_or_device", "file_id_or_inode", "modified_ns"}, set(), f"input {role}.filesystem_identity")
        if not isinstance(identity["modified_ns"], int) or isinstance(identity["modified_ns"], bool) or identity["modified_ns"] < 0:
            raise VectorError(f"input {role}.modified_ns is invalid")
        capsule = _object_keys(item["capsule"], {"format_version", "capsule_id", "revision_id", "app_id", "app_version", "application_digest", "data_schema_id", "data_schema_version"}, {"publisher_key_id"}, f"input {role}.capsule")
        for member in ("capsule_id", "revision_id", "app_id", "app_version", "data_schema_id"):
            _bounded_text(capsule[member], f"input {role}.capsule.{member}")
        _digest(capsule["application_digest"], f"input {role}.application_digest")
        if not isinstance(capsule["data_schema_version"], int) or isinstance(capsule["data_schema_version"], bool) or capsule["data_schema_version"] < 1:
            raise VectorError(f"input {role}.data_schema_version is invalid")
        capsules[role] = capsule
    target = capsules["target"]
    for role, capsule in capsules.items():
        if capsule["app_id"] != target["app_id"] or capsule["data_schema_id"] != target["data_schema_id"] or capsule["data_schema_version"] != target["data_schema_version"]:
            raise VectorError(f"input {role} is not in the exact compatible app/schema family")

    decisions = root["decisions"]
    if not isinstance(decisions, list) or len(decisions) != 1:
        raise VectorError("reconcile lifecycle plan must have exactly one payload-binding decision")
    decision = _object_keys(decisions[0], {"scope", "subject", "action", "reason", "parameters"}, set(), "reconcile decision")
    if decision["scope"] != "application" or decision["subject"] != target["app_id"] or decision["action"] != "bind-reconcile-payload":
        raise VectorError("invalid reconcile payload-binding decision")
    parameters = _object_keys(decision["parameters"], {"reconcile_payload_digest"}, set(), "reconcile decision parameters")
    if parameters["reconcile_payload_digest"] != payload["payload_digest"]:
        raise VectorError("lifecycle decision does not bind the exact reconcile payload")

    output = _object_keys(root["output"], {"path", "leaf_name", "parent_filesystem_identity", "must_not_exist", "publish_mode"}, set(), "output")
    if output["must_not_exist"] is not True or output["publish_mode"] != "create-new-no-replace":
        raise VectorError("reconcile output must be create-new with no replacement")
    expected = _object_keys(root["expected"], {"capsule_id", "revision_id", "app_id", "application_digest", "data_schema_id", "data_schema_version"}, set(), "expected")
    if expected["capsule_id"] != target["capsule_id"] or expected["revision_id"] == target["revision_id"]:
        raise VectorError("output must preserve target capsule id and mint a new revision")
    for member in ("app_id", "application_digest", "data_schema_id", "data_schema_version"):
        if expected[member] != target[member]:
            raise VectorError(f"expected output must preserve target {member}")
    lineage = payload["lineage"]
    if lineage["occurred_at"] != root["created_at"]:
        raise VectorError("lineage occurred_at must equal lifecycle plan created_at")
    if lineage["result"] != {"capsule_id": expected["capsule_id"], "revision_id": expected["revision_id"]}:
        raise VectorError("lineage result must equal lifecycle expected identity")
    for index, role in enumerate(("target", "source")):
        raw_input = inputs[expected_roles.index(role)]
        expected_parent = {
            "ordinal": index + 1,
            "relation": "target-derived-from" if role == "target" else "changes-applied-from",
            "file_sha256": raw_input["file_sha256"],
            "capsule_id": raw_input["capsule"]["capsule_id"],
            "revision_id": raw_input["capsule"]["revision_id"],
        }
        if lineage["parents"][index] != expected_parent:
            raise VectorError(f"lineage parent {index + 1} does not bind the exact {role} input")
    if payload["mode"] == "three-way":
        raw_ancestor = inputs[expected_roles.index("ancestor")]
        ancestor = lineage["details"]["ancestor_evidence"]
        for member in ("file_sha256", "capsule_id", "revision_id"):
            source_value = raw_ancestor["file_sha256"] if member == "file_sha256" else raw_ancestor["capsule"][member]
            if ancestor[member] != source_value:
                raise VectorError(f"ancestor lineage evidence does not bind {member}")
    if verify_digest:
        unsigned = dict(root)
        embedded = unsigned.pop("plan_digest")
        if sha256(canonical_json(unsigned)) != embedded:
            raise VectorError("lifecycle plan digest mismatch")
    return root


def _load_pair(vector_dir: Path) -> tuple[bytes, dict[str, Any], bytes, dict[str, Any], dict[str, Any]]:
    payload_path = vector_dir / "vector-payload.json"
    plan_path = vector_dir / "vector-plan.json"
    if not 1 <= payload_path.stat().st_size <= MAX_PAYLOAD_BYTES:
        raise VectorError("vector payload exceeds the 16 MiB byte ceiling")
    if not 1 <= plan_path.stat().st_size <= MAX_PLAN_BYTES:
        raise VectorError("vector lifecycle plan exceeds its 1 MiB byte ceiling")
    raw_payload = payload_path.read_bytes()
    raw_plan = plan_path.read_bytes()
    if not raw_payload or len(raw_payload) > MAX_PAYLOAD_BYTES:
        raise VectorError("vector payload exceeds the 16 MiB byte ceiling")
    if not raw_plan or len(raw_plan) > MAX_PLAN_BYTES:
        raise VectorError("vector lifecycle plan exceeds its 1 MiB byte ceiling")
    payload = strict_loads(raw_payload)
    plan = strict_loads(raw_plan)
    vectors = strict_loads((vector_dir / "vectors.json").read_bytes())
    if not isinstance(payload, dict) or not isinstance(plan, dict) or not isinstance(vectors, dict):
        raise VectorError("vector roots must be objects")
    return raw_payload, payload, raw_plan, plan, vectors


def rewrite(vector_dir: Path = VECTOR_DIR) -> None:
    _raw_payload, payload, _raw_plan, plan, vectors = _load_pair(vector_dir)
    if "lineage" not in payload:
        by_role = {item["role"]: item for item in plan["inputs"]}
        payload["lineage"] = {
            "event_id": "31fe41d2-35de-4ece-9200-c80061582c42",
            "occurred_at": plan["created_at"],
            "operation": "reconcile-to-copy",
            "result": {
                "capsule_id": plan["expected"]["capsule_id"],
                "revision_id": plan["expected"]["revision_id"],
            },
            "parents": [
                {
                    "ordinal": 1,
                    "relation": "target-derived-from",
                    "file_sha256": by_role["target"]["file_sha256"],
                    "capsule_id": by_role["target"]["capsule"]["capsule_id"],
                    "revision_id": by_role["target"]["capsule"]["revision_id"],
                },
                {
                    "ordinal": 2,
                    "relation": "changes-applied-from",
                    "file_sha256": by_role["source"]["file_sha256"],
                    "capsule_id": by_role["source"]["capsule"]["capsule_id"],
                    "revision_id": by_role["source"]["capsule"]["revision_id"],
                },
            ],
            "details": {
                "profile": "org.sqlite-capsule.reconcile-lineage-details/1",
                "compare_report_digest": payload["compare_report_digest"],
                "payload_digest": "00" * 32,
                "operation_count": len(payload["operations"]),
                "resolved_conflict_count": len(payload["resolved_conflicts"]),
                "ancestor_evidence": {
                    "profile": "org.sqlite-capsule.reconcile-ancestor-evidence/1",
                    "file_sha256": by_role["ancestor"]["file_sha256"],
                    "capsule_id": by_role["ancestor"]["capsule"]["capsule_id"],
                    "revision_id": by_role["ancestor"]["capsule"]["revision_id"],
                    "evidence_digest": "00" * 32,
                },
            },
        }
    payload = seal_payload(payload)
    conflicts = payload["resolved_conflicts"]
    conflicts.sort(key=lambda item: item["id"])
    # Operations bind ids by a stable fixture-only alias before the first rewrite.
    take_source = [item for item in conflicts if item["resolution"] == "take-source"]
    for operation in payload["operations"]:
        if operation.get("basis") == "conflict-resolution" and take_source:
            operation["conflict_id"] = take_source[0]["id"]
    payload = seal_payload(payload)
    plan = seal_plan(plan, payload["payload_digest"])
    payload_bytes = canonical_json(payload)
    plan_bytes = canonical_json(plan)
    (vector_dir / "vector-payload.json").write_bytes(payload_bytes)
    (vector_dir / "vector-plan.json").write_bytes(plan_bytes)
    vectors["payload"] = {
        "payload_digest": payload["payload_digest"],
        "canonical_size": len(payload_bytes),
        "canonical_sha256": sha256(payload_bytes),
    }
    vectors["lifecycle_plan"] = {
        "plan_digest": plan["plan_digest"],
        "canonical_size": len(plan_bytes),
        "canonical_sha256": sha256(plan_bytes),
    }
    (vector_dir / "vectors.json").write_text(json.dumps(vectors, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def verify(vector_dir: Path = VECTOR_DIR) -> list[str]:
    raw_payload, payload, raw_plan, plan, vectors = _load_pair(vector_dir)
    validate_payload(payload)
    validate_plan(plan, payload)
    canonical_payload = canonical_json(payload)
    canonical_plan = canonical_json(plan)
    if raw_payload != canonical_payload or raw_plan != canonical_plan:
        raise VectorError("vector payload/plan must be exact canonical UTF-8 byte streams")
    checks = {
        "payload digest": (payload["payload_digest"], vectors["payload"]["payload_digest"]),
        "payload size": (len(raw_payload), vectors["payload"]["canonical_size"]),
        "payload sha256": (sha256(raw_payload), vectors["payload"]["canonical_sha256"]),
        "plan digest": (plan["plan_digest"], vectors["lifecycle_plan"]["plan_digest"]),
        "plan size": (len(raw_plan), vectors["lifecycle_plan"]["canonical_size"]),
        "plan sha256": (sha256(raw_plan), vectors["lifecycle_plan"]["canonical_sha256"]),
    }
    for label, (actual, expected) in checks.items():
        if actual != expected:
            raise VectorError(f"{label}: expected {expected}, got {actual}")
    return [
        f"payload digest {payload['payload_digest']}",
        f"canonical payload {len(raw_payload)} bytes {sha256(raw_payload)}",
        f"lifecycle plan digest {plan['plan_digest']}",
        f"canonical lifecycle plan {len(raw_plan)} bytes {sha256(raw_plan)}",
        f"{len(payload['operations'])} operations and {len(payload['resolved_conflicts'])} resolved conflicts",
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vector-dir", type=Path, default=VECTOR_DIR)
    parser.add_argument("--rewrite-canonical", action="store_true")
    args = parser.parse_args()
    try:
        if args.rewrite_canonical:
            rewrite(args.vector_dir)
        results = verify(args.vector_dir)
    except (KeyError, OSError, TypeError, VectorError, json.JSONDecodeError) as error:
        print(f"reconcile plan vectors: FAIL: {error}")
        return 1
    for result in results:
        print(f"reconcile plan vectors: {result}")
    print("reconcile plan vectors: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

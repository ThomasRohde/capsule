#!/usr/bin/env python3
"""Validate lifecycle programme files and draft contracts using standard Python."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sqlite3
import sys
from typing import Any


SCRIPT = Path(__file__).resolve()
ROOT = SCRIPT.parents[2]
CONTRACTS = ROOT / "contracts"
EXAMPLES = ROOT / "examples"
MILESTONES = ROOT / "milestones"
STATUS = ROOT / "PROGRAM_STATUS.json"


class ValidationError(RuntimeError):
    pass


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValidationError(f"{path}: {exc}") from exc


def validate_json_files() -> list[str]:
    messages: list[str] = []
    for path in sorted(ROOT.rglob("*.json")):
        load_json(path)
        messages.append(f"json: {path.relative_to(ROOT)}")
    return messages


def validate_sql() -> list[str]:
    base = CONTRACTS / "capsule-v0.3-draft.sql"
    signed = CONTRACTS / "capsule-signed-app-v0.3-draft.sql"
    connection = sqlite3.connect(":memory:")
    try:
        connection.executescript(base.read_text(encoding="utf-8"))
        connection.executescript(signed.read_text(encoding="utf-8"))
        quick = connection.execute("PRAGMA quick_check").fetchone()
        if quick != ("ok",):
            raise ValidationError(f"draft SQL quick_check failed: {quick!r}")
        violations = connection.execute("PRAGMA foreign_key_check").fetchall()
        if violations:
            raise ValidationError(f"draft SQL foreign-key violations: {violations!r}")
        tables = {
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_schema WHERE type='table'"
            )
        }
        required = {
            "capsule_manifest",
            "capsule_application",
            "capsule_instance",
            "capsule_dataset",
            "capsule_lineage_event",
            "capsule_signature",
            "capsule_publisher",
        }
        missing = sorted(required - tables)
        if missing:
            raise ValidationError(f"draft SQL missing required tables: {missing}")
    except sqlite3.Error as exc:
        raise ValidationError(f"draft SQL does not compile: {exc}") from exc
    finally:
        connection.close()
    return ["sql: capsule-v0.3-draft.sql + capsule-signed-app-v0.3-draft.sql"]


def validate_status() -> list[str]:
    data = load_json(STATUS)
    milestones = data.get("milestones")
    if not isinstance(milestones, list) or len(milestones) != 10:
        raise ValidationError("PROGRAM_STATUS.json must contain ten milestones")
    expected = [f"M{index:02d}" for index in range(10)]
    actual = [item.get("id") for item in milestones]
    if actual != expected:
        raise ValidationError(f"milestone order mismatch: {actual!r}")
    states = {"pending", "in_progress", "blocked", "complete"}
    in_progress = 0
    complete: set[str] = set()
    for item in milestones:
        identifier = item["id"]
        if item.get("state") not in states:
            raise ValidationError(f"{identifier}: invalid state")
        if item["state"] == "in_progress":
            in_progress += 1
        if item["state"] == "complete":
            complete.add(identifier)
        directory = MILESTONES / f"{identifier}-{item['slug']}"
        for filename in ("EXECPLAN.md", "PROMPT.md", "ACCEPTANCE.md", "RESULT.md"):
            if not (directory / filename).is_file():
                raise ValidationError(f"{identifier}: missing {filename}")
        expected_result = (directory / "RESULT.md").relative_to(ROOT).as_posix()
        if item.get("result_path") != expected_result:
            raise ValidationError(
                f"{identifier}: result_path {item.get('result_path')!r} "
                f"!= {expected_result!r}"
            )
    if in_progress > 1:
        raise ValidationError("more than one milestone is in_progress")
    for item in milestones:
        if item["state"] in {"in_progress", "complete"}:
            missing = [dep for dep in item.get("depends_on", []) if dep not in complete]
            if missing:
                raise ValidationError(
                    f"{item['id']}: incomplete dependencies {missing!r}"
                )
    return ["status: ten ordered milestone directories and valid dependencies"]


def validate_markdown_links() -> list[str]:
    """Check local Markdown links that include an explicit path.

    Inline code paths are not interpreted. URLs and fragment-only links are skipped.
    """
    pattern = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
    checked = 0
    for path in sorted(ROOT.rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for target in pattern.findall(text):
            target = target.split("#", 1)[0].strip()
            if not target or "://" in target or target.startswith("mailto:"):
                continue
            resolved = (path.parent / target).resolve()
            try:
                resolved.relative_to(ROOT.resolve())
            except ValueError:
                # Programme docs may intentionally link to repository-root paths.
                continue
            if not resolved.exists():
                raise ValidationError(
                    f"{path.relative_to(ROOT)}: broken local link {target!r}"
                )
            checked += 1
    return [f"markdown: {checked} local links checked"]


def optional_jsonschema_validation() -> list[str]:
    try:
        import jsonschema  # type: ignore
        from referencing import Registry, Resource  # type: ignore
    except ImportError:
        return ["jsonschema: optional dependency unavailable; syntax checks only"]

    pairs = [
        (
            CONTRACTS / "compare-report-v1.schema.json",
            EXAMPLES / "compare-summary-same-release.json",
        ),
        (
            CONTRACTS / "compare-page-v1.schema.json",
            EXAMPLES / "compare-page-revealed-sensitive.json",
        ),
        (
            CONTRACTS / "compare-application-v1.schema.json",
            EXAMPLES / "compare-application-detail.json",
        ),
        (
            CONTRACTS / "tauri-compare-v1.schema.json",
            EXAMPLES / "tauri-compare-session.json",
        ),
        (
            CONTRACTS / "exact-copy-preview-v1.schema.json",
            EXAMPLES / "exact-copy-preview-v03-signed.json",
        ),
        (
            CONTRACTS / "compact-copy-preview-v1.schema.json",
            EXAMPLES / "compact-copy-preview-v02-unsigned.json",
        ),
        (
            CONTRACTS / "semantic-copy-preview-v1.schema.json",
            EXAMPLES / "semantic-copy-preview-selective.json",
        ),
        (
            CONTRACTS / "tauri-copy-v1.schema.json",
            EXAMPLES / "tauri-copy-progress.json",
        ),
        (
            CONTRACTS / "cli-reconcile-v1.schema.json",
            EXAMPLES / "cli-reconcile-candidates.json",
        ),
        (
            CONTRACTS / "cli-reconcile-v1.schema.json",
            EXAMPLES / "cli-reconcile-three-way-candidates.json",
        ),
        (
            CONTRACTS / "cli-reconcile-v1.schema.json",
            EXAMPLES / "cli-reconcile-review.json",
        ),
        (
            CONTRACTS / "cli-reconcile-v1.schema.json",
            EXAMPLES / "cli-reconcile-result.json",
        ),
        (
            CONTRACTS / "copy-preview-v1.schema.json",
            EXAMPLES / "copy-preview-template-authenticated.json",
        ),
        (
            CONTRACTS / "template-state-v1.schema.json",
            EXAMPLES / "template-state-example.json",
        ),
        (
            CONTRACTS / "capsule-application-profile-v0.3.schema.json",
            EXAMPLES / "diagram-studio-application-profile.json",
        ),
        (
            CONTRACTS / "capsule-instance-profile-v0.3.schema.json",
            EXAMPLES / "diagram-studio-instance-profile.json",
        ),
        (
            CONTRACTS / "capsule-lineage-v0.3.schema.json",
            EXAMPLES / "diagram-studio-lineage-example.json",
        ),
        (
            CONTRACTS / "capsule-data-contract-v0.3.schema.json",
            EXAMPLES / "diagram-studio-data-contract.json",
        ),
        (
            CONTRACTS / "capsule-migration-v0.3.schema.json",
            EXAMPLES / "diagram-studio-migration-v1-to-v2.json",
        ),
        (
            CONTRACTS / "upgrade-plan-v1.schema.json",
            EXAMPLES / "diagram-studio-upgrade-plan-same-schema.json",
        ),
    ]
    registry = Registry()
    for contract_path in sorted(CONTRACTS.glob("*.schema.json")):
        contract = load_json(contract_path)
        identifier = contract.get("$id")
        if isinstance(identifier, str):
            registry = registry.with_resource(
                identifier, Resource.from_contents(contract)
            )
    for schema_path, instance_path in pairs:
        schema = load_json(schema_path)
        instance = load_json(instance_path)
        jsonschema.Draft202012Validator.check_schema(schema)
        jsonschema.Draft202012Validator(schema, registry=registry).validate(instance)
    return [f"jsonschema: {len(pairs)} examples validated"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--require-jsonschema",
        action="store_true",
        help="Fail when the optional jsonschema package is unavailable.",
    )
    args = parser.parse_args()

    messages: list[str] = []
    messages += validate_json_files()
    messages += validate_sql()
    messages += validate_status()
    messages += validate_markdown_links()
    optional = optional_jsonschema_validation()
    if args.require_jsonschema and "unavailable" in optional[0]:
        raise ValidationError(optional[0])
    messages += optional
    for message in messages:
        print(message)
    print(f"ok: {len(messages)} checks/records")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValidationError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

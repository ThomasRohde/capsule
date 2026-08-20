#!/usr/bin/env python3
"""Inspect and update Capsule lifecycle programme status atomically.

Examples:

    python .../program_status.py show
    python .../program_status.py validate
    python .../program_status.py start M00
    python .../program_status.py complete M00 --summary "Baseline and ADRs accepted" \
        --evidence docs/plans/capsule-lifecycle/evidence/M00/baseline.json \
        --handoff "Start M01 by materialising the accepted v0.3 contracts."
    python .../program_status.py block M03 --summary "Windows UI runner unavailable" \
        --handoff "Run npm run test:native on a Windows x86-64 host."
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
from typing import Any


SCRIPT = Path(__file__).resolve()
PROGRAMME_ROOT = SCRIPT.parents[2]
STATUS_PATH = PROGRAMME_ROOT / "PROGRAM_STATUS.json"
VALID_STATES = {"pending", "in_progress", "blocked", "complete"}


class StatusError(RuntimeError):
    pass


def now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def load() -> dict[str, Any]:
    try:
        data = json.loads(STATUS_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise StatusError(f"cannot read {STATUS_PATH}: {exc}") from exc
    validate(data)
    return data


def milestone(data: dict[str, Any], milestone_id: str) -> dict[str, Any]:
    for item in data["milestones"]:
        if item["id"] == milestone_id:
            return item
    raise StatusError(f"unknown milestone: {milestone_id}")


def validate(data: dict[str, Any]) -> None:
    if not isinstance(data, dict) or not isinstance(data.get("milestones"), list):
        raise StatusError("status must contain a milestones array")
    seen: set[str] = set()
    in_progress = 0
    completed: set[str] = set()
    for item in data["milestones"]:
        identifier = item.get("id")
        if not isinstance(identifier, str) or identifier in seen:
            raise StatusError(f"invalid or duplicate milestone id: {identifier!r}")
        seen.add(identifier)
        state = item.get("state")
        if state not in VALID_STATES:
            raise StatusError(f"{identifier}: invalid state {state!r}")
        if state == "in_progress":
            in_progress += 1
        if state == "complete":
            completed.add(identifier)
        result_path = PROGRAMME_ROOT / item.get("result_path", "")
        if not result_path.is_file():
            raise StatusError(f"{identifier}: missing result file {result_path}")
    if in_progress > 1:
        raise StatusError("at most one milestone may be in_progress")
    for item in data["milestones"]:
        if item["state"] in {"in_progress", "complete"}:
            missing = [
                dep for dep in item.get("depends_on", []) if dep not in completed
            ]
            if missing:
                raise StatusError(
                    f"{item['id']}: dependencies are not complete: {', '.join(missing)}"
                )
    current = data.get("current_milestone")
    if current is not None and current not in seen:
        raise StatusError(f"unknown current_milestone: {current}")


def save(data: dict[str, Any]) -> None:
    validate(data)
    temporary = STATUS_PATH.with_suffix(".json.tmp")
    temporary.write_text(
        json.dumps(data, indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    os.replace(temporary, STATUS_PATH)


def recompute_overall(data: dict[str, Any]) -> None:
    states = [item["state"] for item in data["milestones"]]
    if all(state == "complete" for state in states):
        data["overall_state"] = "complete"
        data["current_milestone"] = None
        return
    if "blocked" in states:
        data["overall_state"] = "blocked"
    elif "in_progress" in states:
        data["overall_state"] = "in_progress"
    else:
        data["overall_state"] = "not_started"
    active = next(
        (
            item["id"]
            for item in data["milestones"]
            if item["state"] in {"in_progress", "blocked"}
        ),
        None,
    )
    if active is None:
        active = next(
            (item["id"] for item in data["milestones"] if item["state"] == "pending"),
            None,
        )
    data["current_milestone"] = active


def show(data: dict[str, Any]) -> None:
    print(f"programme: {data['overall_state']}")
    print(f"current:   {data.get('current_milestone') or '—'}")
    for item in data["milestones"]:
        marker = "*" if item["id"] == data.get("current_milestone") else " "
        print(f"{marker} {item['id']}  {item['state']:<11} {item['title']}")
        if item.get("last_test_summary"):
            print(f"    tests: {item['last_test_summary']}")
        if item.get("handoff"):
            print(f"    handoff: {item['handoff']}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("show")
    sub.add_parser("validate")

    start = sub.add_parser("start")
    start.add_argument("milestone")

    for name in ("complete", "block"):
        command = sub.add_parser(name)
        command.add_argument("milestone")
        command.add_argument("--summary", required=True)
        command.add_argument("--handoff", required=True)
        command.add_argument("--evidence", action="append", default=[])

    args = parser.parse_args()
    data = load()

    if args.command == "show":
        show(data)
        return 0
    if args.command == "validate":
        print("valid")
        return 0

    item = milestone(data, args.milestone)

    if args.command == "start":
        if item["state"] not in {"pending", "blocked", "in_progress"}:
            raise StatusError(f"{item['id']} is already complete")
        completed = {
            candidate["id"]
            for candidate in data["milestones"]
            if candidate["state"] == "complete"
        }
        missing = [dep for dep in item.get("depends_on", []) if dep not in completed]
        if missing:
            raise StatusError(
                f"cannot start {item['id']}; incomplete dependencies: {', '.join(missing)}"
            )
        other = next(
            (
                candidate["id"]
                for candidate in data["milestones"]
                if candidate["state"] == "in_progress"
                and candidate["id"] != item["id"]
            ),
            None,
        )
        if other:
            raise StatusError(f"{other} is already in progress")
        item["state"] = "in_progress"
        item["started_at"] = item.get("started_at") or now()
        item["completed_at"] = None
        recompute_overall(data)
        save(data)
        show(data)
        return 0

    if item["state"] != "in_progress":
        raise StatusError(
            f"{item['id']} must be in_progress before {args.command}; "
            f"current state is {item['state']}"
        )
    item["last_test_summary"] = args.summary
    item["handoff"] = args.handoff
    for evidence in args.evidence:
        if evidence not in item["evidence_paths"]:
            item["evidence_paths"].append(evidence)

    if args.command == "complete":
        item["state"] = "complete"
        item["completed_at"] = now()
    else:
        item["state"] = "blocked"
        item["completed_at"] = None

    recompute_overall(data)
    save(data)
    show(data)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except StatusError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

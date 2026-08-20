#!/usr/bin/env python3
"""Run one command and retain bounded milestone test evidence.

Use `--` before the command:

    python .../run_evidence.py --milestone M01 --name rust-core --cwd native -- \
        cargo test -p capsule-core -p capsule-crypto --all-targets

The wrapper is intentionally simple. It does not invoke a shell and therefore
does not interpret redirection, pipes or environment assignment syntax.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Any, Sequence


SCRIPT = Path(__file__).resolve()
PROGRAMME_ROOT = SCRIPT.parents[2]
REPO_ROOT = PROGRAMME_ROOT.parents[2]
MAX_CAPTURE_BYTES = 16 * 1024 * 1024


class EvidenceError(RuntimeError):
    pass


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_value(*args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        check=False,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return completed.stdout.strip() if completed.returncode == 0 else ""


def bounded_write(path: Path, data: bytes) -> dict[str, Any]:
    truncated = len(data) > MAX_CAPTURE_BYTES
    path.write_bytes(data[:MAX_CAPTURE_BYTES])
    return {
        "path": path.relative_to(REPO_ROOT).as_posix(),
        "captured_bytes": min(len(data), MAX_CAPTURE_BYTES),
        "original_bytes": len(data),
        "truncated": truncated,
        "sha256_captured": sha256_file(path),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--milestone", required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--cwd", default=".")
    parser.add_argument("--timeout", type=int, default=3600)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()

    if not args.milestone.startswith("M") or len(args.milestone) != 3:
        raise EvidenceError("--milestone must look like M00")
    command: Sequence[str] = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        raise EvidenceError("no command supplied after --")

    cwd = (REPO_ROOT / args.cwd).resolve()
    try:
        cwd.relative_to(REPO_ROOT)
    except ValueError as exc:
        raise EvidenceError("--cwd must remain inside the repository") from exc
    if not cwd.is_dir():
        raise EvidenceError(f"working directory does not exist: {cwd}")

    safe_name = "".join(
        character if character.isalnum() or character in "-_." else "-"
        for character in args.name
    ).strip(".-")
    if not safe_name:
        raise EvidenceError("--name contains no usable characters")

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output_dir = (
        PROGRAMME_ROOT / "evidence" / args.milestone / f"{stamp}-{safe_name}"
    )
    output_dir.mkdir(parents=True, exist_ok=False)

    started_at = utc_now()
    start = time.monotonic()
    timed_out = False
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=args.timeout,
            env=os.environ.copy(),
        )
        exit_code: int | None = completed.returncode
        stdout = completed.stdout
        stderr = completed.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = None
        stdout = exc.stdout or b""
        stderr = exc.stderr or b""
    duration_ms = round((time.monotonic() - start) * 1000)
    completed_at = utc_now()

    stdout_record = bounded_write(output_dir / "stdout.txt", stdout)
    stderr_record = bounded_write(output_dir / "stderr.txt", stderr)
    payload = {
        "profile": "org.sqlite-capsule.command-evidence/1",
        "milestone": args.milestone,
        "name": args.name,
        "repository": {
            "commit": git_value("rev-parse", "HEAD"),
            "status_porcelain": git_value(
                "status", "--porcelain=v1", "--untracked-files=all"
            ).splitlines(),
        },
        "command": list(command),
        "cwd": cwd.relative_to(REPO_ROOT).as_posix() or ".",
        "started_at": started_at,
        "completed_at": completed_at,
        "duration_ms": duration_ms,
        "timeout_seconds": args.timeout,
        "timed_out": timed_out,
        "exit_code": exit_code,
        "stdout": stdout_record,
        "stderr": stderr_record,
        "result": "pass" if exit_code == 0 and not timed_out else "fail",
    }
    evidence_path = output_dir / "evidence.json"
    evidence_path.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(evidence_path.relative_to(REPO_ROOT).as_posix())
    return 0 if payload["result"] == "pass" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)

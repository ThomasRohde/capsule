#!/usr/bin/env python3
"""Enforce the RustSec gate with exact, expiring reviewed exceptions."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


PROFILE = "org.sqlite-capsule.rustsec-exceptions/0.2"
ALLOWED_KINDS = {"notice", "unmaintained", "unsound"}
ALLOWED_TARGETS = {"linux", "macos", "windows"}
TARGET_TRIPLES = {
    "linux": ("x86_64-unknown-linux-gnu",),
    "macos": ("aarch64-apple-darwin", "x86_64-apple-darwin"),
    "windows": ("x86_64-pc-windows-msvc",),
}
TOP_LEVEL_FIELDS = {"profile", "cargo_audit_version", "exceptions"}
EXCEPTION_FIELDS = {
    "id",
    "package",
    "version",
    "kind",
    "targets",
    "dependency_path",
    "api_reachability",
    "rationale",
    "compensating_controls",
    "removal_condition",
    "owner",
    "review_by",
}
ADVISORY_ID = re.compile(r"^RUSTSEC-\d{4}-\d{4}$")


class GateError(RuntimeError):
    """A deterministic RustSec gate failure."""


def _require_text(value: Any, field: str) -> str:
    if not isinstance(value, str) or len(value.strip()) < 8:
        raise GateError(f"{field} must be a non-empty explanatory string")
    return value.strip()


def load_exceptions(path: Path, today: dt.date) -> dict[str, dict[str, Any]]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise GateError(f"cannot read {path}: {error}") from error

    if not isinstance(document, dict):
        raise GateError("exception document must be a JSON object")
    unknown = set(document) - TOP_LEVEL_FIELDS
    missing = TOP_LEVEL_FIELDS - set(document)
    if unknown or missing:
        raise GateError(
            f"exception document fields differ: missing={sorted(missing)}, "
            f"unknown={sorted(unknown)}"
        )
    if document["profile"] != PROFILE:
        raise GateError(f"unsupported exception profile: {document['profile']!r}")
    if not re.fullmatch(r"\d+\.\d+\.\d+", str(document["cargo_audit_version"])):
        raise GateError("cargo_audit_version must be an exact semantic version")
    if not isinstance(document["exceptions"], list):
        raise GateError("exceptions must be an array")

    exceptions: dict[str, dict[str, Any]] = {}
    for index, exception in enumerate(document["exceptions"]):
        label = f"exceptions[{index}]"
        if not isinstance(exception, dict):
            raise GateError(f"{label} must be an object")
        unknown = set(exception) - EXCEPTION_FIELDS
        missing = EXCEPTION_FIELDS - set(exception)
        if unknown or missing:
            raise GateError(
                f"{label} fields differ: missing={sorted(missing)}, "
                f"unknown={sorted(unknown)}"
            )

        advisory_id = exception["id"]
        if not isinstance(advisory_id, str) or not ADVISORY_ID.fullmatch(advisory_id):
            raise GateError(f"{label}.id is not a RustSec advisory ID")
        if advisory_id in exceptions:
            raise GateError(f"duplicate exception for {advisory_id}")
        if exception["kind"] not in ALLOWED_KINDS:
            raise GateError(f"{advisory_id}: unsupported warning kind {exception['kind']!r}")
        if not isinstance(exception["package"], str) or not exception["package"]:
            raise GateError(f"{advisory_id}: package must be non-empty")
        if not isinstance(exception["version"], str) or not exception["version"]:
            raise GateError(f"{advisory_id}: version must be non-empty")

        targets = exception["targets"]
        if (
            not isinstance(targets, list)
            or not targets
            or len(targets) != len(set(targets))
            or set(targets) - ALLOWED_TARGETS
            or targets != sorted(targets)
        ):
            raise GateError(
                f"{advisory_id}: targets must be a sorted unique subset of "
                f"{sorted(ALLOWED_TARGETS)}"
            )

        for field in (
            "dependency_path",
            "api_reachability",
            "rationale",
            "removal_condition",
            "owner",
        ):
            _require_text(exception[field], f"{advisory_id}.{field}")
        controls = exception["compensating_controls"]
        if not isinstance(controls, list) or not controls:
            raise GateError(f"{advisory_id}.compensating_controls must be non-empty")
        for control in controls:
            _require_text(control, f"{advisory_id}.compensating_controls[]")

        try:
            review_by = dt.date.fromisoformat(exception["review_by"])
        except (TypeError, ValueError) as error:
            raise GateError(f"{advisory_id}.review_by must be an ISO date") from error
        if review_by < today:
            raise GateError(
                f"{advisory_id}: exception expired on {review_by.isoformat()}"
            )
        exceptions[advisory_id] = exception

    return exceptions


def evaluate_report(
    report: dict[str, Any], exceptions: dict[str, dict[str, Any]]
) -> list[dict[str, str]]:
    vulnerabilities = report.get("vulnerabilities", {})
    vulnerable_items = vulnerabilities.get("list", [])
    if vulnerabilities.get("found") or vulnerable_items:
        ids = sorted(
            item.get("advisory", {}).get("id", "unknown")
            for item in vulnerable_items
        )
        raise GateError(f"RustSec vulnerabilities are never excepted: {ids}")

    warnings = report.get("warnings")
    if not isinstance(warnings, dict):
        raise GateError("cargo-audit report has no warnings object")

    findings: dict[str, dict[str, str]] = {}
    for group_kind, items in warnings.items():
        if not isinstance(items, list):
            raise GateError(f"cargo-audit warning group {group_kind!r} is not an array")
        for item in items:
            if not isinstance(item, dict):
                raise GateError("cargo-audit warning entry is not an object")
            advisory = item.get("advisory", {})
            package = item.get("package", {})
            finding = {
                "id": advisory.get("id"),
                "package": package.get("name"),
                "version": package.get("version"),
                "kind": item.get("kind", group_kind),
            }
            if not all(isinstance(value, str) and value for value in finding.values()):
                raise GateError(f"malformed cargo-audit warning: {finding}")
            if finding["kind"] != group_kind:
                raise GateError(
                    f"{finding['id']}: warning kind {finding['kind']!r} "
                    f"does not match group {group_kind!r}"
                )
            if finding["id"] in findings:
                raise GateError(f"duplicate cargo-audit warning {finding['id']}")
            findings[finding["id"]] = finding

    new_ids = sorted(set(findings) - set(exceptions))
    stale_ids = sorted(set(exceptions) - set(findings))
    if new_ids:
        raise GateError(f"unreviewed RustSec warnings: {new_ids}")
    if stale_ids:
        raise GateError(
            "stale RustSec exceptions must be removed or re-reviewed: "
            f"{stale_ids}"
        )

    accepted: list[dict[str, str]] = []
    for advisory_id in sorted(findings):
        finding = findings[advisory_id]
        exception = exceptions[advisory_id]
        expected = {
            key: exception[key] for key in ("id", "package", "version", "kind")
        }
        if finding != expected:
            raise GateError(
                f"{advisory_id}: finding changed: expected={expected}, actual={finding}"
            )
        accepted.append(finding)
    return accepted


def evaluate_target_reachability(
    exceptions: dict[str, dict[str, Any]],
    actual: dict[tuple[str, str], set[str]],
) -> None:
    for advisory_id, exception in sorted(exceptions.items()):
        package_key = (exception["package"], exception["version"])
        expected_targets = set(exception["targets"])
        actual_targets = actual.get(package_key, set())
        if actual_targets != expected_targets:
            raise GateError(
                f"{advisory_id}: target reachability changed for "
                f"{exception['package']} {exception['version']}: "
                f"expected={sorted(expected_targets)}, actual={sorted(actual_targets)}"
            )


def _run(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        check=False,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    )


def _audit_database_args(advisory_db: Path | None) -> list[str]:
    if advisory_db is None:
        return []
    return ["--db", str(advisory_db)]


def _load_audit_report(
    cargo: str, native_root: Path, advisory_db: Path | None
) -> dict[str, Any]:
    result = _run(
        [
            cargo,
            "audit",
            "--json",
            *_audit_database_args(advisory_db),
            *(["--no-fetch"] if advisory_db is not None else []),
        ],
        native_root,
    )
    if not result.stdout.strip():
        raise GateError(
            "cargo audit produced no JSON report: "
            f"exit={result.returncode}, stderr={result.stderr.strip()}"
        )
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise GateError(f"cargo audit emitted invalid JSON: {error}") from error


def _verify_tool_version(cargo: str, native_root: Path, expected: str) -> None:
    result = _run([cargo, "audit", "--version"], native_root)
    actual = result.stdout.strip()
    # cargo's subcommand dispatch can give clap either the package name or the
    # composed cargo-audit command name. Accept only those two names and the
    # exact pinned version.
    accepted = {f"cargo-audit {expected}", f"cargo-audit-audit {expected}"}
    if result.returncode != 0 or actual not in accepted:
        raise GateError(
            f"expected cargo-audit {expected}, got {actual!r}; "
            f"stderr={result.stderr.strip()!r}"
        )


def _run_strict_audit(
    cargo: str,
    native_root: Path,
    advisory_ids: list[str],
    advisory_db: Path | None,
) -> None:
    command = [
        cargo,
        "audit",
        "--no-fetch",
        "--deny",
        "warnings",
        *_audit_database_args(advisory_db),
    ]
    for advisory_id in advisory_ids:
        command.extend(["--ignore", advisory_id])
    result = _run(command, native_root)
    if result.returncode != 0:
        raise GateError(
            "strict cargo audit failed after applying only reviewed exceptions:\n"
            f"{result.stdout}{result.stderr}"
        )


def _collect_target_reachability(
    cargo: str,
    native_root: Path,
    exceptions: dict[str, dict[str, Any]],
) -> dict[tuple[str, str], set[str]]:
    packages = sorted(
        {(entry["package"], entry["version"]) for entry in exceptions.values()}
    )
    actual: dict[tuple[str, str], set[str]] = {}
    for package, version in packages:
        reached: set[str] = set()
        for target_name, target_triples in TARGET_TRIPLES.items():
            architecture_results: list[bool] = []
            for target_triple in target_triples:
                result = _run(
                    [
                        cargo,
                        "tree",
                        "--locked",
                        "--target",
                        target_triple,
                        "--invert",
                        f"{package}@{version}",
                        "--prefix",
                        "none",
                        "--depth",
                        "0",
                    ],
                    native_root,
                )
                if result.returncode != 0:
                    raise GateError(
                        f"cannot resolve {package} {version} for {target_triple}: "
                        f"{result.stderr.strip()}"
                    )
                architecture_results.append(bool(result.stdout.strip()))
            if len(set(architecture_results)) != 1:
                raise GateError(
                    f"{package} {version}: {target_name} architecture "
                    f"reachability differs across {list(target_triples)}"
                )
            if architecture_results[0]:
                reached.add(target_name)
        actual[(package, version)] = reached
    return actual


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--exceptions",
        type=Path,
        help="exception JSON (defaults to native/rustsec-exceptions.json)",
    )
    parser.add_argument(
        "--report",
        type=Path,
        help="validate a saved cargo-audit JSON report without invoking Cargo",
    )
    parser.add_argument(
        "--advisory-db",
        type=Path,
        help="use an existing clean advisory database checkout without fetching it",
    )
    args = parser.parse_args(argv)

    native_root = Path(__file__).resolve().parents[1]
    exception_path = args.exceptions or native_root / "rustsec-exceptions.json"
    try:
        exceptions = load_exceptions(exception_path, dt.date.today())
        document = json.loads(exception_path.read_text(encoding="utf-8"))
        if args.report:
            report = json.loads(args.report.read_text(encoding="utf-8"))
        else:
            cargo = shutil.which("cargo")
            if cargo is None:
                raise GateError("cargo is not on PATH")
            advisory_db = args.advisory_db.resolve(strict=True) if args.advisory_db else None
            if advisory_db is not None and not (advisory_db / ".git").exists():
                raise GateError("--advisory-db must identify a Git checkout")
            _verify_tool_version(
                cargo, native_root, document["cargo_audit_version"]
            )
            report = _load_audit_report(cargo, native_root, advisory_db)

        accepted = evaluate_report(report, exceptions)
        if not args.report:
            actual_targets = _collect_target_reachability(
                cargo, native_root, exceptions
            )
            evaluate_target_reachability(exceptions, actual_targets)
            _run_strict_audit(
                cargo,
                native_root,
                [finding["id"] for finding in accepted],
                advisory_db,
            )
    except (GateError, OSError, json.JSONDecodeError) as error:
        print(f"RustSec gate failed: {error}", file=sys.stderr)
        return 1

    print(
        f"RustSec gate passed: no vulnerabilities; "
        f"{len(accepted)} exact reviewed warning exception(s)"
    )
    for finding in accepted:
        print(
            f"  {finding['id']} {finding['package']} {finding['version']} "
            f"({finding['kind']})"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

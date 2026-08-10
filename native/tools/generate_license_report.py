#!/usr/bin/env python3
"""Generate a deterministic third-party license inventory from Cargo metadata."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "THIRD_PARTY_LICENSES.md"


def cargo_metadata() -> dict[str, object]:
    cargo_name = "cargo.exe" if sys.platform == "win32" else "cargo"
    command = [
        str(Path.home() / ".cargo" / "bin" / cargo_name),
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--all-features",
    ]
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if result.returncode:
        detail = result.stderr.strip() or "cargo metadata failed"
        lines = detail.splitlines()
        if len(lines) > 40:
            detail = "\n".join(
                [
                    f"cargo metadata failed ({len(lines)} diagnostic lines; showing the last 40)",
                    *lines[-40:],
                ]
            )
        raise RuntimeError(detail)
    return json.loads(result.stdout)


def render(metadata: dict[str, object]) -> str:
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise RuntimeError("cargo metadata omitted packages")

    rows: list[tuple[str, str, str, str]] = []
    missing: list[str] = []
    for package in packages:
        if not isinstance(package, dict) or package.get("source") is None:
            continue
        name = package.get("name")
        version = package.get("version")
        source = package.get("source")
        license_expression = package.get("license")
        license_file = package.get("license_file")
        if not all(isinstance(value, str) for value in (name, version, source)):
            raise RuntimeError("cargo metadata contained an invalid package record")
        if isinstance(license_expression, str) and license_expression.strip():
            license_value = license_expression.strip()
        elif isinstance(license_file, str) and license_file.strip():
            license_value = f"License file: {Path(license_file).name}"
        else:
            missing.append(f"{name} {version}")
            continue
        rows.append((name, version, license_value, source))

    if missing:
        raise RuntimeError("packages without license metadata: " + ", ".join(sorted(missing)))
    rows.sort(key=lambda row: (row[0].casefold(), row[1], row[3]))

    expressions = sorted({row[2] for row in rows}, key=str.casefold)
    lines = [
        "# Native third-party license inventory",
        "",
        "This deterministic inventory is generated from `native/Cargo.lock` via",
        "`cargo metadata --locked --all-features`. It includes registry packages for",
        "all resolved target branches; workspace crates are MIT and are not repeated.",
        "A package without a Cargo license expression or declared license file makes",
        "generation fail.",
        "",
        f"Resolved third-party package records: **{len(rows)}**.",
        "",
        "License expressions/files present:",
        "",
    ]
    lines.extend(f"- `{expression}`" for expression in expressions)
    lines.extend(
        [
            "",
            "| Package | Version | License | Locked source |",
            "| --- | --- | --- | --- |",
        ]
    )
    for name, version, license_value, source in rows:
        lines.append(
            f"| `{name}` | `{version}` | `{license_value}` | `{source}` |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    output = args.output.resolve()
    content = render(cargo_metadata())
    if args.check:
        if not output.is_file() or output.read_text(encoding="utf-8") != content:
            print(f"stale license report: {output}", file=sys.stderr)
            return 1
        print(json.dumps({"ok": True, "output": str(output)}, indent=2))
        return 0
    output.write_text(content, encoding="utf-8", newline="\n")
    print(json.dumps({"ok": True, "output": str(output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

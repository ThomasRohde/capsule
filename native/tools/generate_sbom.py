#!/usr/bin/env python3
"""Generate a deterministic CycloneDX SBOM from the locked Cargo graph."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
import urllib.parse
import uuid
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "sbom.cdx.json"
NAMESPACE = uuid.UUID("5d2fa8ec-3c6b-5dce-8663-51ac0e29b6a7")


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


def lock_checksums() -> dict[tuple[str, str, str], str]:
    document = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    output: dict[tuple[str, str, str], str] = {}
    for package in document.get("package", []):
        source = package.get("source", "")
        checksum = package.get("checksum")
        if isinstance(checksum, str):
            output[(package["name"], package["version"], source)] = checksum
    return output


def purl(name: str, version: str, source: str | None) -> str:
    value = f"pkg:cargo/{urllib.parse.quote(name, safe='')}@{urllib.parse.quote(version, safe='')}"
    if source:
        value += "?repository_url=" + urllib.parse.quote(source, safe="")
    return value


def render(metadata: dict[str, object]) -> str:
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    workspace_members = metadata.get("workspace_members")
    if (
        not isinstance(packages, list)
        or not isinstance(resolve, dict)
        or not isinstance(resolve.get("nodes"), list)
        or not isinstance(workspace_members, list)
    ):
        raise RuntimeError("cargo metadata omitted packages, resolution, or workspace members")

    checksums = lock_checksums()
    components: list[dict[str, object]] = []
    refs: dict[str, str] = {}
    for package in packages:
        if not isinstance(package, dict):
            raise RuntimeError("cargo metadata contained an invalid package record")
        package_id = package.get("id")
        name = package.get("name")
        version = package.get("version")
        source = package.get("source")
        license_expression = package.get("license")
        if not all(isinstance(value, str) for value in (package_id, name, version)):
            raise RuntimeError("cargo metadata package identity is invalid")
        if source is not None and not isinstance(source, str):
            raise RuntimeError("cargo metadata package source is invalid")
        if not isinstance(license_expression, str) or not license_expression.strip():
            license_file = package.get("license_file")
            if not isinstance(license_file, str) or not license_file.strip():
                raise RuntimeError(f"package without license metadata: {name} {version}")
            license_expression = f"LicenseRef-{Path(license_file).name}"
        reference = purl(name, version, source)
        refs[package_id] = reference
        component: dict[str, object] = {
            "type": "library",
            "bom-ref": reference,
            "name": name,
            "version": version,
            # Cargo metadata contains a few historical slash-separated values
            # that are not valid SPDX expressions. Preserve every declaration
            # exactly as a CycloneDX license name instead of rewriting meaning.
            "licenses": [{"license": {"name": license_expression.strip()}}],
            "purl": reference,
        }
        if source is None:
            component["group"] = "sqlite-capsule"
        if source:
            component["externalReferences"] = [
                {
                    "type": "distribution",
                    "url": source.removeprefix("registry+"),
                }
            ]
            checksum = checksums.get((name, version, source))
            if checksum:
                component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        components.append(component)
    components.sort(key=lambda value: str(value["bom-ref"]))

    dependency_rows: list[dict[str, object]] = []
    for node in resolve["nodes"]:
        if not isinstance(node, dict) or not isinstance(node.get("id"), str):
            raise RuntimeError("cargo metadata resolution node is invalid")
        reference = refs.get(node["id"])
        dependencies = node.get("dependencies")
        if reference is None or not isinstance(dependencies, list):
            raise RuntimeError("cargo metadata resolution references an unknown package")
        dependency_rows.append(
            {
                "ref": reference,
                "dependsOn": sorted(refs[item] for item in dependencies),
            }
        )

    root_ref = "pkg:generic/sqlite-capsule-native-workspace@0.2.0"
    dependency_rows.append(
        {
            "ref": root_ref,
            "dependsOn": sorted(refs[item] for item in workspace_members),
        }
    )
    dependency_rows.sort(key=lambda value: str(value["ref"]))
    graph_fingerprint = "\n".join(str(value["bom-ref"]) for value in components)
    document = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.uuid5(NAMESPACE, graph_fingerprint)}",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": root_ref,
                "name": "sqlite-capsule-native-workspace",
                "version": "0.2.0",
            },
            "properties": [
                {"name": "sqlite-capsule:cargo-locked", "value": "true"},
                {"name": "sqlite-capsule:cargo-all-features", "value": "true"},
            ],
        },
        "components": components,
        "dependencies": dependency_rows,
    }
    return json.dumps(document, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    content = render(cargo_metadata())
    if arguments.check:
        if not output.is_file() or output.read_text(encoding="utf-8") != content:
            print(f"stale SBOM: {output}", file=sys.stderr)
            return 1
        print(json.dumps({"ok": True, "output": str(output)}, indent=2))
        return 0
    output.write_text(content, encoding="utf-8", newline="\n")
    print(json.dumps({"ok": True, "output": str(output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

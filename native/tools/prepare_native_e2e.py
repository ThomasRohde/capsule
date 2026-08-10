#!/usr/bin/env python3
"""Prepare pinned, workspace-local Windows drivers for native WebView2 tests."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[2]
TOOLS_ROOT = ROOT / ".tmp" / "native-e2e-tools"
BIN_DIR = TOOLS_ROOT / "bin"
EDGE_DIR = TOOLS_ROOT / "edge"
TAURI_DRIVER_VERSION = "2.0.6"
EDGE_TOOL_REPOSITORY = "https://github.com/chippers/msedgedriver-tool.git"
EDGE_TOOL_REVISION = "8c4b34f51b45f5cf08013366d703de464ab871d1"


def executable(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def cargo_path() -> Path:
    candidates = []
    if value := os.environ.get("CARGO"):
        candidates.append(Path(value))
    if value := shutil.which("cargo"):
        candidates.append(Path(value))
    if value := os.environ.get("CARGO_HOME"):
        candidates.append(Path(value) / "bin" / executable("cargo"))
    if value := os.environ.get("USERPROFILE"):
        candidates.append(Path(value) / ".cargo" / "bin" / executable("cargo"))
    candidates.append(Path.home() / ".cargo" / "bin" / executable("cargo"))
    for candidate in candidates:
        if candidate.is_file():
            # On Windows the rustup proxies may resolve to rustup.exe. Execute
            # the explicitly named cargo proxy so argv[0] selects Cargo.
            return candidate.absolute()
    raise RuntimeError("cargo was not found; set CARGO to the exact executable path")


def run(command: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        check=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def installed_package_matches(prefix: str, revision: str | None = None) -> bool:
    manifest = TOOLS_ROOT / ".crates.toml"
    if not manifest.is_file():
        return False
    try:
        packages = tomllib.loads(manifest.read_text(encoding="utf-8")).get("v1", {})
    except (OSError, tomllib.TOMLDecodeError):
        return False
    return any(
        entry.startswith(prefix) and (revision is None or revision in entry)
        for entry in packages
    )


def edge_driver_path() -> Path | None:
    direct = EDGE_DIR / executable("msedgedriver")
    if direct.is_file():
        return direct.resolve()
    matches = sorted(EDGE_DIR.glob(f"**/{executable('msedgedriver')}"))
    return matches[0].resolve() if matches else None


def current_report() -> dict[str, object]:
    tauri_driver = BIN_DIR / executable("tauri-driver")
    edge_tool = BIN_DIR / executable("msedgedriver-tool")
    edge_driver = edge_driver_path()
    return {
        "contract": "org.sqlite-capsule.native-e2e-tools/0.2",
        "platform": sys.platform,
        "tauri_driver": str(tauri_driver.resolve()) if tauri_driver.is_file() else None,
        "tauri_driver_version": TAURI_DRIVER_VERSION,
        "edge_tool": str(edge_tool.resolve()) if edge_tool.is_file() else None,
        "edge_tool_revision": EDGE_TOOL_REVISION,
        "edge_driver": str(edge_driver) if edge_driver else None,
        "ready": tauri_driver.is_file()
        and installed_package_matches(f"tauri-driver {TAURI_DRIVER_VERSION} ")
        and edge_tool.is_file()
        and installed_package_matches("msedgedriver-tool ", EDGE_TOOL_REVISION)
        and edge_driver is not None,
    }


def prepare() -> dict[str, object]:
    if os.name != "nt":
        raise RuntimeError(
            "this local preparer is Windows-only; macOS/Linux native setup is deferred "
            "until public platform runners are available"
        )
    TOOLS_ROOT.mkdir(parents=True, exist_ok=True)
    EDGE_DIR.mkdir(parents=True, exist_ok=True)
    cargo = cargo_path()
    tauri_driver = BIN_DIR / executable("tauri-driver")
    if not (
        tauri_driver.is_file()
        and installed_package_matches(f"tauri-driver {TAURI_DRIVER_VERSION} ")
    ):
        run(
            [
                str(cargo),
                "install",
                "tauri-driver",
                "--version",
                TAURI_DRIVER_VERSION,
                "--locked",
                "--root",
                str(TOOLS_ROOT),
                "--force",
            ],
            cwd=ROOT,
        )
    edge_tool = BIN_DIR / executable("msedgedriver-tool")
    if not (
        edge_tool.is_file()
        and installed_package_matches("msedgedriver-tool ", EDGE_TOOL_REVISION)
    ):
        run(
            [
                str(cargo),
                "install",
                "--git",
                EDGE_TOOL_REPOSITORY,
                "--rev",
                EDGE_TOOL_REVISION,
                "--locked",
                "--root",
                str(TOOLS_ROOT),
            ],
            cwd=ROOT,
        )
    run([str(edge_tool)], cwd=EDGE_DIR)
    report = current_report()
    if not report["ready"]:
        raise RuntimeError("native WebDriver preparation did not produce both exact drivers")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the exact workspace-local tools without downloading or changing them",
    )
    args = parser.parse_args()
    try:
        report = current_report() if args.check else prepare()
        if args.check and not report["ready"]:
            raise RuntimeError(
                "native E2E tools are missing or stale; run npm run test:native:prepare"
            )
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"native E2E preparation failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

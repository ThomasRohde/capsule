#!/usr/bin/env python3
"""Repository entry point for capsule runtime and authoring operations."""

from __future__ import annotations

import sys
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from runtime.capsule_host import CapsuleDatabase, main as host_main  # noqa: E402
from tools.capsule_author import main as author_main  # noqa: E402


AUTHORING_COMMANDS = {"unpack", "pack", "diff"}
HTML_COMMANDS = {"export-html", "inspect-html", "verify-html"}


def permissions_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Inspect requested and fine-grained capsule grants")
    parser.add_argument("capsule", type=Path)
    args = parser.parse_args(argv)
    with CapsuleDatabase(args.capsule, read_only=True) as capsule:
        print(json.dumps(capsule.permissions(), indent=2, sort_keys=True))
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] in {"-h", "--help"}:
        print(
            "SQLite Capsule v0.2/v0.3 repository CLI\n\n"
            "Runtime commands:\n"
            "  inspect, instructions, assets, verify, extract-host, run, start, status, stop\n\n"
            "Conformance commands:\n"
            "  conformance, permissions\n\n"
            "Authoring commands:\n"
            "  unpack, pack, diff\n\n"
            "HTML export commands:\n"
            "  export-html, inspect-html, verify-html\n\n"
            "Signature commands:\n"
            "  signatures\n\n"
            "Run `python tools/capsule.py <command> --help` for command details."
        )
        return 0
    if len(sys.argv) > 1 and sys.argv[1] in AUTHORING_COMMANDS:
        return author_main(sys.argv[1:])
    if len(sys.argv) > 1 and sys.argv[1] in HTML_COMMANDS:
        from tools.capsule_html import main as html_main

        return html_main(sys.argv[1:])
    if len(sys.argv) > 1 and sys.argv[1] == "conformance":
        from tools.capsule_conformance import main as conformance_main

        return conformance_main(sys.argv[2:])
    if len(sys.argv) > 1 and sys.argv[1] == "permissions":
        return permissions_main(sys.argv[2:])
    if len(sys.argv) > 1 and sys.argv[1] == "signatures":
        from tools.capsule_signatures import main as signatures_main

        return signatures_main(sys.argv[2:])
    return host_main()


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Small, dependency-free host for SQLite Capsule v0.2 and v0.3 files.

This file is intentionally self-contained so a copy can be embedded in a capsule
and extracted by a coding agent when no installed host is available.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import mimetypes
import os
import re
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import webbrowser
from dataclasses import dataclass
from datetime import datetime, timezone
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Iterable, Mapping

APPLICATION_ID = 1129337676  # 0x4350534C, "CPSL"
FORMAT_ID = "org.sqlite-capsule"
FORMAT_VERSION = "0.2"
RUNTIME_PROTOCOL = "capsule-http/0.2"
USER_VERSION = 2
SUPPORTED_FORMAT_PROFILES = {
    2: {
        "format_id": FORMAT_ID,
        "format_version": "0.2",
        "runtime_protocol": RUNTIME_PROTOCOL,
        "minimum_host_profile": None,
    },
    3: {
        "format_id": FORMAT_ID,
        "format_version": "0.3",
        "runtime_protocol": RUNTIME_PROTOCOL,
        "minimum_host_profile": "org.sqlite-capsule.host-profile/0.3",
    },
}
SUPPORTED_RUNTIME_PROTOCOLS = {RUNTIME_PROTOCOL}
MAX_REQUEST_BYTES = 1_048_576
MAX_CAPSULE_BYTES = 64 * 1_048_576
MAX_ASSET_BYTES = 16 * 1_048_576
MAX_RESULT_ROWS = 1_000
MAX_RESULT_BYTES = 2 * 1_048_576
MAX_CONCURRENT_REQUESTS = 8
MAX_JSON_DEPTH = 32
ENDPOINT_TIMEOUT_SECONDS = 3.0
MAX_ENDPOINT_STEPS = 16

REQUIRED_TABLES = {
    "capsule_manifest",
    "capsule_asset",
    "capsule_runbook",
    "capsule_command",
    "capsule_doc",
    "capsule_endpoint",
    "capsule_endpoint_step",
    "capsule_grant",
    "capsule_check",
    "capsule_prompt",
    "capsule_change_log",
}
REQUIRED_VIEWS = {"START_HERE"}
PROTECTED_PREFIXES = ("capsule_", "sqlite_")

REQUIRED_TABLE_COLUMNS = {
    "capsule_manifest": {
        "id",
        "format_id",
        "format_version",
        "capsule_id",
        "title",
        "summary",
        "app_id",
        "app_version",
        "entry_asset",
        "runtime_protocol",
        "permissions_json",
        "created_at",
        "updated_at",
    },
    "capsule_asset": {
        "path",
        "media_type",
        "content",
        "sha256",
        "executable",
        "cache_policy",
        "description",
    },
    "capsule_command": {
        "id",
        "purpose",
        "platform",
        "cwd",
        "command_template",
        "argv_json",
        "risk_class",
        "success_condition",
    },
    "capsule_runbook": {
        "id",
        "audience",
        "sequence",
        "title",
        "body_md",
        "command_id",
    },
    "capsule_doc": {"slug", "title", "media_type", "content", "sequence"},
    "capsule_endpoint": {
        "name",
        "operation",
        "sql_text",
        "parameters_json",
        "result_mode",
        "description",
        "enabled",
    },
    "capsule_endpoint_step": {
        "endpoint_name",
        "sequence",
        "sql_text",
        "required_changes",
    },
    "capsule_grant": {"capability", "decision", "reason", "granted_at"},
    "capsule_check": {
        "id",
        "severity",
        "description",
        "sql_text",
        "result_mode",
        "expected_json",
    },
    "capsule_prompt": {"id", "title", "prompt_text", "sequence"},
    "capsule_change_log": {
        "id",
        "endpoint_name",
        "parameters_json",
        "changed_rows",
        "occurred_at",
    },
}
OPTIONAL_TABLE_COLUMNS: dict[str, dict[str, str]] = {
    "capsule_publisher": {
        "id": "INTEGER", "profile": "TEXT", "publisher_id": "TEXT", "publisher_name": "TEXT",
    },
    "capsule_signature": {
        "key_id": "TEXT", "algorithm": "TEXT", "public_key": "BLOB",
        "application_digest": "BLOB", "signature": "BLOB", "signed_at": "TEXT",
    },
}
OPTIONAL_PRIMARY_KEYS: dict[str, tuple[str, ...]] = {
    "capsule_publisher": ("id",),
    "capsule_signature": ("key_id",),
}
START_HERE_COLUMNS = (
    "sequence",
    "audience",
    "title",
    "instruction",
    "command_id",
    "command_purpose",
    "platform",
    "cwd",
    "command_template",
    "argv_json",
    "risk_class",
    "success_condition",
)
REQUIRED_CONTENT_TABLES = {
    "capsule_asset": "application assets",
    "capsule_command": "launch commands",
    "capsule_runbook": "agent runbook steps",
    "capsule_doc": "embedded documentation",
    "capsule_endpoint": "named data-access endpoints",
    "capsule_check": "application validation checks",
    "capsule_prompt": "agent prompts",
}
REQUIRED_PRIMARY_KEYS = {
    "capsule_manifest": ("id",),
    "capsule_asset": ("path",),
    "capsule_command": ("id",),
    "capsule_runbook": ("id",),
    "capsule_doc": ("slug",),
    "capsule_endpoint": ("name",),
    "capsule_endpoint_step": ("endpoint_name", "sequence"),
    "capsule_grant": ("capability",),
    "capsule_check": ("id",),
    "capsule_prompt": ("id",),
    "capsule_change_log": ("id",),
}
REQUIRED_FOREIGN_KEYS = {
    "capsule_runbook": {("command_id", "capsule_command", "id")},
    "capsule_endpoint_step": {("endpoint_name", "capsule_endpoint", "name")},
}
REQUIRED_COLUMN_TYPES = {
    "capsule_manifest": {
        "id": "INTEGER",
        **dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_manifest"] - {"id"}, "TEXT"),
    },
    "capsule_asset": {
        **dict.fromkeys(
            REQUIRED_TABLE_COLUMNS["capsule_asset"] - {"content", "executable"}, "TEXT"
        ),
        "content": "BLOB",
        "executable": "INTEGER",
    },
    "capsule_command": dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_command"], "TEXT"),
    "capsule_runbook": {
        **dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_runbook"] - {"sequence"}, "TEXT"),
        "sequence": "INTEGER",
    },
    "capsule_doc": {
        **dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_doc"] - {"sequence"}, "TEXT"),
        "sequence": "INTEGER",
    },
    "capsule_endpoint": {
        **dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_endpoint"] - {"enabled"}, "TEXT"),
        "enabled": "INTEGER",
    },
    "capsule_endpoint_step": {
        "endpoint_name": "TEXT",
        "sequence": "INTEGER",
        "sql_text": "TEXT",
        "required_changes": "INTEGER",
    },
    "capsule_grant": dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_grant"], "TEXT"),
    "capsule_check": dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_check"], "TEXT"),
    "capsule_prompt": {
        **dict.fromkeys(REQUIRED_TABLE_COLUMNS["capsule_prompt"] - {"sequence"}, "TEXT"),
        "sequence": "INTEGER",
    },
    "capsule_change_log": {
        **dict.fromkeys(
            REQUIRED_TABLE_COLUMNS["capsule_change_log"] - {"id", "changed_rows"}, "TEXT"
        ),
        "id": "INTEGER",
        "changed_rows": "INTEGER",
    },
}
NULLABLE_COLUMNS = {
    ("capsule_asset", "description"),
    ("capsule_command", "argv_json"),
    ("capsule_runbook", "command_id"),
    ("capsule_endpoint_step", "required_changes"),
    ("capsule_grant", "granted_at"),
}


@dataclass(frozen=True)
class FormatContract:
    format_version: str
    runtime_protocol: str
    required_table_columns: Mapping[str, set[str]]
    required_column_types: Mapping[str, Mapping[str, str]]
    required_primary_keys: Mapping[str, tuple[str, ...]]
    required_foreign_keys: Mapping[str, set[tuple[str, str, str]]]
    nullable_columns: set[tuple[str, str]]
    required_content_tables: Mapping[str, str]


V03_REQUIRED_TABLE_COLUMNS = {
    "capsule_manifest": {
        "id", "format_id", "format_version", "app_id", "app_version",
        "entry_asset", "runtime_protocol", "permissions_json", "data_schema_id",
        "data_schema_version", "minimum_host_profile", "released_at",
    },
    "capsule_application": {
        "id", "name", "description", "category", "icon_asset", "release_notes_doc",
    },
    "capsule_instance_asset": {
        "id", "media_type", "content", "sha256", "width", "height", "description",
    },
    "capsule_instance": {
        "id", "capsule_id", "revision_id", "title", "description", "document_kind",
        "tags_json", "icon_asset_id", "cover_asset_id", "created_at",
        "content_updated_at",
    },
    "capsule_grant": {"capability", "decision", "reason", "granted_at"},
    "capsule_asset": REQUIRED_TABLE_COLUMNS["capsule_asset"],
    "capsule_command": REQUIRED_TABLE_COLUMNS["capsule_command"],
    "capsule_runbook": REQUIRED_TABLE_COLUMNS["capsule_runbook"],
    "capsule_doc": REQUIRED_TABLE_COLUMNS["capsule_doc"],
    "capsule_endpoint": REQUIRED_TABLE_COLUMNS["capsule_endpoint"],
    "capsule_endpoint_step": REQUIRED_TABLE_COLUMNS["capsule_endpoint_step"],
    "capsule_check": REQUIRED_TABLE_COLUMNS["capsule_check"],
    "capsule_prompt": REQUIRED_TABLE_COLUMNS["capsule_prompt"],
    "capsule_dataset": {
        "id", "role", "description", "fork_policy", "compare_policy",
        "reconcile_policy", "upgrade_policy", "sensitivity", "required",
    },
    "capsule_dataset_table": {
        "dataset_id", "table_name", "sequence", "primary_key_json",
        "ignored_columns_json", "immutable_columns_json",
    },
    "capsule_dataset_dependency": {"dataset_id", "depends_on_dataset_id", "reason"},
    "capsule_migration": {
        "id", "data_schema_id", "from_version", "to_version", "description",
        "operation_profile", "reversible",
    },
    "capsule_migration_step": {"migration_id", "sequence", "operation", "definition_json"},
    "capsule_migration_check": {
        "migration_id", "sequence", "stage", "severity", "description", "definition_json",
    },
    "capsule_lineage_event": {
        "event_id", "sequence", "operation", "result_capsule_id", "result_revision_id",
        "occurred_at", "application_digest", "data_schema_id", "data_schema_version",
        "plan_digest", "details_json",
    },
    "capsule_lineage_parent": {
        "event_id", "ordinal", "relation", "parent_capsule_id", "parent_revision_id",
        "parent_file_sha256",
    },
    "capsule_change_log": REQUIRED_TABLE_COLUMNS["capsule_change_log"],
}


def _declared_types(
    columns: set[str],
    *,
    integers: Iterable[str] = (),
    blobs: Iterable[str] = (),
) -> dict[str, str]:
    result = dict.fromkeys(columns, "TEXT")
    result.update(dict.fromkeys(integers, "INTEGER"))
    result.update(dict.fromkeys(blobs, "BLOB"))
    return result


V03_REQUIRED_COLUMN_TYPES = {
    "capsule_manifest": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_manifest"],
        integers=("id", "data_schema_version"),
    ),
    "capsule_application": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_application"], integers=("id",)
    ),
    "capsule_instance_asset": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_instance_asset"],
        integers=("width", "height"), blobs=("content",),
    ),
    "capsule_instance": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_instance"], integers=("id",)
    ),
    "capsule_dataset": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_dataset"], integers=("required",)
    ),
    "capsule_dataset_table": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_dataset_table"], integers=("sequence",)
    ),
    "capsule_dataset_dependency": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_dataset_dependency"]
    ),
    "capsule_migration": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_migration"],
        integers=("from_version", "to_version", "reversible"),
    ),
    "capsule_migration_step": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_migration_step"], integers=("sequence",)
    ),
    "capsule_migration_check": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_migration_check"], integers=("sequence",)
    ),
    "capsule_lineage_event": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_lineage_event"],
        integers=("sequence", "data_schema_version"),
    ),
    "capsule_lineage_parent": _declared_types(
        V03_REQUIRED_TABLE_COLUMNS["capsule_lineage_parent"], integers=("ordinal",)
    ),
    **{
        table: REQUIRED_COLUMN_TYPES[table]
        for table in (
            "capsule_grant", "capsule_asset", "capsule_command", "capsule_runbook",
            "capsule_doc", "capsule_endpoint", "capsule_endpoint_step", "capsule_check",
            "capsule_prompt", "capsule_change_log",
        )
    },
}

V03_REQUIRED_PRIMARY_KEYS = {
    "capsule_manifest": ("id",),
    "capsule_application": ("id",),
    "capsule_instance_asset": ("id",),
    "capsule_instance": ("id",),
    "capsule_grant": ("capability",),
    "capsule_asset": ("path",),
    "capsule_command": ("id",),
    "capsule_runbook": ("id",),
    "capsule_doc": ("slug",),
    "capsule_endpoint": ("name",),
    "capsule_endpoint_step": ("endpoint_name", "sequence"),
    "capsule_check": ("id",),
    "capsule_prompt": ("id",),
    "capsule_dataset": ("id",),
    "capsule_dataset_table": ("dataset_id", "table_name"),
    "capsule_dataset_dependency": ("dataset_id", "depends_on_dataset_id"),
    "capsule_migration": ("id",),
    "capsule_migration_step": ("migration_id", "sequence"),
    "capsule_migration_check": ("migration_id", "sequence"),
    "capsule_lineage_event": ("event_id",),
    "capsule_lineage_parent": ("event_id", "ordinal"),
    "capsule_change_log": ("id",),
}

V03_REQUIRED_FOREIGN_KEYS = {
    "capsule_application": {
        ("icon_asset", "capsule_asset", "path"),
        ("release_notes_doc", "capsule_doc", "slug"),
    },
    "capsule_instance": {
        ("icon_asset_id", "capsule_instance_asset", "id"),
        ("cover_asset_id", "capsule_instance_asset", "id"),
    },
    "capsule_runbook": {("command_id", "capsule_command", "id")},
    "capsule_endpoint_step": {("endpoint_name", "capsule_endpoint", "name")},
    "capsule_dataset_table": {("dataset_id", "capsule_dataset", "id")},
    "capsule_dataset_dependency": {
        ("dataset_id", "capsule_dataset", "id"),
        ("depends_on_dataset_id", "capsule_dataset", "id"),
    },
    "capsule_migration_step": {("migration_id", "capsule_migration", "id")},
    "capsule_migration_check": {("migration_id", "capsule_migration", "id")},
    "capsule_lineage_parent": {("event_id", "capsule_lineage_event", "event_id")},
}

V03_NULLABLE_COLUMNS = {
    ("capsule_application", "icon_asset"),
    ("capsule_application", "release_notes_doc"),
    ("capsule_instance", "icon_asset_id"),
    ("capsule_instance", "cover_asset_id"),
    ("capsule_asset", "description"),
    ("capsule_command", "argv_json"),
    ("capsule_runbook", "command_id"),
    ("capsule_endpoint_step", "required_changes"),
    ("capsule_grant", "granted_at"),
    ("capsule_lineage_parent", "parent_capsule_id"),
    ("capsule_lineage_parent", "parent_revision_id"),
}

FORMAT_CONTRACTS = {
    2: FormatContract(
        "0.2", RUNTIME_PROTOCOL, REQUIRED_TABLE_COLUMNS, REQUIRED_COLUMN_TYPES,
        REQUIRED_PRIMARY_KEYS, REQUIRED_FOREIGN_KEYS, NULLABLE_COLUMNS,
        REQUIRED_CONTENT_TABLES,
    ),
    3: FormatContract(
        "0.3", RUNTIME_PROTOCOL, V03_REQUIRED_TABLE_COLUMNS,
        V03_REQUIRED_COLUMN_TYPES, V03_REQUIRED_PRIMARY_KEYS,
        V03_REQUIRED_FOREIGN_KEYS, V03_NULLABLE_COLUMNS,
        {
            "capsule_application": "application metadata",
            "capsule_instance": "instance metadata",
            "capsule_asset": "application assets",
            "capsule_command": "launch commands",
            "capsule_runbook": "agent runbook steps",
            "capsule_doc": "embedded documentation",
            "capsule_endpoint": "named data-access endpoints",
            "capsule_check": "application validation checks",
            "capsule_prompt": "agent prompts",
            "capsule_dataset": "declared data contracts",
            "capsule_dataset_table": "classified domain tables",
        },
    ),
}
ENDPOINT_NAME_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
UUID_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)
DOCUMENT_KIND_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
MEDIA_TYPE_PATTERN = re.compile(
    r"^[A-Za-z0-9!#$&^_.+-]+/[A-Za-z0-9!#$&^_.+-]+"
    r"(?:\s*;\s*[A-Za-z0-9!#$&^_.+-]+\s*=\s*[A-Za-z0-9!#$&^_.+'-]+)*$"
)
ALLOWED_CACHE_POLICIES = {"no-store"}

CSP = "; ".join(
    [
        "default-src 'none'",
        "script-src 'self' 'wasm-unsafe-eval'",
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' data: blob:",
        "font-src 'self' data:",
        "connect-src 'self'",
        "object-src 'none'",
        "frame-src 'none'",
        "frame-ancestors 'none'",
        "base-uri 'none'",
        "form-action 'none'",
    ]
)


class CapsuleError(RuntimeError):
    """Base error for capsule operations."""


class VerificationError(CapsuleError):
    """Raised when a capsule cannot be safely launched."""


class EndpointError(CapsuleError):
    """Raised for invalid or failed named endpoint operations."""


@dataclass(frozen=True)
class Asset:
    path: str
    media_type: str
    content: bytes
    sha256: str
    executable: bool
    cache_policy: str


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _readonly_uri(path: Path) -> str:
    quoted = urllib.parse.quote(str(path.resolve()).replace("\\", "/"), safe="/:")
    return f"file:{quoted}?mode=ro"


def connect_database(path: Path, *, read_only: bool = False) -> sqlite3.Connection:
    if read_only:
        connection = sqlite3.connect(_readonly_uri(path), uri=True, timeout=5.0)
    else:
        connection = sqlite3.connect(path, timeout=5.0, check_same_thread=False)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys = ON")
    connection.execute("PRAGMA busy_timeout = 5000")
    try:
        connection.execute("PRAGMA trusted_schema = OFF")
    except sqlite3.DatabaseError:
        pass
    try:
        connection.enable_load_extension(False)
    except (AttributeError, sqlite3.DatabaseError):
        pass
    return connection


def safe_asset_path(path: str) -> bool:
    if (
        not path
        or path.startswith("/")
        or "\\" in path
        or any(ord(char) < 32 or ord(char) == 127 for char in path)
    ):
        return False
    parts = path.split("/")
    return all(part not in {"", ".", ".."} for part in parts)


def _quote_sql_identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def safe_http_header_value(value: Any) -> bool:
    return isinstance(value, str) and bool(value) and all(32 <= ord(char) <= 126 for char in value)


def safe_media_type(value: Any) -> bool:
    return safe_http_header_value(value) and bool(MEDIA_TYPE_PATTERN.fullmatch(value))


def parse_utc_timestamp(value: Any) -> bool:
    if not isinstance(value, str) or len(value) != 20:
        return False
    try:
        parsed = datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return parsed.strftime("%Y-%m-%dT%H:%M:%SZ") == value


def _bounded_text(value: Any, *, minimum: int = 0, maximum: int) -> bool:
    return (
        isinstance(value, str)
        and minimum <= len(value) <= maximum
        and len(value.encode("utf-8")) <= maximum
    )


def sql_parameters(sql_text: str) -> tuple[set[str], set[str]]:
    """Extract v0 ``:name`` parameters while ignoring quoted SQL text and comments."""

    named: set[str] = set()
    unsupported: set[str] = set()
    index = 0
    length = len(sql_text)
    while index < length:
        char = sql_text[index]
        next_char = sql_text[index + 1] if index + 1 < length else ""
        if char == "-" and next_char == "-":
            index += 2
            while index < length and sql_text[index] not in "\r\n":
                index += 1
            continue
        if char == "/" and next_char == "*":
            end = sql_text.find("*/", index + 2)
            index = length if end < 0 else end + 2
            continue
        if char in {"'", '"', "`"}:
            quote = char
            index += 1
            while index < length:
                if sql_text[index] == quote:
                    if index + 1 < length and sql_text[index + 1] == quote:
                        index += 2
                        continue
                    index += 1
                    break
                index += 1
            continue
        if char == "[":
            end = sql_text.find("]", index + 1)
            index = length if end < 0 else end + 1
            continue
        if char == ":" and (next_char.isalpha() or next_char == "_"):
            end = index + 2
            while end < length and (sql_text[end].isalnum() or sql_text[end] == "_"):
                end += 1
            named.add(sql_text[index + 1 : end])
            index = end
            continue
        if char in {"?", "@", "$"}:
            unsupported.add(char)
        index += 1
    return named, unsupported


def endpoint_probe_values(spec: Mapping[str, Any]) -> dict[str, Any]:
    values = {
        "string": "",
        "number": 0.0,
        "integer": 0,
        "boolean": 0,
        "json": "null",
    }
    return {name: values[rule["type"]] for name, rule in spec.items()}


def statement_kind(sql_text: str) -> str:
    index = 0
    while index < len(sql_text):
        while index < len(sql_text) and sql_text[index].isspace():
            index += 1
        if sql_text.startswith("--", index):
            newline = sql_text.find("\n", index + 2)
            index = len(sql_text) if newline < 0 else newline + 1
            continue
        if sql_text.startswith("/*", index):
            end = sql_text.find("*/", index + 2)
            index = len(sql_text) if end < 0 else end + 2
            continue
        break
    stripped = sql_text[index:]
    token = stripped.split(None, 1)[0].upper() if stripped else ""
    return token


def looks_like_single_statement(sql_text: str) -> bool:
    # SQLite's execute() is still the parser of record. This lexical pass rejects
    # additional statements without mistaking quoted semicolons for separators.
    index = 0
    has_content = False
    terminated = False
    while index < len(sql_text):
        char = sql_text[index]
        next_char = sql_text[index + 1] if index + 1 < len(sql_text) else ""
        if char.isspace():
            index += 1
            continue
        if char == "-" and next_char == "-":
            newline = sql_text.find("\n", index + 2)
            index = len(sql_text) if newline < 0 else newline + 1
            continue
        if char == "/" and next_char == "*":
            end = sql_text.find("*/", index + 2)
            index = len(sql_text) if end < 0 else end + 2
            continue
        if terminated:
            return False
        if char == ";":
            if not has_content:
                return False
            terminated = True
            index += 1
            continue
        has_content = True
        if char in {"'", '"', "`"}:
            quote = char
            index += 1
            while index < len(sql_text):
                if sql_text[index] == quote:
                    if index + 1 < len(sql_text) and sql_text[index + 1] == quote:
                        index += 2
                        continue
                    index += 1
                    break
                index += 1
            continue
        if char == "[":
            end = sql_text.find("]", index + 1)
            index = len(sql_text) if end < 0 else end + 1
            continue
        index += 1
    return has_content


def json_value(value: Any) -> Any:
    if isinstance(value, sqlite3.Row):
        value = dict(value)
    if isinstance(value, bytes):
        raise EndpointError("Blob values are not exposed by the generic JSON bridge")
    return value


def decode_row(row: sqlite3.Row | Mapping[str, Any]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, raw in dict(row).items():
        value = json_value(raw)
        if key.endswith("_json") and isinstance(value, str):
            try:
                value = json.loads(value)
            except json.JSONDecodeError:
                pass
        result[key] = value
    return result


def _json_dump(value: Any) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )


def _check_json_depth(value: Any, depth: int = 0) -> None:
    if depth > MAX_JSON_DEPTH:
        raise EndpointError(f"JSON value exceeds the {MAX_JSON_DEPTH}-level nesting limit")
    if isinstance(value, dict):
        for child in value.values():
            _check_json_depth(child, depth + 1)
    elif isinstance(value, list):
        for child in value:
            _check_json_depth(child, depth + 1)


def _format_human_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)


class CapsuleDatabase:
    """Generic access to one SQLite Capsule file."""

    def __init__(self, path: str | os.PathLike[str], *, read_only: bool = False) -> None:
        self.path = Path(path).expanduser().resolve()
        if not self.path.is_file():
            raise CapsuleError(f"Capsule not found: {self.path}")
        try:
            size = self.path.stat().st_size
        except OSError as exc:
            raise CapsuleError(f"Could not stat capsule: {exc}") from exc
        if size > MAX_CAPSULE_BYTES:
            raise CapsuleError(
                f"Capsule is {size} bytes; the host limit is {MAX_CAPSULE_BYTES} bytes"
            )
        self.read_only = read_only
        self._connection = connect_database(self.path, read_only=read_only)
        self._lock = threading.RLock()

    def close(self) -> None:
        with self._lock:
            self._connection.close()

    def __enter__(self) -> "CapsuleDatabase":
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        self.close()

    def manifest(self) -> dict[str, Any]:
        with self._lock:
            row = self._connection.execute("SELECT * FROM capsule_manifest WHERE id = 1").fetchone()
        if row is None:
            raise CapsuleError("Missing capsule manifest")
        return decode_row(row)

    def _overview_manifest_profile(self) -> tuple[int, dict[str, Any]]:
        """Select one exact supported identity profile without executing capsule code."""

        try:
            with self._lock:
                application_id = int(
                    self._connection.execute("PRAGMA application_id").fetchone()[0]
                )
                user_version = int(
                    self._connection.execute("PRAGMA user_version").fetchone()[0]
                )
                rows = self._connection.execute(
                    "SELECT * FROM capsule_manifest ORDER BY id LIMIT 2"
                ).fetchall()
        except sqlite3.DatabaseError as exc:
            raise CapsuleError(f"Could not select capsule identity profile: {exc}") from exc
        if application_id != APPLICATION_ID:
            raise CapsuleError(
                f"Unexpected application_id {application_id}; expected {APPLICATION_ID}"
            )
        if len(rows) != 1 or rows[0]["id"] != 1:
            raise CapsuleError("capsule_manifest must contain exactly one row with id = 1")
        profile = SUPPORTED_FORMAT_PROFILES.get(user_version)
        if profile is None:
            raise CapsuleError(f"Unsupported capsule user_version: {user_version}")
        manifest = decode_row(rows[0])
        expected_tuple = (
            profile["format_id"],
            profile["format_version"],
            profile["runtime_protocol"],
        )
        actual_tuple = (
            manifest.get("format_id"),
            manifest.get("format_version"),
            manifest.get("runtime_protocol"),
        )
        if actual_tuple != expected_tuple:
            raise CapsuleError(
                "Manifest format_id, format_version, and runtime_protocol do not match "
                f"the supported user_version {user_version} profile"
            )
        if user_version == 3:
            if manifest.get("minimum_host_profile") != profile["minimum_host_profile"]:
                raise CapsuleError("Manifest has an unsupported minimum_host_profile")
            if not parse_utc_timestamp(manifest.get("released_at")):
                raise CapsuleError(
                    "Manifest released_at must be a present exact UTC-seconds timestamp"
                )
        return user_version, manifest

    def overview(self) -> dict[str, Any]:
        """Return version-dispatched application, instance, and schema identity.

        This reads bounded metadata references only. It never decodes an icon or
        releases executable asset bytes.
        """

        user_version, manifest = self._overview_manifest_profile()
        if user_version == 2:
            return {
                "format": {"id": manifest["format_id"], "version": manifest["format_version"]},
                "application": {
                    "app_id": manifest["app_id"],
                    "app_version": manifest["app_version"],
                    "name": manifest["title"],
                    "description": manifest["summary"],
                    "icon_asset": None,
                    "legacy_fallback": True,
                },
                "instance": {
                    "capsule_id": manifest["capsule_id"],
                    "revision_id": None,
                    "title": manifest["title"],
                    "description": manifest["summary"],
                    "document_kind": "legacy-v0.2",
                    "tags": [],
                    "icon_asset_id": None,
                    "cover_asset_id": None,
                    "created_at": manifest["created_at"],
                    "content_updated_at": manifest["updated_at"],
                },
                "data_schema": None,
            }
        if user_version != 3:
            raise CapsuleError(f"Unsupported capsule user_version: {user_version}")
        application_count = int(
            self._connection.execute("SELECT count(*) FROM capsule_application").fetchone()[0]
        )
        instance_count = int(
            self._connection.execute("SELECT count(*) FROM capsule_instance").fetchone()[0]
        )
        application_rows = self._connection.execute(
            "SELECT name, description, category, icon_asset, release_notes_doc "
            "FROM capsule_application WHERE id = 1"
        ).fetchall()
        instance_rows = self._connection.execute(
            "SELECT capsule_id, revision_id, title, description, document_kind, tags_json, "
            "icon_asset_id, cover_asset_id, created_at, content_updated_at "
            "FROM capsule_instance WHERE id = 1"
        ).fetchall()
        if (
            application_count != 1
            or instance_count != 1
            or len(application_rows) != 1
            or len(instance_rows) != 1
        ):
            raise CapsuleError(
                "v0.3 application and instance identity must each contain exactly one id=1 row"
            )
        application = decode_row(application_rows[0])
        instance = decode_row(instance_rows[0])
        if not (
            _bounded_text(manifest.get("app_id"), minimum=1, maximum=512)
            and _bounded_text(manifest.get("app_version"), minimum=1, maximum=128)
            and re.fullmatch(
                r"[0-9A-Za-z][0-9A-Za-z.+_-]*", str(manifest.get("app_version"))
            )
        ):
            raise CapsuleError("v0.3 application release identity violates its bound")
        for field, minimum, maximum in (
            ("name", 1, 256),
            ("description", 0, 4096),
            ("category", 1, 128),
        ):
            if not _bounded_text(application.get(field), minimum=minimum, maximum=maximum):
                raise CapsuleError(f"v0.3 application field {field!r} violates its bound")
        for field in ("icon_asset", "release_notes_doc"):
            value = application.get(field)
            if value is not None and not _bounded_text(
                value,
                minimum=1 if field == "icon_asset" else 0,
                maximum=1024,
            ):
                raise CapsuleError(f"v0.3 application reference {field!r} violates its bound")
        if not UUID_PATTERN.fullmatch(str(instance.get("capsule_id", ""))):
            raise CapsuleError("v0.3 capsule_id is not a canonical RFC 4122 UUID")
        if not UUID_PATTERN.fullmatch(str(instance.get("revision_id", ""))):
            raise CapsuleError("v0.3 revision_id is not a canonical RFC 4122 UUID")
        for field, minimum, maximum in (
            ("title", 1, 512),
            ("description", 0, 8192),
            ("document_kind", 1, 128),
        ):
            if not _bounded_text(instance.get(field), minimum=minimum, maximum=maximum):
                raise CapsuleError(f"v0.3 instance field {field!r} violates its bound")
        if not DOCUMENT_KIND_PATTERN.fullmatch(str(instance.get("document_kind", ""))):
            raise CapsuleError("v0.3 document_kind has an unsafe shape")
        tags = instance.get("tags_json")
        if not (
            isinstance(tags, list)
            and len(tags) <= 64
            and all(_bounded_text(tag, minimum=1, maximum=128) for tag in tags)
            and len(tags) == len(set(tags))
        ):
            raise CapsuleError("v0.3 tags violate their bounded unique-array contract")
        for field in ("icon_asset_id", "cover_asset_id"):
            value = instance.get(field)
            if value is not None and not _bounded_text(value, minimum=1, maximum=256):
                raise CapsuleError(f"v0.3 instance reference {field!r} violates its bound")
        for field in ("created_at", "content_updated_at"):
            if not parse_utc_timestamp(instance.get(field)):
                raise CapsuleError(f"v0.3 instance timestamp {field!r} is invalid")
        if not _bounded_text(manifest.get("data_schema_id"), minimum=1, maximum=512):
            raise CapsuleError("v0.3 data_schema_id violates its bound")
        if not isinstance(manifest.get("data_schema_version"), int) or manifest[
            "data_schema_version"
        ] < 1:
            raise CapsuleError("v0.3 data_schema_version must be a positive integer")
        return {
            "format": {"id": manifest["format_id"], "version": manifest["format_version"]},
            "application": {
                "app_id": manifest["app_id"],
                "app_version": manifest["app_version"],
                **application,
                "legacy_fallback": False,
            },
            "instance": instance,
            "data_schema": {
                "id": manifest["data_schema_id"],
                "version": manifest["data_schema_version"],
            },
        }

    def permissions(self) -> dict[str, Any]:
        manifest = self.manifest()
        requested = manifest.get("permissions_json")
        if not isinstance(requested, dict):
            raise CapsuleError("Manifest permissions_json must decode to an object")
        grants: dict[str, dict[str, Any]] = {}
        with self._lock:
            has_grants = bool(
                self._connection.execute(
                    "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'capsule_grant'"
                ).fetchone()
            )
            if has_grants:
                rows = self._connection.execute(
                    "SELECT capability, decision, reason, granted_at "
                    "FROM capsule_grant ORDER BY capability"
                ).fetchall()
                grants = {str(row["capability"]): decode_row(row) for row in rows}
        effective = {
            capability: grants.get(capability, {"capability": capability, "decision": "prompt"})
            for capability in requested
        }
        return {"requested": requested, "grants": grants, "effective": effective}

    def runbooks(self, audience: str | None = None) -> list[dict[str, Any]]:
        sql = "SELECT * FROM capsule_runbook"
        params: tuple[Any, ...] = ()
        if audience:
            sql += " WHERE audience IN (?, 'all')"
            params = (audience,)
        sql += " ORDER BY sequence, id"
        with self._lock:
            rows = self._connection.execute(sql, params).fetchall()
        return [decode_row(row) for row in rows]

    def commands(self) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._connection.execute("SELECT * FROM capsule_command ORDER BY id").fetchall()
        return [decode_row(row) for row in rows]

    def prompts(self) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._connection.execute(
                "SELECT * FROM capsule_prompt ORDER BY sequence, id"
            ).fetchall()
        return [decode_row(row) for row in rows]

    def documents(self) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._connection.execute(
                "SELECT slug, title, media_type, content, sequence "
                "FROM capsule_doc ORDER BY sequence, slug"
            ).fetchall()
        return [decode_row(row) for row in rows]

    def list_assets(self) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._connection.execute(
                "SELECT path, media_type, length(content) AS bytes, sha256, executable, "
                "cache_policy, description FROM capsule_asset ORDER BY path"
            ).fetchall()
        return [decode_row(row) for row in rows]

    def asset(self, path: str) -> Asset | None:
        if not safe_asset_path(path):
            return None
        with self._lock:
            row = self._connection.execute(
                "SELECT path, media_type, content, sha256, executable, cache_policy "
                "FROM capsule_asset WHERE path = ?",
                (path,),
            ).fetchone()
        if row is None:
            return None
        content = bytes(row["content"])
        actual = hashlib.sha256(content).hexdigest()
        if actual != row["sha256"]:
            raise VerificationError(f"Asset hash mismatch: {path}")
        if not safe_media_type(row["media_type"]):
            raise VerificationError(f"Unsafe asset media type: {path}")
        if row["cache_policy"] not in ALLOWED_CACHE_POLICIES:
            raise VerificationError(f"Unsafe asset cache policy: {path}")
        return Asset(
            path=row["path"],
            media_type=row["media_type"],
            content=content,
            sha256=row["sha256"],
            executable=bool(row["executable"]),
            cache_policy=row["cache_policy"],
        )

    def extract_asset(self, asset_path: str, output_path: Path, *, replace: bool = False) -> Path:
        asset = self.asset(asset_path)
        if asset is None:
            raise CapsuleError(f"Asset not found: {asset_path}")
        output_path = output_path.expanduser().resolve()
        if output_path.exists():
            if output_path.is_file() and output_path.read_bytes() == asset.content:
                return output_path
            if not replace:
                raise CapsuleError(f"Refusing to replace unrelated existing file: {output_path}")
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(asset.content)
        return output_path

    def _objects(self, kind: str) -> set[str]:
        with self._lock:
            rows = self._connection.execute(
                "SELECT name FROM sqlite_master WHERE type = ?", (kind,)
            ).fetchall()
        return {str(row["name"]) for row in rows}

    def verify(self) -> dict[str, Any]:
        errors: list[str] = []
        warnings: list[str] = []
        check_results: list[dict[str, Any]] = []
        user_version: int | None = None
        format_profile: Mapping[str, str] | None = None
        format_contract: FormatContract | None = None

        try:
            application_id = int(self._connection.execute("PRAGMA application_id").fetchone()[0])
            if application_id != APPLICATION_ID:
                errors.append(
                    f"Unexpected application_id {application_id}; expected {APPLICATION_ID}"
                )
        except sqlite3.DatabaseError as exc:
            errors.append(f"Could not read application_id: {exc}")

        try:
            user_version = int(self._connection.execute("PRAGMA user_version").fetchone()[0])
            format_profile = SUPPORTED_FORMAT_PROFILES.get(user_version)
            format_contract = FORMAT_CONTRACTS.get(user_version)
            if format_profile is None or format_contract is None:
                errors.append(
                    f"Unsupported user_version {user_version}; expected one of {sorted(FORMAT_CONTRACTS)}"
                )
        except sqlite3.DatabaseError as exc:
            errors.append(f"Could not read user_version: {exc}")

        try:
            integrity_rows = self._connection.execute("PRAGMA integrity_check").fetchall()
            integrity_messages = [str(row[0]) for row in integrity_rows]
            if integrity_messages != ["ok"]:
                errors.append(
                    "SQLite integrity_check failed: " + "; ".join(integrity_messages[:10])
                )
        except sqlite3.DatabaseError as exc:
            errors.append(f"SQLite integrity_check could not run: {exc}")

        try:
            fk_rows = self._connection.execute("PRAGMA foreign_key_check").fetchall()
            if fk_rows:
                errors.append(f"Foreign-key violations: {len(fk_rows)}")
        except sqlite3.DatabaseError as exc:
            errors.append(f"Foreign-key check could not run: {exc}")

        tables = self._objects("table")
        views = self._objects("view")
        required_table_columns = (
            format_contract.required_table_columns if format_contract else {}
        )
        required_tables = set(required_table_columns)
        missing_tables = sorted(required_tables - tables)
        missing_views = sorted(REQUIRED_VIEWS - views)
        if missing_tables:
            errors.append(f"Missing platform tables: {', '.join(missing_tables)}")
        if missing_views:
            errors.append(f"Missing platform views: {', '.join(missing_views)}")
        if user_version == 3:
            allowed_platform_objects = required_tables | set(OPTIONAL_TABLE_COLUMNS)
            platform_objects = {
                str(row["name"])
                for row in self._connection.execute(
                    "SELECT name FROM sqlite_schema WHERE name GLOB 'capsule_*'"
                ).fetchall()
            }
            unexpected_platform_objects = sorted(platform_objects - allowed_platform_objects)
            if unexpected_platform_objects:
                errors.append(
                    "Unknown v0.3 platform objects: " + ", ".join(unexpected_platform_objects)
                )
        triggers = self._objects("trigger")
        if triggers:
            errors.append(
                "Triggers are not permitted by the supported endpoint contracts: "
                + ", ".join(sorted(triggers))
            )
        try:
            virtual_tables = sorted(
                str(row["name"])
                for row in self._connection.execute("PRAGMA table_list").fetchall()
                if str(row["type"]).casefold() == "virtual"
            )
        except sqlite3.DatabaseError as exc:
            errors.append(f"Could not inspect virtual-table declarations: {exc}")
        else:
            if virtual_tables:
                errors.append(
                    "Virtual tables are not permitted by the portable capsule contract: "
                    + ", ".join(virtual_tables)
                )

        for table in sorted(required_tables & tables):
            try:
                column_rows = self._connection.execute(
                    f"PRAGMA table_xinfo({_quote_sql_identifier(table)})"
                ).fetchall()
                actual_columns = {str(row["name"]) for row in column_rows}
                missing_columns = sorted(required_table_columns[table] - actual_columns)
                if missing_columns:
                    errors.append(
                        f"Platform table {table!r} is missing columns: "
                        + ", ".join(missing_columns)
                    )
                if user_version == 3 and actual_columns != required_table_columns[table]:
                    errors.append(f"Platform table {table!r} has non-contract columns")
                rows_by_name = {str(row["name"]): row for row in column_rows}
                required_column_types = (
                    format_contract.required_column_types if format_contract else {}
                )
                nullable_columns = format_contract.nullable_columns if format_contract else set()
                for column, expected_type in required_column_types[table].items():
                    column_row = rows_by_name.get(column)
                    if column_row is None:
                        continue
                    actual_type = str(column_row["type"]).upper()
                    if actual_type != expected_type:
                        errors.append(
                            f"Platform column {table}.{column} has type {actual_type!r}; "
                            f"expected {expected_type!r}"
                        )
                    integer_primary_key = expected_type == "INTEGER" and int(column_row["pk"]) > 0
                    if (
                        (table, column) not in nullable_columns
                        and not integer_primary_key
                        and not bool(column_row["notnull"])
                    ):
                        errors.append(f"Platform column {table}.{column} must be NOT NULL")
                actual_primary_key = tuple(
                    str(row["name"])
                    for row in sorted(column_rows, key=lambda item: int(item["pk"]) or 1_000_000)
                    if int(row["pk"])
                )
                required_primary_keys = (
                    format_contract.required_primary_keys if format_contract else {}
                )
                if actual_primary_key != required_primary_keys[table]:
                    errors.append(
                        f"Platform table {table!r} has incompatible primary key: "
                        + ", ".join(actual_primary_key)
                    )
                required_foreign_keys = (
                    format_contract.required_foreign_keys.get(table, set())
                    if format_contract
                    else set()
                )
                if required_foreign_keys:
                    foreign_key_rows = self._connection.execute(
                        f"PRAGMA foreign_key_list({_quote_sql_identifier(table)})"
                    ).fetchall()
                    actual_foreign_keys = {
                        (str(row["from"]), str(row["table"]), str(row["to"]))
                        for row in foreign_key_rows
                    }
                    missing_foreign_keys = sorted(required_foreign_keys - actual_foreign_keys)
                    if missing_foreign_keys:
                        errors.append(
                            f"Platform table {table!r} is missing required foreign keys: "
                            + repr(missing_foreign_keys)
                        )
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not inspect platform table {table!r}: {exc}")

        for table, expected_types in OPTIONAL_TABLE_COLUMNS.items():
            if table not in tables:
                continue
            try:
                column_rows = self._connection.execute(
                    f"PRAGMA table_xinfo({_quote_sql_identifier(table)})"
                ).fetchall()
                actual_columns = {str(row["name"]): row for row in column_rows}
                if user_version == 3 and set(actual_columns) != set(expected_types):
                    errors.append(f"Optional platform table {table!r} has non-contract columns")
                for column, expected_type in expected_types.items():
                    row = actual_columns.get(column)
                    if row is None:
                        errors.append(f"Optional platform table {table!r} is missing {column!r}")
                        continue
                    if str(row["type"]).upper() != expected_type:
                        errors.append(f"Optional platform column {table}.{column} has an incompatible type")
                    integer_primary_key = expected_type == "INTEGER" and int(row["pk"]) > 0
                    if not bool(row["notnull"]) and not integer_primary_key:
                        errors.append(f"Optional platform column {table}.{column} must be NOT NULL")
                actual_primary_key = tuple(
                    str(row["name"])
                    for row in sorted(column_rows, key=lambda item: int(item["pk"]) or 1_000_000)
                    if int(row["pk"])
                )
                if actual_primary_key != OPTIONAL_PRIMARY_KEYS[table]:
                    errors.append(f"Optional platform table {table!r} has an incompatible primary key")
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not inspect optional platform table {table!r}: {exc}")

        required_content_tables = (
            format_contract.required_content_tables if format_contract else {}
        )
        for table, purpose in required_content_tables.items():
            if table not in tables:
                continue
            try:
                count = int(
                    self._connection.execute(
                        f"SELECT count(*) FROM {_quote_sql_identifier(table)}"
                    ).fetchone()[0]
                )
                if count == 0:
                    errors.append(f"Capsule must contain {purpose}; {table!r} is empty")
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not inspect required content in {table!r}: {exc}")

        if "START_HERE" in views:
            try:
                cursor = self._connection.execute("SELECT * FROM START_HERE LIMIT 1")
                actual_columns = tuple(item[0] for item in cursor.description or ())
                first_row = cursor.fetchone()
                if actual_columns != START_HERE_COLUMNS:
                    errors.append(
                        "START_HERE has incompatible columns: " + ", ".join(actual_columns)
                    )
                if first_row is None:
                    errors.append("START_HERE must expose at least one agent runbook step")
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not inspect START_HERE: {exc}")

        if "capsule_command" in tables:
            try:
                command_rows = self._connection.execute(
                    "SELECT id, argv_json FROM capsule_command ORDER BY id"
                ).fetchall()
                for row in command_rows:
                    if row["argv_json"] is None:
                        continue
                    try:
                        arguments = json.loads(row["argv_json"])
                    except json.JSONDecodeError as exc:
                        errors.append(f"Command {row['id']!r} has invalid argv_json: {exc}")
                        continue
                    if arguments is None:
                        continue
                    if not (
                        isinstance(arguments, list)
                        and arguments
                        and all(isinstance(item, str) and item for item in arguments)
                    ):
                        errors.append(
                            f"Command {row['id']!r} argv_json must be null or a non-empty string array"
                        )
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not inspect structured command arguments: {exc}")

        manifest: dict[str, Any] | None = None
        if "capsule_manifest" in tables:
            try:
                count = int(
                    self._connection.execute("SELECT count(*) FROM capsule_manifest").fetchone()[0]
                )
                if count != 1:
                    errors.append(f"Manifest must contain exactly one row; found {count}")
                else:
                    manifest = self.manifest()
                    if manifest["format_id"] != FORMAT_ID:
                        errors.append(f"Unsupported format_id: {manifest['format_id']}")
                    expected_format = (
                        str(format_profile["format_version"]) if format_profile else None
                    )
                    expected_protocol = (
                        str(format_profile["runtime_protocol"]) if format_profile else None
                    )
                    if manifest["format_version"] != expected_format:
                        errors.append(
                            "Format identity mismatch: "
                            f"user_version {user_version} requires format_version "
                            f"{expected_format!r}, got {manifest['format_version']!r}"
                        )
                    if manifest["runtime_protocol"] != expected_protocol:
                        errors.append(
                            "Runtime identity mismatch: "
                            f"user_version {user_version} requires runtime_protocol "
                            f"{expected_protocol!r}, got {manifest['runtime_protocol']!r}"
                        )
                    permissions = manifest.get("permissions_json")
                    if not isinstance(permissions, dict):
                        errors.append("Manifest permissions_json must decode to an object")
                    else:
                        enabled_operations = {
                            str(row[0])
                            for row in self._connection.execute(
                                "SELECT DISTINCT operation FROM capsule_endpoint WHERE enabled = 1"
                            ).fetchall()
                        }
                        for operation, capability in (
                            ("read", "database.read"),
                            ("write", "database.write"),
                        ):
                            declaration = permissions.get(capability)
                            if operation in enabled_operations and not (
                                isinstance(declaration, dict)
                                and declaration.get("required") is True
                            ):
                                errors.append(
                                    f"Enabled {operation} endpoints require permission {capability!r}"
                                )
                        network = permissions.get("network")
                        if not isinstance(network, dict) or network.get("value") != "none":
                            errors.append(
                                "The bootstrap host requires permissions_json network.value to be 'none'"
                            )
                    common_fields = ("app_id", "app_version", "entry_asset")
                    legacy_fields = ("capsule_id", "title", "summary") if user_version == 2 else ()
                    for field in common_fields + legacy_fields:
                        if not isinstance(manifest.get(field), str) or not manifest[field].strip():
                            errors.append(f"Manifest field {field!r} must be a non-empty string")
                    timestamp_fields = (
                        ("created_at", "updated_at")
                        if user_version == 2
                        else ("released_at",)
                    )
                    for field in timestamp_fields:
                        if not parse_utc_timestamp(manifest.get(field)):
                            errors.append(
                                f"Manifest field {field!r} must be an exact UTC-seconds timestamp"
                            )
                    if user_version == 3:
                        if not _bounded_text(manifest.get("app_id"), minimum=1, maximum=512):
                            errors.append("Manifest app_id is invalid or exceeds 512 UTF-8 bytes")
                        app_version = manifest.get("app_version")
                        if not (
                            _bounded_text(app_version, minimum=1, maximum=128)
                            and re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z.+_-]*", str(app_version))
                        ):
                            errors.append("Manifest app_version violates its bounded version shape")
                        if manifest.get("minimum_host_profile") != "org.sqlite-capsule.host-profile/0.3":
                            errors.append("Manifest has an unsupported minimum_host_profile")
                        if not isinstance(manifest.get("data_schema_version"), int) or manifest["data_schema_version"] < 1:
                            errors.append("Manifest data_schema_version must be a positive integer")
                        if not _bounded_text(manifest.get("data_schema_id"), minimum=1, maximum=512):
                            errors.append("Manifest data_schema_id is invalid or exceeds 512 UTF-8 bytes")
            except (sqlite3.DatabaseError, CapsuleError, KeyError) as exc:
                errors.append(f"Invalid manifest: {exc}")

        overview: dict[str, Any] | None = None
        if user_version == 3 and {"capsule_application", "capsule_instance"} <= tables:
            try:
                overview = self.overview()
                application = overview["application"]
                instance = overview["instance"]
                for field, minimum, maximum in (
                    ("name", 1, 256), ("description", 0, 4096), ("category", 1, 128),
                ):
                    if not _bounded_text(application.get(field), minimum=minimum, maximum=maximum):
                        errors.append(f"Application field {field!r} violates its metadata bound")
                for field in ("icon_asset", "release_notes_doc"):
                    value = application.get(field)
                    if value is not None and not _bounded_text(
                        value,
                        minimum=1 if field == "icon_asset" else 0,
                        maximum=1024,
                    ):
                        errors.append(f"Application field {field!r} violates its reference bound")
                if not UUID_PATTERN.fullmatch(str(instance.get("capsule_id", ""))):
                    errors.append("Instance capsule_id is not a canonical RFC 4122 UUID")
                if not UUID_PATTERN.fullmatch(str(instance.get("revision_id", ""))):
                    errors.append("Instance revision_id is not a canonical RFC 4122 UUID")
                for field, minimum, maximum in (
                    ("title", 1, 512), ("description", 0, 8192), ("document_kind", 1, 128),
                ):
                    if not _bounded_text(instance.get(field), minimum=minimum, maximum=maximum):
                        errors.append(f"Instance field {field!r} violates its metadata bound")
                for field in ("icon_asset_id", "cover_asset_id"):
                    value = instance.get(field)
                    if value is not None and not _bounded_text(value, minimum=1, maximum=256):
                        errors.append(f"Instance field {field!r} violates its reference bound")
                if not DOCUMENT_KIND_PATTERN.fullmatch(str(instance.get("document_kind", ""))):
                    errors.append("Instance document_kind has an unsafe shape")
                tags = instance.get("tags_json")
                if not (
                    isinstance(tags, list)
                    and len(tags) <= 64
                    and len(tags) == len(set(tags))
                    and all(_bounded_text(tag, minimum=1, maximum=128) for tag in tags)
                ):
                    errors.append("Instance tags_json must contain at most 64 unique bounded strings")
                for field in ("created_at", "content_updated_at"):
                    if not parse_utc_timestamp(instance.get(field)):
                        errors.append(f"Instance field {field!r} must be an exact UTC-seconds timestamp")
            except (CapsuleError, KeyError, TypeError, ValueError, sqlite3.DatabaseError) as exc:
                errors.append(f"Invalid v0.3 identity metadata: {exc}")

        if "capsule_asset" in tables:
            seen_casefold: dict[str, str] = {}
            try:
                rows = self._connection.execute(
                    "SELECT path, media_type, content, sha256, cache_policy "
                    "FROM capsule_asset ORDER BY path"
                ).fetchall()
                for row in rows:
                    path = str(row["path"])
                    if not safe_asset_path(path):
                        errors.append(f"Unsafe asset path: {path!r}")
                    folded = path.casefold()
                    prior = seen_casefold.get(folded)
                    if prior and prior != path:
                        errors.append(f"Case-insensitive asset collision: {prior!r} and {path!r}")
                    seen_casefold[folded] = path
                    content = bytes(row["content"])
                    if len(content) > MAX_ASSET_BYTES:
                        errors.append(
                            f"Asset {path!r} is {len(content)} bytes; the host limit is {MAX_ASSET_BYTES}"
                        )
                    actual = hashlib.sha256(content).hexdigest()
                    if actual != row["sha256"]:
                        errors.append(f"Asset hash mismatch: {path}")
                    if not safe_media_type(row["media_type"]):
                        errors.append(f"Asset {path!r} has an unsafe media type")
                    if row["cache_policy"] not in ALLOWED_CACHE_POLICIES:
                        errors.append(f"Asset {path!r} has an unsafe cache policy")
                if manifest and manifest.get("entry_asset") not in {
                    str(row["path"]) for row in rows
                }:
                    errors.append(f"Entry asset missing: {manifest.get('entry_asset')}")
                if user_version == 3 and overview:
                    icon_asset = overview["application"].get("icon_asset")
                    if icon_asset is not None:
                        icon_row = next(
                            (row for row in rows if str(row["path"]) == icon_asset),
                            None,
                        )
                        if icon_row is None:
                            errors.append("Application icon_asset does not resolve to capsule_asset")
                        elif (
                            str(icon_row["media_type"]) not in {"image/png", "image/webp"}
                            or len(bytes(icon_row["content"])) > 524_288
                        ):
                            errors.append("Application icon must be bounded PNG or WebP content")
                if "bootstrap/capsule_host.py" not in {
                    str(row["path"]) for row in rows
                }:
                    warnings.append("No embedded standalone host asset")
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not verify assets: {exc}")

        if user_version == 3 and "capsule_instance_asset" in tables:
            try:
                instance_assets = self._connection.execute(
                    "SELECT id, media_type, content, sha256, width, height "
                    "FROM capsule_instance_asset ORDER BY id"
                ).fetchall()
                for row in instance_assets:
                    content = bytes(row["content"])
                    if (
                        row["media_type"] not in {"image/png", "image/webp"}
                        or len(content) > 524_288
                        or not isinstance(row["width"], int)
                        or not 1 <= row["width"] <= 1024
                        or not isinstance(row["height"], int)
                        or not 1 <= row["height"] <= 1024
                        or row["sha256"] != hashlib.sha256(content).hexdigest()
                    ):
                        errors.append(
                            f"Instance asset {row['id']!r} violates bounded media policy"
                        )
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not verify instance assets: {exc}")

        if "capsule_endpoint" in tables:
            try:
                steps_by_endpoint: dict[str, list[sqlite3.Row]] = {}
                if "capsule_endpoint_step" in tables:
                    step_rows = self._connection.execute(
                        "SELECT endpoint_name, sequence, sql_text, required_changes "
                        "FROM capsule_endpoint_step ORDER BY endpoint_name, sequence"
                    ).fetchall()
                    for step in step_rows:
                        steps_by_endpoint.setdefault(str(step["endpoint_name"]), []).append(step)
                endpoint_rows = self._connection.execute(
                    "SELECT * FROM capsule_endpoint ORDER BY name"
                ).fetchall()
                for row in endpoint_rows:
                    name = str(row["name"])
                    operation = str(row["operation"])
                    sql_text = str(row["sql_text"])
                    if not ENDPOINT_NAME_PATTERN.fullmatch(name):
                        errors.append(f"Endpoint {name!r} has an unsafe or unstable name")
                    try:
                        spec = json.loads(row["parameters_json"])
                        _validate_parameter_spec(spec)
                    except (json.JSONDecodeError, EndpointError) as exc:
                        errors.append(f"Endpoint {name!r} has invalid parameter schema: {exc}")
                        continue

                    compound_steps = steps_by_endpoint.get(name, [])
                    statements: list[tuple[str, str]]
                    if compound_steps:
                        sequences = [int(step["sequence"]) for step in compound_steps]
                        if not 2 <= len(compound_steps) <= MAX_ENDPOINT_STEPS:
                            errors.append(
                                f"Endpoint {name!r} must declare between 2 and "
                                f"{MAX_ENDPOINT_STEPS} compound steps"
                            )
                        if sequences != list(range(1, len(compound_steps) + 1)):
                            errors.append(
                                f"Endpoint {name!r} compound step sequence is not contiguous and one-based"
                            )
                        if operation != "write":
                            errors.append(f"Compound endpoint {name!r} must be a write endpoint")
                        if row["result_mode"] != "changes":
                            errors.append(
                                f"Compound endpoint {name!r} must use result_mode 'changes'"
                            )
                        if str(compound_steps[0]["sql_text"]) != sql_text:
                            errors.append(
                                f"Endpoint {name!r} sql_text must equal compound step 1"
                            )
                        statements = [
                            (f"step {int(step['sequence'])}", str(step["sql_text"]))
                            for step in compound_steps
                        ]
                    else:
                        statements = [("statement", sql_text)]

                    declared_parameters = set(spec)
                    used_parameters: set[str] = set()
                    unsupported_parameters: set[str] = set()
                    for label, statement in statements:
                        if not looks_like_single_statement(statement):
                            errors.append(f"Endpoint {name!r} {label} is not one SQL statement")
                        kind = statement_kind(statement)
                        if operation == "read" and kind not in {"SELECT", "WITH"}:
                            errors.append(
                                f"Read endpoint {name!r} {label} begins with disallowed {kind!r}"
                            )
                        if operation == "write" and kind not in {
                            "INSERT",
                            "UPDATE",
                            "DELETE",
                            "REPLACE",
                            "WITH",
                        }:
                            errors.append(
                                f"Write endpoint {name!r} {label} begins with disallowed {kind!r}"
                            )
                        statement_parameters, statement_unsupported = sql_parameters(statement)
                        used_parameters.update(statement_parameters)
                        unsupported_parameters.update(statement_unsupported)
                        try:
                            self._connection.set_authorizer(_authorizer_for(operation))
                            self._connection.execute(
                                "EXPLAIN " + statement, endpoint_probe_values(spec)
                            ).fetchall()
                        except sqlite3.DatabaseError as exc:
                            errors.append(f"Endpoint {name!r} {label} does not compile: {exc}")
                        finally:
                            self._connection.set_authorizer(None)

                    if unsupported_parameters:
                        errors.append(
                            f"Endpoint {name!r} uses unsupported parameter markers: "
                            + ", ".join(sorted(unsupported_parameters))
                        )
                    if declared_parameters != used_parameters:
                        missing = sorted(used_parameters - declared_parameters)
                        unused = sorted(declared_parameters - used_parameters)
                        if missing:
                            errors.append(
                                f"Endpoint {name!r} uses undeclared parameters: "
                                + ", ".join(missing)
                            )
                        if unused:
                            errors.append(
                                f"Endpoint {name!r} declares unused parameters: "
                                + ", ".join(unused)
                            )
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not verify endpoints: {exc}")

        if "capsule_check" in tables:
            try:
                check_rows = self._connection.execute(
                    "SELECT * FROM capsule_check ORDER BY id"
                ).fetchall()
                for row in check_rows:
                    result = self._run_declared_check(row)
                    check_results.append(result)
                    if not result["passed"]:
                        message = f"Check {row['id']!r} failed: {result['detail']}"
                        if row["severity"] == "error":
                            errors.append(message)
                        else:
                            warnings.append(message)
            except sqlite3.DatabaseError as exc:
                errors.append(f"Could not run declared checks: {exc}")

        return {
            "ok": not errors,
            "capsule": str(self.path),
            "manifest": manifest,
            "overview": overview if user_version == 3 else (
                self.overview() if manifest is not None and user_version == 2 else None
            ),
            "errors": errors,
            "warnings": warnings,
            "checks": check_results,
        }

    def _run_declared_check(self, row: sqlite3.Row) -> dict[str, Any]:
        sql_text = str(row["sql_text"])
        if not looks_like_single_statement(sql_text):
            return {"id": row["id"], "passed": False, "detail": "not one statement"}
        if statement_kind(sql_text) not in {"SELECT", "WITH"}:
            return {"id": row["id"], "passed": False, "detail": "not read-only SQL"}

        expected = json.loads(row["expected_json"])
        deadline = time.monotonic() + ENDPOINT_TIMEOUT_SECONDS
        self._connection.set_progress_handler(
            lambda: 1 if time.monotonic() > deadline else 0, 1_000
        )
        try:
            self._connection.execute("PRAGMA query_only = ON")
            self._connection.set_authorizer(_authorizer_for("read"))
            cursor = self._connection.execute(sql_text)
            mode = row["result_mode"]
            if mode == "scalar":
                actual = encode_cursor_result(cursor, "scalar")
                passed = actual == expected
            else:
                actual_rows = encode_cursor_result(cursor, "rows")
                if mode == "empty":
                    actual = actual_rows
                    passed = len(actual_rows) == 0
                else:
                    actual = actual_rows
                    passed = actual_rows == expected
            return {
                "id": row["id"],
                "passed": passed,
                "detail": "ok" if passed else f"expected {expected!r}, got {actual!r}",
            }
        except sqlite3.DatabaseError as exc:
            return {"id": row["id"], "passed": False, "detail": str(exc)}
        finally:
            self._connection.set_authorizer(None)
            self._connection.set_progress_handler(None, 0)
            try:
                self._connection.execute("PRAGMA query_only = OFF")
            except sqlite3.DatabaseError:
                pass

    def execute_endpoint(
        self, name: str, operation: str, parameters: Mapping[str, Any]
    ) -> Any:
        if self.read_only and operation == "write":
            raise EndpointError("Capsule is open read-only")
        with self._lock:
            endpoint = self._connection.execute(
                "SELECT * FROM capsule_endpoint WHERE name = ? AND enabled = 1", (name,)
            ).fetchone()
            if endpoint is None:
                raise EndpointError(f"Unknown or disabled endpoint: {name}")
            if endpoint["operation"] != operation:
                raise EndpointError(
                    f"Endpoint {name!r} is {endpoint['operation']!r}, not {operation!r}"
                )
            spec = json.loads(endpoint["parameters_json"])
            _validate_parameter_spec(spec)
            bound = validate_parameters(spec, parameters)
            sql_text = str(endpoint["sql_text"])
            step_rows: list[sqlite3.Row] = []
            step_rows = self._connection.execute(
                "SELECT endpoint_name, sequence, sql_text, required_changes "
                "FROM capsule_endpoint_step WHERE endpoint_name = ? ORDER BY sequence",
                (name,),
            ).fetchall()

            statements = [str(step["sql_text"]) for step in step_rows] or [sql_text]
            used_parameters: set[str] = set()
            unsupported_parameters: set[str] = set()
            for statement in statements:
                if not looks_like_single_statement(statement):
                    raise EndpointError("Endpoint contains multiple statements in one step")
                used, unsupported = sql_parameters(statement)
                used_parameters.update(used)
                unsupported_parameters.update(unsupported)
            if unsupported_parameters:
                raise EndpointError("Endpoint contains unsupported parameter markers")
            if set(spec) != used_parameters:
                raise EndpointError("Endpoint parameters do not match SQL placeholders")

            if step_rows:
                sequences = [int(step["sequence"]) for step in step_rows]
                if operation != "write":
                    raise EndpointError("Compound endpoints must be write operations")
                if endpoint["result_mode"] != "changes":
                    raise EndpointError("Compound endpoints must use changes result mode")
                if not 2 <= len(step_rows) <= MAX_ENDPOINT_STEPS:
                    raise EndpointError("Compound endpoint has an invalid step count")
                if sequences != list(range(1, len(step_rows) + 1)):
                    raise EndpointError("Compound endpoint steps are not contiguous")
                if str(step_rows[0]["sql_text"]) != sql_text:
                    raise EndpointError("Compound endpoint step 1 differs from sql_text")
                return self._execute_compound_write_endpoint(endpoint, step_rows, bound)

            if operation == "read":
                return self._execute_read_endpoint(endpoint, sql_text, bound)
            return self._execute_write_endpoint(endpoint, sql_text, bound)

    def _execute_read_endpoint(
        self, endpoint: sqlite3.Row, sql_text: str, bound: Mapping[str, Any]
    ) -> Any:
        deadline = time.monotonic() + ENDPOINT_TIMEOUT_SECONDS
        self._connection.set_progress_handler(
            lambda: 1 if time.monotonic() > deadline else 0, 1_000
        )
        try:
            # Enable SQLite's own read-only guard before installing the endpoint
            # authorizer. The authorizer intentionally rejects PRAGMA statements.
            self._connection.execute("PRAGMA query_only = ON")
            self._connection.set_authorizer(_authorizer_for("read"))
            cursor = self._connection.execute(sql_text, bound)
            return encode_cursor_result(cursor, endpoint["result_mode"])
        except sqlite3.DatabaseError as exc:
            raise EndpointError(f"Read endpoint {endpoint['name']!r} failed: {exc}") from exc
        finally:
            self._connection.set_authorizer(None)
            self._connection.set_progress_handler(None, 0)
            try:
                self._connection.execute("PRAGMA query_only = OFF")
            except sqlite3.DatabaseError:
                pass

    def _execute_write_endpoint(
        self, endpoint: sqlite3.Row, sql_text: str, bound: Mapping[str, Any]
    ) -> Any:
        deadline = time.monotonic() + ENDPOINT_TIMEOUT_SECONDS
        self._connection.set_progress_handler(
            lambda: 1 if time.monotonic() > deadline else 0, 1_000
        )
        try:
            self._connection.execute("BEGIN IMMEDIATE")
            self._connection.set_authorizer(_authorizer_for("write"))
            cursor = self._connection.execute(sql_text, bound)
            result = encode_cursor_result(cursor, endpoint["result_mode"], drain=True)
            changed_rows = int(self._connection.execute("SELECT changes()").fetchone()[0])
            self._connection.set_authorizer(None)
            self._connection.execute(
                "INSERT INTO capsule_change_log "
                "(endpoint_name, parameters_json, changed_rows, occurred_at) "
                "VALUES (?, ?, ?, ?)",
                (endpoint["name"], _json_dump(dict(bound)), changed_rows, utc_now()),
            )
            self._advance_v03_revision_if_needed()
            self._connection.commit()
            if isinstance(result, dict) and endpoint["result_mode"] == "changes":
                result["changes"] = changed_rows
            return result
        except (sqlite3.DatabaseError, EndpointError) as exc:
            self._connection.rollback()
            if isinstance(exc, EndpointError):
                raise
            raise EndpointError(f"Write endpoint {endpoint['name']!r} failed: {exc}") from exc
        finally:
            self._connection.set_authorizer(None)
            self._connection.set_progress_handler(None, 0)

    def _execute_compound_write_endpoint(
        self,
        endpoint: sqlite3.Row,
        steps: list[sqlite3.Row],
        bound: Mapping[str, Any],
    ) -> dict[str, Any]:
        deadline = time.monotonic() + ENDPOINT_TIMEOUT_SECONDS
        self._connection.set_progress_handler(
            lambda: 1 if time.monotonic() > deadline else 0, 1_000
        )
        try:
            self._connection.execute("BEGIN IMMEDIATE")
            self._connection.set_authorizer(_authorizer_for("write"))
            step_changes: list[int] = []
            lastrowid: int | None = None
            for step in steps:
                cursor = self._connection.execute(str(step["sql_text"]), bound)
                _drain_cursor(cursor)
                changed_rows = int(self._connection.execute("SELECT changes()").fetchone()[0])
                required_changes = step["required_changes"]
                if required_changes is not None and changed_rows != int(required_changes):
                    raise EndpointError(
                        f"Compound endpoint {endpoint['name']!r} step {step['sequence']} "
                        f"changed {changed_rows} rows; expected {required_changes}"
                    )
                step_changes.append(changed_rows)
                lastrowid = cursor.lastrowid
            total_changes = sum(step_changes)
            self._connection.set_authorizer(None)
            self._connection.execute(
                "INSERT INTO capsule_change_log "
                "(endpoint_name, parameters_json, changed_rows, occurred_at) "
                "VALUES (?, ?, ?, ?)",
                (endpoint["name"], _json_dump(dict(bound)), total_changes, utc_now()),
            )
            self._advance_v03_revision_if_needed()
            self._connection.commit()
            return {
                "changes": total_changes,
                "step_changes": step_changes,
                "lastrowid": lastrowid,
            }
        except (sqlite3.DatabaseError, EndpointError) as exc:
            self._connection.rollback()
            if isinstance(exc, EndpointError):
                raise
            raise EndpointError(
                f"Compound write endpoint {endpoint['name']!r} failed: {exc}"
            ) from exc
        finally:
            self._connection.set_authorizer(None)
            self._connection.set_progress_handler(None, 0)

    def _advance_v03_revision_if_needed(self) -> None:
        user_version = int(self._connection.execute("PRAGMA user_version").fetchone()[0])
        if user_version != 3:
            return
        self._connection.execute(
            "UPDATE capsule_instance SET revision_id = ?, content_updated_at = ? WHERE id = 1",
            (str(uuid.uuid4()), utc_now()),
        )
        if int(self._connection.execute("SELECT changes()").fetchone()[0]) != 1:
            raise EndpointError("v0.3 instance revision row is missing")


def _validate_parameter_spec(spec: Any) -> None:
    if not isinstance(spec, dict):
        raise EndpointError("Parameter schema must be an object")
    allowed_types = {"string", "number", "integer", "boolean", "json"}
    for name, rule in spec.items():
        if not isinstance(name, str) or not name:
            raise EndpointError("Parameter names must be non-empty strings")
        if not isinstance(rule, dict):
            raise EndpointError(f"Rule for {name!r} must be an object")
        kind = rule.get("type")
        if kind not in allowed_types:
            raise EndpointError(f"Unsupported type for {name!r}: {kind!r}")
        if "required" in rule and not isinstance(rule["required"], bool):
            raise EndpointError(f"required for {name!r} must be boolean")
        if "nullable" in rule and not isinstance(rule["nullable"], bool):
            raise EndpointError(f"nullable for {name!r} must be boolean")
        if "default" in rule:
            default = rule["default"]
            if default is None:
                if not rule.get("nullable", False):
                    raise EndpointError(
                        f"default for {name!r} is null but nullable is not enabled"
                    )
            else:
                _coerce_value(name, default, kind)


def _coerce_value(name: str, value: Any, kind: str) -> Any:
    if kind == "string":
        if isinstance(value, str):
            return value
        raise EndpointError(f"Parameter {name!r} must be a string")
    if kind == "number":
        if isinstance(value, bool):
            raise EndpointError(f"Parameter {name!r} must be a number")
        if isinstance(value, (int, float)):
            number = float(value) if isinstance(value, float) else value
            if isinstance(number, float) and not math.isfinite(number):
                raise EndpointError(f"Parameter {name!r} must be finite")
            if isinstance(number, int) and not -(2**63) <= number <= 2**63 - 1:
                raise EndpointError(f"Parameter {name!r} is outside SQLite numeric range")
            return number
        if isinstance(value, str):
            try:
                number = float(value)
                if math.isfinite(number):
                    return number
            except ValueError:
                pass
        raise EndpointError(f"Parameter {name!r} must be a number")
    if kind == "integer":
        if isinstance(value, bool):
            raise EndpointError(f"Parameter {name!r} must be an integer")
        if isinstance(value, int):
            if -(2**63) <= value <= 2**63 - 1:
                return value
            raise EndpointError(f"Parameter {name!r} is outside SQLite integer range")
        if isinstance(value, float) and value.is_integer():
            integer = int(value)
            if -(2**63) <= integer <= 2**63 - 1:
                return integer
            raise EndpointError(f"Parameter {name!r} is outside SQLite integer range")
        if isinstance(value, str):
            text = value.strip()
            try:
                if not re.fullmatch(r"[+-]?\d+", text):
                    raise ValueError
                integer = int(text)
                if -(2**63) <= integer <= 2**63 - 1:
                    return integer
            except ValueError:
                pass
        raise EndpointError(f"Parameter {name!r} must be an integer")
    if kind == "boolean":
        if isinstance(value, bool):
            return int(value)
        if isinstance(value, str) and value.lower() in {"true", "false", "1", "0"}:
            return 1 if value.lower() in {"true", "1"} else 0
        raise EndpointError(f"Parameter {name!r} must be boolean")
    if kind == "json":
        try:
            _check_json_depth(value)
            return _json_dump(value)
        except (TypeError, ValueError) as exc:
            raise EndpointError(f"Parameter {name!r} must be JSON-serialisable") from exc
    raise EndpointError(f"Unsupported parameter type: {kind}")


def validate_parameters(spec: Mapping[str, Any], parameters: Mapping[str, Any]) -> dict[str, Any]:
    if not isinstance(parameters, Mapping):
        raise EndpointError("Parameters must be an object")
    unknown = sorted(set(parameters) - set(spec))
    if unknown:
        raise EndpointError(f"Unknown parameters: {', '.join(unknown)}")
    bound: dict[str, Any] = {}
    for name, rule in spec.items():
        provided = name in parameters
        has_default = "default" in rule
        if provided:
            value = parameters[name]
        elif has_default:
            value = rule["default"]
        elif rule.get("required", False):
            raise EndpointError(f"Missing required parameter: {name}")
        else:
            value = None
        if value is None:
            if (provided or has_default) and not rule.get("nullable", False):
                raise EndpointError(f"Parameter {name!r} may not be null")
            bound[name] = None
        else:
            bound[name] = _coerce_value(name, value, rule["type"])
    return bound


def _drain_cursor(cursor: sqlite3.Cursor) -> None:
    while cursor.fetchmany(128):
        pass


def encode_cursor_result(
    cursor: sqlite3.Cursor, result_mode: str, *, drain: bool = False
) -> Any:
    if result_mode == "rows":
        result: list[dict[str, Any]] = []
        encoded_size = 2
        while True:
            row = cursor.fetchone()
            if row is None:
                return result
            if len(result) >= MAX_RESULT_ROWS:
                raise EndpointError(f"Result exceeds the {MAX_RESULT_ROWS}-row limit")
            decoded = decode_row(row)
            encoded_size += len(_json_dump(decoded).encode("utf-8")) + (1 if result else 0)
            if encoded_size > MAX_RESULT_BYTES:
                raise EndpointError(f"Result exceeds the {MAX_RESULT_BYTES}-byte limit")
            result.append(decoded)
    if result_mode == "row":
        row = cursor.fetchone()
        if row is None:
            return None
        result = decode_row(row)
        if len(_json_dump(result).encode("utf-8")) > MAX_RESULT_BYTES:
            raise EndpointError(f"Result exceeds the {MAX_RESULT_BYTES}-byte limit")
        if drain:
            _drain_cursor(cursor)
        return result
    if result_mode == "scalar":
        row = cursor.fetchone()
        if row is None:
            return None
        result = json_value(row[0])
        if len(_json_dump(result).encode("utf-8")) > MAX_RESULT_BYTES:
            raise EndpointError(f"Result exceeds the {MAX_RESULT_BYTES}-byte limit")
        if drain:
            _drain_cursor(cursor)
        return result
    if result_mode == "changes":
        _drain_cursor(cursor)
        return {"changes": max(cursor.rowcount, 0), "lastrowid": cursor.lastrowid}
    raise EndpointError(f"Unsupported result mode: {result_mode}")


def _authorizer_for(operation: str):
    deny_actions = {
        getattr(sqlite3, name)
        for name in (
            "SQLITE_ATTACH",
            "SQLITE_DETACH",
            "SQLITE_ALTER_TABLE",
            "SQLITE_CREATE_INDEX",
            "SQLITE_CREATE_TABLE",
            "SQLITE_CREATE_TEMP_INDEX",
            "SQLITE_CREATE_TEMP_TABLE",
            "SQLITE_CREATE_TEMP_TRIGGER",
            "SQLITE_CREATE_TEMP_VIEW",
            "SQLITE_CREATE_TRIGGER",
            "SQLITE_CREATE_VIEW",
            "SQLITE_CREATE_VTABLE",
            "SQLITE_DROP_INDEX",
            "SQLITE_DROP_TABLE",
            "SQLITE_DROP_TEMP_INDEX",
            "SQLITE_DROP_TEMP_TABLE",
            "SQLITE_DROP_TEMP_TRIGGER",
            "SQLITE_DROP_TEMP_VIEW",
            "SQLITE_DROP_TRIGGER",
            "SQLITE_DROP_VIEW",
            "SQLITE_DROP_VTABLE",
            "SQLITE_PRAGMA",
            "SQLITE_REINDEX",
            "SQLITE_ANALYZE",
        )
        if hasattr(sqlite3, name)
    }
    write_actions = {
        getattr(sqlite3, name)
        for name in ("SQLITE_INSERT", "SQLITE_UPDATE", "SQLITE_DELETE")
        if hasattr(sqlite3, name)
    }

    def authorizer(
        action: int,
        arg1: str | None,
        arg2: str | None,
        database_name: str | None,
        trigger_name: str | None,
    ) -> int:
        del arg2, database_name, trigger_name
        if action in deny_actions:
            return sqlite3.SQLITE_DENY
        if action in write_actions:
            table = (arg1 or "").casefold()
            if operation == "read" or table.startswith(PROTECTED_PREFIXES):
                return sqlite3.SQLITE_DENY
        return sqlite3.SQLITE_OK

    return authorizer


class CapsuleHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = os.name != "nt"

    def server_bind(self) -> None:
        if os.name == "nt" and hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()

    def __init__(
        self,
        address: tuple[str, int],
        capsule: CapsuleDatabase,
        shutdown_token: str,
        quiet: bool = False,
    ) -> None:
        self.capsule = capsule
        self.shutdown_token = shutdown_token
        self.session_token = secrets.token_urlsafe(32)
        self.quiet = quiet
        self._request_slots = threading.BoundedSemaphore(MAX_CONCURRENT_REQUESTS)
        super().__init__(address, CapsuleRequestHandler)

    def process_request(self, request: Any, client_address: Any) -> None:
        if not self._request_slots.acquire(blocking=False):
            threading.Thread(target=self._reject_overloaded, args=(request,), daemon=True).start()
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self._request_slots.release()
            raise

    def process_request_thread(self, request: Any, client_address: Any) -> None:
        try:
            super().process_request_thread(request, client_address)
        finally:
            self._request_slots.release()

    @staticmethod
    def _reject_overloaded(request: Any) -> None:
        try:
            request.settimeout(1.0)
            request.recv(4096)
            request.sendall(
                b"HTTP/1.1 503 Service Unavailable\r\n"
                b"Content-Length: 0\r\n"
                b"Connection: close\r\n\r\n"
            )
        except OSError:
            pass
        finally:
            request.close()


class CapsuleRequestHandler(BaseHTTPRequestHandler):
    server: CapsuleHTTPServer

    def log_message(self, format: str, *args: Any) -> None:
        if not self.server.quiet:
            super().log_message(format, *args)

    def _valid_host(self) -> bool:
        host = (self.headers.get("Host") or "").split(":", 1)[0].strip("[]").lower()
        return host in {"127.0.0.1", "localhost", "::1"}

    def _valid_origin(self) -> bool:
        origin = self.headers.get("Origin")
        if not origin:
            return True
        parsed = urllib.parse.urlparse(origin)
        return parsed.scheme == "http" and parsed.hostname in {"127.0.0.1", "localhost", "::1"}

    def _send_security_headers(self, *, cache_policy: str = "no-store") -> None:
        self.send_header("Content-Security-Policy", CSP)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
        self.send_header("Cache-Control", cache_policy)

    def _send_json(self, status: int, value: Any) -> None:
        payload = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self._send_security_headers()
        self.end_headers()
        self.wfile.write(payload)

    def _send_error_json(self, status: int, message: str) -> None:
        self._send_json(status, {"ok": False, "error": message})

    def _authorised_api(self) -> bool:
        return secrets.compare_digest(
            self.headers.get("X-Capsule-Token", ""), self.server.session_token
        )

    def do_GET(self) -> None:  # noqa: N802
        if not self._valid_host():
            self._send_error_json(HTTPStatus.BAD_REQUEST, "Invalid Host header")
            return
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        if path == "/__capsule/health":
            try:
                manifest = self.server.capsule.manifest()
                overview = self.server.capsule.overview()
                self._send_json(
                    HTTPStatus.OK,
                    {
                        "ok": True,
                        "capsule_id": overview["instance"]["capsule_id"],
                        "app_id": manifest["app_id"],
                        "runtime_protocol": manifest["runtime_protocol"],
                    },
                )
            except (CapsuleError, sqlite3.DatabaseError) as exc:
                self._send_error_json(HTTPStatus.INTERNAL_SERVER_ERROR, str(exc))
            return

        if path == "/__capsule/manifest":
            try:
                manifest = self.server.capsule.manifest()
                self._send_json(
                    HTTPStatus.OK,
                    {
                        "ok": True,
                        "manifest": manifest,
                        "session_token": self.server.session_token,
                        "api": {
                            "read": "/__capsule/read/{name}",
                            "write": "/__capsule/write/{name}",
                        },
                    },
                )
            except (CapsuleError, sqlite3.DatabaseError) as exc:
                self._send_error_json(HTTPStatus.INTERNAL_SERVER_ERROR, str(exc))
            return

        if path == "/__capsule/permissions":
            if not self._authorised_api():
                self._send_error_json(HTTPStatus.FORBIDDEN, "Missing or invalid capsule token")
                return
            try:
                self._send_json(HTTPStatus.OK, {"ok": True, "permissions": self.server.capsule.permissions()})
            except CapsuleError as exc:
                self._send_error_json(HTTPStatus.INTERNAL_SERVER_ERROR, str(exc))
            return

        prefix = "/__capsule/read/"
        if path.startswith(prefix):
            if not self._authorised_api():
                self._send_error_json(HTTPStatus.FORBIDDEN, "Missing or invalid capsule token")
                return
            name = urllib.parse.unquote(path[len(prefix) :])
            query = urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
            params = {key: values[-1] for key, values in query.items()}
            try:
                result = self.server.capsule.execute_endpoint(name, "read", params)
                self._send_json(HTTPStatus.OK, {"ok": True, "result": result})
            except EndpointError as exc:
                self._send_error_json(HTTPStatus.BAD_REQUEST, str(exc))
            return

        try:
            manifest = self.server.capsule.manifest()
            asset_path = manifest["entry_asset"] if path == "/" else urllib.parse.unquote(path[1:])
            asset = self.server.capsule.asset(asset_path)
            if asset is None:
                self._send_error_json(HTTPStatus.NOT_FOUND, "Asset not found")
                return
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", asset.media_type)
            self.send_header("Content-Length", str(len(asset.content)))
            self.send_header("ETag", f'"{asset.sha256}"')
            self._send_security_headers(cache_policy=asset.cache_policy)
            self.end_headers()
            self.wfile.write(asset.content)
        except (CapsuleError, VerificationError) as exc:
            self._send_error_json(HTTPStatus.INTERNAL_SERVER_ERROR, str(exc))

    def do_POST(self) -> None:  # noqa: N802
        if not self._valid_host() or not self._valid_origin():
            self._send_error_json(HTTPStatus.BAD_REQUEST, "Invalid local request origin")
            return
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        if path == "/__capsule/shutdown":
            token = self.headers.get("X-Capsule-Shutdown", "")
            if not secrets.compare_digest(token, self.server.shutdown_token):
                self._send_error_json(HTTPStatus.FORBIDDEN, "Invalid shutdown token")
                return
            self._send_json(HTTPStatus.OK, {"ok": True, "stopping": True})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
            return

        prefix = "/__capsule/write/"
        if not path.startswith(prefix):
            self._send_error_json(HTTPStatus.NOT_FOUND, "Unknown endpoint")
            return
        if not self._authorised_api():
            self._send_error_json(HTTPStatus.FORBIDDEN, "Missing or invalid capsule token")
            return
        content_type = (self.headers.get("Content-Type") or "").split(";", 1)[0].strip()
        if content_type != "application/json":
            self._send_error_json(HTTPStatus.UNSUPPORTED_MEDIA_TYPE, "Expected application/json")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self._send_error_json(HTTPStatus.BAD_REQUEST, "Invalid Content-Length")
            return
        if length < 0 or length > MAX_REQUEST_BYTES:
            self._send_error_json(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, "Request body too large")
            return
        try:
            body = self.rfile.read(length)
            params = json.loads(body.decode("utf-8")) if body else {}
            _check_json_depth(params)
            if not isinstance(params, dict):
                raise ValueError("JSON body must be an object")
            name = urllib.parse.unquote(path[len(prefix) :])
            result = self.server.capsule.execute_endpoint(name, "write", params)
            self._send_json(HTTPStatus.OK, {"ok": True, "result": result})
        except (UnicodeDecodeError, json.JSONDecodeError, RecursionError, ValueError) as exc:
            self._send_error_json(HTTPStatus.BAD_REQUEST, f"Invalid JSON body: {exc}")
        except EndpointError as exc:
            self._send_error_json(HTTPStatus.BAD_REQUEST, str(exc))

    def do_OPTIONS(self) -> None:  # noqa: N802
        self._send_error_json(HTTPStatus.METHOD_NOT_ALLOWED, "Cross-origin access is not enabled")


def find_free_port(host: str = "127.0.0.1") -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind((host, 0))
        return int(sock.getsockname()[1])


def windows_current_principal() -> str:
    """Read the process token's Windows principal without trusting inherited env vars."""

    if os.name != "nt":
        raise CapsuleError("Windows principal lookup is available only on Windows")
    import ctypes
    from ctypes import wintypes

    name_sam_compatible = 2
    size = wintypes.ULONG(0)
    get_username = ctypes.WinDLL("secur32", use_last_error=True).GetUserNameExW
    get_username.argtypes = [wintypes.ULONG, wintypes.LPWSTR, ctypes.POINTER(wintypes.ULONG)]
    get_username.restype = wintypes.BOOL
    get_username(name_sam_compatible, None, ctypes.byref(size))
    if size.value <= 1:
        raise CapsuleError("Could not determine the current Windows security principal")
    buffer = ctypes.create_unicode_buffer(size.value)
    if not get_username(name_sam_compatible, buffer, ctypes.byref(size)) or not buffer.value:
        raise CapsuleError("Could not determine the current Windows security principal")
    return buffer.value


def windows_state_identity_key(principal: str) -> str:
    """Return a versioned path-safe key for one Windows process principal."""

    normalized = principal.strip().casefold()
    if not normalized:
        raise CapsuleError("Windows security principal must not be empty")
    digest = hashlib.sha256(f"windows-principal-v2\0{normalized}".encode("utf-8")).hexdigest()[:16]
    return f"win-v2-{digest}"


def state_directory() -> Path:
    configured = os.environ.get("SQLITE_CAPSULE_STATE_DIR")
    if configured:
        candidate = Path(configured).expanduser()
    else:
        identity_key = (
            str(os.getuid())
            if hasattr(os, "getuid")
            else windows_state_identity_key(windows_current_principal())
        )
        candidate = Path(tempfile.gettempdir()) / f"sqlite-capsule-state-{identity_key}"
    if candidate.is_symlink():
        raise CapsuleError(f"Refusing symlinked state directory: {candidate}")
    resolved = candidate.resolve()
    resolved.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not resolved.is_dir():
        raise CapsuleError(f"Capsule state path is not a directory: {resolved}")
    if os.name != "nt":
        os.chmod(resolved, 0o700)
    return resolved


def state_path(capsule_path: Path) -> Path:
    key = hashlib.sha256(str(capsule_path.resolve()).encode("utf-8")).hexdigest()[:16]
    return state_directory() / f"{key}.json"


def read_state(capsule_path: Path) -> dict[str, Any] | None:
    path = state_path(capsule_path)
    if path.is_symlink():
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return None
    if not isinstance(value, dict):
        return None
    required_strings = ("capsule", "capsule_id", "host", "url", "shutdown_token", "log")
    if not all(isinstance(value.get(name), str) for name in required_strings):
        return None
    if Path(value["capsule"]).resolve() != capsule_path.resolve():
        return None
    if value["host"] not in {"127.0.0.1", "localhost"} or not isinstance(
        value.get("port"), int
    ):
        return None
    if not 1 <= value["port"] <= 65535 or value["url"] != (
        f"http://{value['host']}:{value['port']}"
    ):
        return None
    if not local_http_url(value["url"]):
        return None
    if not isinstance(value.get("pid"), int) or value["pid"] <= 0:
        return None
    if len(value["shutdown_token"]) < 32:
        return None
    return value


def write_state(capsule_path: Path, state: Mapping[str, Any]) -> Path:
    path = state_path(capsule_path)
    if path.is_symlink():
        raise CapsuleError(f"Refusing symlinked state file: {path}")
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.stem}.", dir=path.parent)
    temp = Path(temporary_name)
    try:
        if os.name != "nt":
            os.chmod(temp, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            descriptor = -1
            output.write(_format_human_json(dict(state)) + "\n")
            output.flush()
            os.fsync(output.fileno())
        temp.replace(path)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        temp.unlink(missing_ok=True)
    return path


def remove_state(capsule_path: Path) -> None:
    try:
        state_path(capsule_path).unlink()
    except FileNotFoundError:
        pass


def local_http_url(url: str) -> bool:
    try:
        parsed = urllib.parse.urlparse(url)
        port = parsed.port
    except ValueError:
        return False
    return (
        parsed.scheme == "http"
        and parsed.hostname in {"127.0.0.1", "localhost", "::1"}
        and parsed.username is None
        and parsed.password is None
        and port is not None
        and 1 <= port <= 65535
    )


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, newurl: str) -> None:
        del req, fp, code, msg, headers, newurl
        return None


def _open_local(request: str | urllib.request.Request, *, timeout: float):
    url = request.full_url if isinstance(request, urllib.request.Request) else request
    if not local_http_url(url):
        raise CapsuleError(f"Refusing non-loopback lifecycle URL: {url!r}")
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        _NoRedirectHandler(),
    )
    response = opener.open(request, timeout=timeout)
    if not local_http_url(response.geturl()):
        response.close()
        raise CapsuleError("Lifecycle request left the loopback origin")
    return response


def health(
    url: str,
    timeout: float = 1.0,
    *,
    expected_capsule_id: str | None = None,
    expected_runtime_protocol: str | None = None,
) -> dict[str, Any] | None:
    try:
        with _open_local(f"{url}/__capsule/health", timeout=timeout) as response:
            body = response.read(65_537)
            if len(body) > 65_536:
                return None
            value = json.loads(body.decode("utf-8"))
        if not isinstance(value, dict) or value.get("ok") is not True:
            return None
        runtime_protocol = value.get("runtime_protocol")
        if runtime_protocol not in SUPPORTED_RUNTIME_PROTOCOLS:
            return None
        if (
            expected_runtime_protocol is not None
            and runtime_protocol != expected_runtime_protocol
        ):
            return None
        if not isinstance(value.get("capsule_id"), str) or not isinstance(
            value.get("app_id"), str
        ):
            return None
        if expected_capsule_id is not None and value["capsule_id"] != expected_capsule_id:
            return None
        return value
    except (CapsuleError, urllib.error.URLError, OSError, json.JSONDecodeError):
        return None


def public_state(state: Mapping[str, Any]) -> dict[str, Any]:
    return {
        key: value
        for key, value in state.items()
        if key not in {"shutdown_token"}
    }


def verify_or_raise(capsule_path: Path) -> dict[str, Any]:
    with CapsuleDatabase(capsule_path, read_only=True) as capsule:
        report = capsule.verify()
    if not report["ok"]:
        raise VerificationError("; ".join(report["errors"]))
    return report


def serve_foreground(
    capsule_path: Path,
    host: str,
    port: int,
    shutdown_token: str,
    *,
    quiet: bool = False,
    open_browser: bool = False,
) -> None:
    if host not in {"127.0.0.1", "localhost"}:
        raise CapsuleError("Bootstrap host may bind only to loopback")
    verify_or_raise(capsule_path)
    capsule = CapsuleDatabase(capsule_path, read_only=False)
    server = CapsuleHTTPServer((host, port), capsule, shutdown_token, quiet=quiet)
    actual_port = int(server.server_address[1])
    url = f"http://{host}:{actual_port}"
    print(_format_human_json({"ok": True, "url": url, "capsule": str(capsule_path)}), flush=True)
    if open_browser:
        webbrowser.open(url)
    try:
        server.serve_forever(poll_interval=0.25)
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        capsule.close()


def start_detached(capsule_path: Path, *, host: str, port: int, open_browser: bool) -> dict[str, Any]:
    if host not in {"127.0.0.1", "localhost"}:
        raise CapsuleError("Bootstrap host may bind only to loopback")
    verification = verify_or_raise(capsule_path)
    expected_capsule_id = str(verification["overview"]["instance"]["capsule_id"])
    expected_runtime_protocol = str(verification["manifest"]["runtime_protocol"])
    existing = read_state(capsule_path)
    existing_health = (
        health(
            existing["url"],
            expected_capsule_id=expected_capsule_id,
            expected_runtime_protocol=expected_runtime_protocol,
        )
        if existing
        else None
    )
    if existing and existing_health:
        if open_browser:
            webbrowser.open(existing["url"])
        return {
            "ok": True,
            "already_running": True,
            **public_state(existing),
            "health": existing_health,
        }
    remove_state(capsule_path)
    if port == 0:
        port = find_free_port(host)
    shutdown_token = secrets.token_urlsafe(32)
    url = f"http://{host}:{port}"
    state_dir = state_directory()
    state_dir.mkdir(parents=True, exist_ok=True)
    log_path = state_dir / f"{state_path(capsule_path).stem}.log"
    script_path = Path(__file__).resolve()
    command = [
        sys.executable,
        str(script_path),
        "_serve",
        str(capsule_path),
        "--host",
        host,
        "--port",
        str(port),
        f"--shutdown-token={shutdown_token}",
        "--trust-capsule",
        "--quiet",
    ]
    creationflags = 0
    kwargs: dict[str, Any] = {"start_new_session": True}
    if os.name == "nt":
        creationflags = getattr(subprocess, "DETACHED_PROCESS", 0) | getattr(
            subprocess, "CREATE_NEW_PROCESS_GROUP", 0
        )
        kwargs = {"creationflags": creationflags}
    with log_path.open("ab") as log_file:
        process = subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            # This must stay true on Windows as well. With captured parent output,
            # inheriting unrelated handles keeps the caller's pipes open after this
            # process exits, so a documented detached start can appear to hang even
            # though the host is already healthy.
            close_fds=True,
            **kwargs,
        )
    state = {
        "capsule": str(capsule_path),
        "capsule_id": expected_capsule_id,
        "runtime_protocol": expected_runtime_protocol,
        "pid": process.pid,
        "host": host,
        "port": port,
        "url": url,
        "shutdown_token": shutdown_token,
        "log": str(log_path),
        "started_at": utc_now(),
    }
    write_state(capsule_path, state)
    for _ in range(60):
        info = health(
            url,
            timeout=0.25,
            expected_capsule_id=expected_capsule_id,
            expected_runtime_protocol=expected_runtime_protocol,
        )
        if info:
            if open_browser:
                webbrowser.open(url)
            return {
                "ok": True,
                "already_running": False,
                **public_state(state),
                "health": info,
            }
        if process.poll() is not None:
            break
        time.sleep(0.1)
    remove_state(capsule_path)
    try:
        process.terminate()
    except OSError:
        pass
    tail = ""
    try:
        tail = log_path.read_text(encoding="utf-8", errors="replace")[-2000:]
    except OSError:
        pass
    raise CapsuleError(f"Detached host failed to start. Log: {log_path}\n{tail}")


def stop_detached(capsule_path: Path) -> dict[str, Any]:
    state = read_state(capsule_path)
    if not state:
        return {"ok": True, "running": False, "message": "No host state found"}
    request = urllib.request.Request(
        f"{state['url']}/__capsule/shutdown",
        method="POST",
        data=b"{}",
        headers={
            "Content-Type": "application/json",
            "X-Capsule-Shutdown": state.get("shutdown_token", ""),
        },
    )
    try:
        with _open_local(request, timeout=2.0) as response:
            response.read()
    except (CapsuleError, urllib.error.URLError, OSError):
        pass
    for _ in range(30):
        if not health(
            state["url"],
            timeout=0.15,
            expected_capsule_id=state["capsule_id"],
            expected_runtime_protocol=state.get("runtime_protocol"),
        ):
            remove_state(capsule_path)
            return {"ok": True, "running": False, "stopped": True, "url": state["url"]}
        time.sleep(0.1)
    return {
        "ok": False,
        "running": True,
        "message": "Host did not stop cleanly",
        **public_state(state),
    }


def print_instructions(capsule: CapsuleDatabase, audience: str) -> None:
    overview = capsule.overview()
    print(f"# {overview['instance']['title']}\n")
    for row in capsule.runbooks(audience):
        print(f"## {row['sequence']:02d}. {row['title']}\n")
        print(row["body_md"].rstrip())
        if row.get("command_id"):
            print(f"\nCommand reference: `{row['command_id']}`")
        print()
    print("## Command templates\n")
    for command in capsule.commands():
        print(f"### {command['id']} — {command['purpose']}\n")
        print(f"Risk: `{command['risk_class']}`  ")
        print(f"Platform: `{command['platform']}`  ")
        print(f"Working directory: `{command['cwd']}`\n")
        print(f"```text\n{command['command_template']}\n```\n")
        if command.get("argv_json"):
            print("Structured arguments (preferred):\n")
            print(f"```json\n{_format_human_json(command['argv_json'])}\n```\n")
        print(f"Success: {command['success_condition']}\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="capsule",
        description="Inspect, verify, and run SQLite Capsule v0.2 files.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    def capsule_arg(subparser: argparse.ArgumentParser) -> None:
        subparser.add_argument("capsule", type=Path)

    inspect_parser = subparsers.add_parser("inspect", help="Print manifest and inventory")
    capsule_arg(inspect_parser)

    instructions_parser = subparsers.add_parser(
        "instructions", help="Print the runbook embedded in the capsule"
    )
    capsule_arg(instructions_parser)
    instructions_parser.add_argument("--audience", default="agent", choices=["agent", "human", "runtime"])

    assets_parser = subparsers.add_parser("assets", help="List embedded assets")
    capsule_arg(assets_parser)

    verify_parser = subparsers.add_parser("verify", help="Verify structure and declared checks")
    capsule_arg(verify_parser)

    extract_parser = subparsers.add_parser("extract-host", help="Extract embedded standalone host")
    capsule_arg(extract_parser)
    extract_parser.add_argument("--output", type=Path, default=Path(".capsule-cache/capsule_host.py"))
    extract_parser.add_argument(
        "--replace", action="store_true", help="Replace an unrelated existing output file"
    )

    run_parser = subparsers.add_parser("run", help="Run host in the foreground")
    capsule_arg(run_parser)
    run_parser.add_argument("--host", default="127.0.0.1")
    run_parser.add_argument("--port", type=int, default=8765)
    run_parser.add_argument("--open", action="store_true", dest="open_browser")
    run_parser.add_argument("--trust-capsule", action="store_true")
    run_parser.add_argument("--quiet", action="store_true")

    start_parser = subparsers.add_parser("start", help="Start a detached local host")
    capsule_arg(start_parser)
    start_parser.add_argument("--host", default="127.0.0.1")
    start_parser.add_argument("--port", type=int, default=0)
    start_parser.add_argument("--open", action="store_true", dest="open_browser")
    start_parser.add_argument("--trust-capsule", action="store_true")

    stop_parser = subparsers.add_parser("stop", help="Stop a detached local host")
    capsule_arg(stop_parser)

    status_parser = subparsers.add_parser("status", help="Check detached host status")
    capsule_arg(status_parser)

    serve_parser = subparsers.add_parser("_serve", help=argparse.SUPPRESS)
    capsule_arg(serve_parser)
    serve_parser.add_argument("--host", default="127.0.0.1")
    serve_parser.add_argument("--port", type=int, required=True)
    serve_parser.add_argument("--shutdown-token", required=True)
    serve_parser.add_argument("--trust-capsule", action="store_true")
    serve_parser.add_argument("--quiet", action="store_true")

    return parser


def main(argv: Iterable[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            reconfigure(encoding="utf-8", errors="strict")
    parser = build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    capsule_path = args.capsule.expanduser().resolve()
    try:
        if args.command == "inspect":
            with CapsuleDatabase(capsule_path, read_only=True) as capsule:
                value = {
                    "manifest": capsule.manifest(),
                    "overview": capsule.overview(),
                    "assets": capsule.list_assets(),
                    "prompts": capsule.prompts(),
                    "commands": capsule.commands(),
                    "documents": [
                        {key: item[key] for key in ("slug", "title", "media_type", "sequence")}
                        for item in capsule.documents()
                    ],
                }
            print(_format_human_json(value))
            return 0

        if args.command == "instructions":
            with CapsuleDatabase(capsule_path, read_only=True) as capsule:
                print_instructions(capsule, args.audience)
            return 0

        if args.command == "assets":
            with CapsuleDatabase(capsule_path, read_only=True) as capsule:
                print(_format_human_json(capsule.list_assets()))
            return 0

        if args.command == "verify":
            with CapsuleDatabase(capsule_path, read_only=True) as capsule:
                report = capsule.verify()
            print(_format_human_json(report))
            return 0 if report["ok"] else 1

        if args.command == "extract-host":
            with CapsuleDatabase(capsule_path, read_only=True) as capsule:
                output = capsule.extract_asset(
                    "bootstrap/capsule_host.py",
                    args.output.resolve(),
                    replace=args.replace,
                )
            print(_format_human_json({"ok": True, "output": str(output)}))
            return 0

        if args.command in {"run", "start", "_serve"} and not args.trust_capsule:
            raise CapsuleError(
                "Active execution requires --trust-capsule. Inspect and verify unfamiliar files first."
            )

        if args.command == "run":
            if args.host not in {"127.0.0.1", "localhost"}:
                raise CapsuleError("Bootstrap host may bind only to loopback")
            serve_foreground(
                capsule_path,
                "127.0.0.1",
                args.port,
                secrets.token_urlsafe(32),
                quiet=args.quiet,
                open_browser=args.open_browser,
            )
            return 0

        if args.command == "start":
            if args.host not in {"127.0.0.1", "localhost"}:
                raise CapsuleError("Bootstrap host may bind only to loopback")
            result = start_detached(
                capsule_path,
                host="127.0.0.1",
                port=args.port,
                open_browser=args.open_browser,
            )
            print(_format_human_json(result))
            return 0

        if args.command == "stop":
            result = stop_detached(capsule_path)
            print(_format_human_json(result))
            return 0 if result.get("ok") else 1

        if args.command == "status":
            state = read_state(capsule_path)
            info = (
                health(
                    state["url"],
                    expected_capsule_id=state["capsule_id"],
                    expected_runtime_protocol=state.get("runtime_protocol"),
                )
                if state
                else None
            )
            value = {
                "ok": True,
                "running": bool(info),
                "state": public_state(state) if state else None,
                "health": info,
            }
            print(_format_human_json(value))
            return 0 if info else 1

        if args.command == "_serve":
            if args.host not in {"127.0.0.1", "localhost"}:
                raise CapsuleError("Bootstrap host may bind only to loopback")
            serve_foreground(
                capsule_path,
                "127.0.0.1",
                args.port,
                args.shutdown_token,
                quiet=args.quiet,
                open_browser=False,
            )
            return 0

        parser.error(f"Unknown command: {args.command}")
        return 2
    except (CapsuleError, VerificationError, EndpointError) as exc:
        print(_format_human_json({"ok": False, "error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

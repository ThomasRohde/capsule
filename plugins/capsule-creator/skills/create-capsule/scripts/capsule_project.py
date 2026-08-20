#!/usr/bin/env python3
"""Scaffold and deterministically build reviewable SQLite Capsule projects."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import re
import sqlite3
import struct
import subprocess
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping

PROJECT_FORMAT = "org.sqlite-capsule.source-project/0.2"
PROJECT_FORMAT_V03 = "org.sqlite-capsule.source-project/0.3"
FORMAT_ID = "org.sqlite-capsule"
DEFAULT_FORMAT_VERSION = "0.2"
SUPPORTED_FORMAT_VERSIONS = ("0.2", "0.3")
PROJECT_FORMATS = {
    "0.2": PROJECT_FORMAT,
    "0.3": PROJECT_FORMAT_V03,
}
RUNTIME_PROTOCOL = "capsule-http/0.2"
V03_HOST_PROFILE = "org.sqlite-capsule.host-profile/0.3"
TEMPLATE_STATE_PROFILE = "org.sqlite-capsule.template-state/1"
DATASET_STATE_PROFILE = "org.sqlite-capsule.dataset-state/1"
TEMPLATE_PLATFORM_RESET_PROFILE = "org.sqlite-capsule.template-platform-reset/1"
TEMPLATE_STATE_DOC_SLUG = "org.sqlite-capsule.template-state"
DATASET_STREAM_CONTEXT = b"SQLite Capsule dataset-state canonical stream v1\0"
MAX_TEMPLATE_ROWS = 100_000
MAX_TEMPLATE_STREAM_BYTES = 512 * 1024 * 1024
APP_ID = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)+$")
MEDIA_TYPES = {
    ".css": "text/css; charset=utf-8",
    ".gif": "image/gif",
    ".html": "text/html; charset=utf-8",
    ".ico": "image/x-icon",
    ".jpeg": "image/jpeg",
    ".jpg": "image/jpeg",
    ".js": "text/javascript; charset=utf-8",
    ".json": "application/json",
    ".md": "text/markdown; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".png": "image/png",
    ".svg": "image/svg+xml",
    ".txt": "text/plain; charset=utf-8",
    ".wasm": "application/wasm",
    ".webp": "image/webp",
}
REQUIRED_PROJECT_FILES = (
    "capsule-project.json",
    "domain.sql",
    "source/app/index.html",
    "source/app/app.js",
    "source/app/styles.css",
    "source/data/seed.json",
    "source/endpoints.json",
    "source/checks.json",
    "source/prompts.json",
    "source/runbooks.json",
    "source/docs.json",
)


class ProjectError(RuntimeError):
    """Raised when project source cannot be safely scaffolded or built."""


def _json_dump(value: Any, *, indent: int | None = None) -> str:
    separators = None if indent is not None else (",", ":")
    return json.dumps(
        value,
        ensure_ascii=False,
        indent=indent,
        sort_keys=True,
        separators=separators,
    )


def _sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _utc_now() -> str:
    return (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def _write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content.rstrip() + "\n", encoding="utf-8", newline="\n")


def _write_json(path: Path, value: Any) -> None:
    _write_text(path, _json_dump(value, indent=2))


def _load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ProjectError(f"Could not read JSON source {path}: {exc}") from exc


def _load_array(path: Path) -> list[dict[str, Any]]:
    value = _load_json(path)
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise ProjectError(f"{path} must contain an array of objects")
    return value


def _quote_identifier(value: str) -> str:
    if not isinstance(value, str) or not value or "\x00" in value or len(value.encode("utf-8")) > 256:
        raise ProjectError(f"Unsafe SQLite identifier: {value!r}")
    return '"' + value.replace('"', '""') + '"'


def _insert_mapping(
    connection: sqlite3.Connection,
    table: str,
    row: Mapping[str, Any],
    *,
    json_fields: Iterable[str] = (),
) -> None:
    prepared = dict(row)
    for field in json_fields:
        if field not in prepared:
            raise ProjectError(f"Missing JSON field {field!r} for {table!r}")
        prepared[field] = _json_dump(prepared[field])
    columns = list(prepared)
    if not columns:
        raise ProjectError(f"Cannot insert an empty row into {table!r}")
    names = ", ".join(_quote_identifier(column) for column in columns)
    values = ", ".join(f":{column}" for column in columns)
    connection.execute(
        f"INSERT INTO {_quote_identifier(table)} ({names}) VALUES ({values})",
        prepared,
    )


def _resolve_within(root: Path, relative: str) -> Path:
    if not isinstance(relative, str) or not relative:
        raise ProjectError("Source path must be a non-empty string")
    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError as exc:
        raise ProjectError(f"Source path escapes the project: {relative!r}") from exc
    return candidate


def _resource_root(explicit: Path | None = None) -> Path:
    """Resolve the plugin-owned format/runtime bundle.

    A repository root remains a development-only override so maintainers can
    compare the bundled snapshot with live sources. Normal plugin use always
    resolves beside this script and therefore works after the plugin directory
    is copied away from its source repository.
    """

    candidates = []
    if explicit is not None:
        candidates.append(explicit.expanduser().resolve())
    candidates.append(Path(__file__).resolve().parent.parent / "assets")
    for candidate in candidates:
        if (
            (candidate / "format" / "capsule-v0.2.sql").is_file()
            and (candidate / "format" / "capsule-v0.2.conformance.json").is_file()
            and (candidate / "format" / "capsule-v0.3.sql").is_file()
            and (candidate / "format" / "capsule-v0.3.conformance.json").is_file()
            and (candidate / "format" / "capsule_conformance.py").is_file()
            and (candidate / "runtime" / "capsule_host.py").is_file()
            and (candidate / "runtime" / "browser" / "capsule-client.js").is_file()
        ):
            return candidate
    raise ProjectError("The plugin's bundled SQLite Capsule resources are incomplete")


def _load_capsule_host(resources: Path):
    host_path = resources / "runtime" / "capsule_host.py"
    module_name = f"capsule_creator_host_{_sha256(str(host_path).encode())[:12]}"
    existing = sys.modules.get(module_name)
    if existing is not None:
        return existing
    spec = importlib.util.spec_from_file_location(module_name, host_path)
    if spec is None or spec.loader is None:
        raise ProjectError(f"Could not load bundled host: {host_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def _project_config(project: Path) -> dict[str, Any]:
    project = project.expanduser().resolve()
    missing = [relative for relative in REQUIRED_PROJECT_FILES if not (project / relative).is_file()]
    if missing:
        raise ProjectError("Capsule project is incomplete: " + ", ".join(missing))
    config = _load_json(project / "capsule-project.json")
    if not isinstance(config, dict):
        raise ProjectError("capsule-project.json must contain an object")
    format_version = config.get("format_version", DEFAULT_FORMAT_VERSION)
    if format_version not in SUPPORTED_FORMAT_VERSIONS:
        raise ProjectError(f"Unsupported capsule format version: {format_version!r}")
    expected_project_format = PROJECT_FORMATS[format_version]
    if config.get("project_format") != expected_project_format:
        raise ProjectError(
            "Project format does not match format_version: "
            f"expected {expected_project_format!r}, got {config.get('project_format')!r}"
        )
    required_strings = [
        "capsule_id",
        "title",
        "summary",
        "app_id",
        "app_version",
        "entry_asset",
        "created_at",
    ]
    if format_version == "0.2":
        required_strings.append("updated_at")
    else:
        required_strings.extend(
            (
                "revision_id",
                "released_at",
                "content_updated_at",
                "application_category",
                "document_kind",
                "data_schema_id",
            )
        )
    for field in required_strings:
        if not isinstance(config.get(field), str) or not config[field].strip():
            raise ProjectError(f"Project field {field!r} must be a non-empty string")
    if not APP_ID.fullmatch(config["app_id"]):
        raise ProjectError("app_id must be a lowercase dotted or hyphenated identifier")
    permissions = config.get("permissions_json")
    if not isinstance(permissions, dict):
        raise ProjectError("permissions_json must be an object")
    network = permissions.get("network")
    if not isinstance(network, dict) or network.get("value") != "none":
        raise ProjectError("permissions_json network.value must be 'none'")
    non_executable_assets = config.get("non_executable_assets", [])
    if not isinstance(non_executable_assets, list) or not all(
        isinstance(path, str)
        and path.startswith("app/")
        and all(part not in {"", ".", ".."} for part in path.split("/"))
        for path in non_executable_assets
    ):
        raise ProjectError("non_executable_assets must contain safe app/ asset paths")
    if len(non_executable_assets) != len(set(non_executable_assets)):
        raise ProjectError("non_executable_assets must not contain duplicates")
    if config["entry_asset"] in non_executable_assets:
        raise ProjectError("entry_asset cannot be declared non-executable")
    if format_version == "0.3":
        for field in ("capsule_id", "revision_id"):
            try:
                parsed = uuid.UUID(config[field])
            except (AttributeError, ValueError) as exc:
                raise ProjectError(f"{field} must be a canonical RFC 4122 UUID") from exc
            if str(parsed) != config[field] or parsed.variant != uuid.RFC_4122:
                raise ProjectError(f"{field} must be a canonical RFC 4122 UUID")
        if not isinstance(config.get("data_schema_version"), int) or config[
            "data_schema_version"
        ] < 1:
            raise ProjectError("data_schema_version must be a positive integer")
        tags = config.get("tags_json")
        if not (
            isinstance(tags, list)
            and len(tags) <= 64
            and len(tags) == len(set(tags))
            and all(
                isinstance(tag, str)
                and 1 <= len(tag) <= 128
                and len(tag.encode("utf-8")) <= 128
                for tag in tags
            )
        ):
            raise ProjectError("tags_json must contain at most 64 unique bounded strings")
        if not (project / "source" / "data-contract.json").is_file():
            raise ProjectError("A v0.3 project requires source/data-contract.json")
        template_state = config.get("template_state")
        if template_state is not None and not (
            isinstance(template_state, dict)
            and template_state
            and all(
                isinstance(dataset_id, str)
                and disposition in {"seed", "empty"}
                for dataset_id, disposition in template_state.items()
            )
        ):
            raise ProjectError(
                "template_state must map every dataset id to 'seed' or 'empty'"
            )
    elif "template_state" in config:
        raise ProjectError("template_state is available only for v0.3 projects")
    return config


def _starter_index(title: str) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/app/styles.css">
</head>
<body>
  <a class="skip-link" href="#content">Skip to content</a>
  <div class="app-shell">
    <header class="titlebar"><span class="app-mark" aria-hidden="true">◉</span><strong>{title}</strong><span class="offline">Local only</span></header>
    <div class="workspace">
      <aside class="navigation" aria-label="Application pages"><p>Application</p><button class="nav-item is-active" type="button" aria-current="page"><span aria-hidden="true">☷</span>Items</button><div class="boundary"><strong>SQLite Capsule</strong><small>Offline · named interfaces</small></div></aside>
      <main id="content" class="content-surface" tabindex="-1">
        <header class="page-heading"><p class="eyebrow">SQLite Capsule</p><h1>{title}</h1><p id="summary">Loading the embedded application…</p></header>
        <form id="new-item-form" class="card">
          <label for="new-item">Add an item</label>
          <div class="form-row"><input id="new-item" name="title" maxlength="120" required autocomplete="off"><button type="submit">Add</button></div>
        </form>
        <p id="status" role="status" aria-live="polite"></p>
        <ul id="items" aria-label="Items"></ul>
      </main>
    </div>
  </div>
  <script src="/app/capsule-client.js"></script>
  <script src="/app/app.js"></script>
</body>
</html>"""


STARTER_STYLES = """:root {
  color-scheme: dark;
  font-family: "Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
  --smoke: #0d0d0d; --layer: #202020; --card: rgba(255,255,255,.0512); --card-hover: rgba(255,255,255,.0837);
  --stroke: rgba(255,255,255,.0698); --text: #fff; --secondary: rgba(255,255,255,.786); --tertiary: rgba(255,255,255,.5442);
  --accent: #60cdff; --accent-hover: #4cc2ff; --accent-text: #000; --control: rgba(255,255,255,.0605); --ok: #6ccb5f;
}
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; color: var(--text); background: var(--smoke); font-size: 14px; }
button, input { font: inherit; } button:focus-visible, input:focus-visible, a:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.skip-link { position: fixed; z-index: 10; top: 6px; left: 12px; padding: 7px 12px; border-radius: 4px; color: var(--accent-text); background: var(--accent); transform: translateY(-150%); }.skip-link:focus { transform: translateY(0); }
.app-shell { min-height: 100vh; background: radial-gradient(120% 90% at 82% -20%, rgba(96,205,255,.055), transparent 60%), var(--layer); }
.titlebar { display: flex; height: 48px; align-items: center; gap: 9px; padding: 0 16px; font-size: 12px; }.app-mark { color: var(--accent); }.offline { margin-left: auto; padding: 4px 9px; border: 1px solid var(--stroke); border-radius: 12px; color: var(--secondary); background: var(--card); font-size: 10px; }
.workspace { display: grid; min-height: calc(100vh - 48px); grid-template-columns: 210px minmax(0,1fr); }
.navigation { display: flex; flex-direction: column; padding: 14px 8px; }.navigation > p { margin: 0 11px 9px; color: var(--tertiary); font-size: 10px; font-weight: 600; text-transform: uppercase; letter-spacing: .06em; }
.nav-item { position: relative; display: flex; min-height: 38px; align-items: center; gap: 10px; padding: 6px 12px; border: 0; border-radius: 4px; color: var(--text); background: var(--card-hover); text-align: left; }.nav-item::before { position: absolute; top: 9px; bottom: 9px; left: 0; width: 3px; border-radius: 2px; background: var(--accent); content: ""; }
.boundary { margin-top: auto; padding: 11px 12px; border: 1px solid var(--stroke); border-radius: 4px; background: var(--card); }.boundary strong,.boundary small { display: block; }.boundary strong { font-size: 11px; }.boundary small { margin-top: 3px; color: var(--tertiary); font-size: 9px; }
.content-surface { min-width: 0; padding: 28px 36px 40px; border-top: 1px solid var(--stroke); border-left: 1px solid var(--stroke); border-top-left-radius: 8px; background: rgba(255,255,255,.0326); }
.page-heading { margin-bottom: 22px; }.eyebrow { margin: 0 0 6px; color: var(--accent); font-size: 11px; font-weight: 650; letter-spacing: .055em; text-transform: uppercase; }h1 { margin: 0 0 7px; font: 600 28px/1.2 "Segoe UI Variable Display","Segoe UI",sans-serif; }#summary { max-width: 68ch; margin: 0; color: var(--tertiary); line-height: 1.45; }
.card, li { border: 1px solid var(--stroke); border-radius: 4px; background: var(--card); }.card { padding: 15px 16px; }label { display: block; margin-bottom: 9px; font-weight: 600; }.form-row { display: flex; gap: 8px; }
input { min-width: 0; flex: 1; min-height: 34px; padding: 6px 10px; border: 1px solid var(--stroke); border-radius: 4px; color: var(--text); background: var(--control); }button { min-height: 34px; padding: 6px 14px; border: 1px solid var(--stroke); border-radius: 4px; color: var(--accent-text); background: var(--accent); font-weight: 600; cursor: pointer; }button:hover { background: var(--accent-hover); }
#status { min-height: 20px; margin: 10px 0; color: var(--secondary); }ul { display: flex; margin: 0; padding: 0; flex-direction: column; gap: 4px; list-style: none; }li { display: flex; min-height: 52px; align-items: center; justify-content: space-between; gap: 16px; padding: 9px 12px; }li:hover { background: var(--card-hover); }li.done span { color: var(--tertiary); text-decoration: line-through; }li button { color: var(--secondary); background: var(--control); font-size: 12px; }
@media (max-width: 700px) { .workspace { display: flex; flex-direction: column; }.navigation { flex-direction: row; padding: 6px 8px; border-top: 1px solid var(--stroke); }.navigation > p,.boundary { display: none; }.content-surface { padding: 22px 14px 30px; border-left: 0; border-radius: 0; }.form-row { flex-direction: column; } }
@media (prefers-reduced-motion: reduce) { * { scroll-behavior: auto !important; } }
"""


STARTER_APP = """(() => {
  "use strict";
  const client = globalThis.SQLiteCapsuleClient;
  const list = document.querySelector("#items");
  const status = document.querySelector("#status");
  const summary = document.querySelector("#summary");
  const form = document.querySelector("#new-item-form");
  const input = document.querySelector("#new-item");

  function setStatus(message) { status.textContent = message || ""; }

  async function load() {
    const [manifest, items] = await Promise.all([
      client.manifest(),
      client.read("item.list", {}),
    ]);
    summary.textContent = manifest.summary;
    list.replaceChildren();
    for (const item of items) {
      const row = document.createElement("li");
      if (item.completed) row.className = "done";
      const title = document.createElement("span");
      title.textContent = item.title;
      const toggle = document.createElement("button");
      toggle.type = "button";
      toggle.textContent = item.completed ? "Reopen" : "Complete";
      toggle.addEventListener("click", async () => {
        try {
          setStatus("Saving…");
          await client.write("item.toggle", {
            id: item.id,
            completed: item.completed ? 0 : 1,
            updated_at: new Date().toISOString(),
          });
          await load();
          setStatus("Saved in this capsule.");
        } catch (error) { setStatus(error.message); }
      });
      row.append(title, toggle);
      list.append(row);
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const title = input.value.trim();
    if (!title) return;
    try {
      setStatus("Saving…");
      const now = new Date().toISOString();
      await client.write("item.create", {
        id: `item-${crypto.randomUUID()}`,
        title,
        created_at: now,
        updated_at: now,
      });
      form.reset();
      await load();
      setStatus("Saved in this capsule.");
    } catch (error) { setStatus(error.message); }
  });

  load().catch((error) => setStatus(error.message));
})();
"""


STARTER_DOMAIN = """CREATE TABLE item (
    id          TEXT PRIMARY KEY NOT NULL,
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    completed   INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_item_created ON item(created_at, id);
"""


def init_project(
    project: Path,
    *,
    title: str,
    app_id: str,
    summary: str | None = None,
    app_version: str = "0.1.0",
    format_version: str = DEFAULT_FORMAT_VERSION,
    template: bool = False,
) -> dict[str, Any]:
    project = project.expanduser().resolve()
    if project.exists():
        raise ProjectError(f"Refusing to replace existing path: {project}")
    if not title.strip():
        raise ProjectError("title must not be empty")
    if not APP_ID.fullmatch(app_id):
        raise ProjectError("app-id must be a lowercase dotted or hyphenated identifier")
    if format_version not in SUPPORTED_FORMAT_VERSIONS:
        raise ProjectError(f"Unsupported capsule format version: {format_version!r}")
    if template and format_version != "0.3":
        raise ProjectError("--template requires --format-version 0.3")
    timestamp = _utc_now()
    stable_uuid = uuid.uuid5(uuid.NAMESPACE_URL, app_id)
    capsule_id = (
        f"urn:uuid:{stable_uuid}" if format_version == "0.2" else str(stable_uuid)
    )
    summary = summary or f"A self-describing offline {title} application stored in SQLite."
    config = {
        "project_format": PROJECT_FORMATS[format_version],
        "format_version": format_version,
        "capsule_id": capsule_id,
        "title": title.strip(),
        "summary": summary.strip(),
        "app_id": app_id,
        "app_version": app_version,
        "entry_asset": "app/index.html",
        "non_executable_assets": [],
        "permissions_json": {
            "database.read": {
                "required": True,
                "reason": "Read application data through named endpoints.",
            },
            "database.write": {
                "required": True,
                "reason": "Persist user actions through named endpoints.",
            },
            "network": {
                "required": False,
                "value": "none",
                "reason": "The application is fully offline.",
            },
        },
        "created_at": timestamp,
    }
    if format_version == "0.2":
        config["updated_at"] = timestamp
    else:
        config.update(
            {
                "revision_id": str(
                    uuid.uuid5(uuid.NAMESPACE_URL, f"{app_id}/revision/initial")
                ),
                "released_at": timestamp,
                "content_updated_at": timestamp,
                "application_category": "productivity",
                "document_kind": "document",
                "tags_json": [],
                "data_schema_id": f"{app_id}.data",
                "data_schema_version": 1,
            }
        )
        if template:
            config["template_state"] = {"items": "seed"}
    try:
        _write_json(project / "capsule-project.json", config)
        _write_text(project / "domain.sql", STARTER_DOMAIN)
        _write_text(project / "source" / "app" / "index.html", _starter_index(title.strip()))
        _write_text(project / "source" / "app" / "styles.css", STARTER_STYLES)
        _write_text(project / "source" / "app" / "app.js", STARTER_APP)
        _write_json(
            project / "source" / "data" / "seed.json",
            {
                "item": [
                    {
                        "id": "item-welcome",
                        "title": "Replace the starter domain with your application",
                        "completed": 0,
                        "created_at": timestamp,
                        "updated_at": timestamp,
                    }
                ]
            },
        )
        if format_version == "0.3":
            _write_json(
                project / "source" / "data-contract.json",
                {
                    "datasets": [
                        {
                            "id": "items",
                            "role": "user-content",
                            "description": "User-authored application items.",
                            "fork_policy": "copy",
                            "compare_policy": "field",
                            "reconcile_policy": "three-way",
                            "upgrade_policy": "copy",
                            "sensitivity": "normal",
                            "required": 1,
                        }
                    ],
                    "tables": [
                        {
                            "dataset_id": "items",
                            "table_name": "item",
                            "sequence": 10,
                            "primary_key_json": ["id"],
                            "ignored_columns_json": [],
                            "immutable_columns_json": ["id"],
                        }
                    ],
                    "dependencies": [],
                    "migrations": [],
                    "migration_steps": [],
                    "migration_checks": [],
                },
            )
        _write_json(
            project / "source" / "endpoints.json",
            [
                {
                    "name": "item.list",
                    "operation": "read",
                    "sql_text": "SELECT id, title, completed, created_at, updated_at FROM item ORDER BY created_at, id",
                    "parameters_json": {},
                    "result_mode": "rows",
                    "description": "List all items in stable creation order.",
                    "enabled": 1,
                },
                {
                    "name": "item.create",
                    "operation": "write",
                    "sql_text": "INSERT INTO item (id, title, completed, created_at, updated_at) VALUES (:id, :title, 0, :created_at, :updated_at)",
                    "parameters_json": {
                        "id": {"type": "string", "required": True},
                        "title": {"type": "string", "required": True},
                        "created_at": {"type": "string", "required": True},
                        "updated_at": {"type": "string", "required": True},
                    },
                    "result_mode": "changes",
                    "description": "Create one item.",
                    "enabled": 1,
                },
                {
                    "name": "item.toggle",
                    "operation": "write",
                    "sql_text": "UPDATE item SET completed = :completed, updated_at = :updated_at WHERE id = :id",
                    "parameters_json": {
                        "id": {"type": "string", "required": True},
                        "completed": {"type": "integer", "required": True},
                        "updated_at": {"type": "string", "required": True},
                    },
                    "result_mode": "changes",
                    "description": "Set the completion state for one item.",
                    "enabled": 1,
                },
            ],
        )
        _write_json(
            project / "source" / "checks.json",
            [
                {
                    "id": "item-titles-valid",
                    "severity": "error",
                    "description": "Every item has a bounded non-empty title.",
                    "sql_text": "SELECT id FROM item WHERE length(title) NOT BETWEEN 1 AND 120 ORDER BY id",
                    "result_mode": "empty",
                    "expected_json": [],
                }
            ],
        )
        _write_json(
            project / "source" / "prompts.json",
            [
                {
                    "id": "explain-application",
                    "title": "Explain this application",
                    "prompt_text": "Read START_HERE, inspect the manifest and embedded documentation, then explain the domain and safe named operations.",
                    "sequence": 10,
                }
            ],
        )
        _write_json(
            project / "source" / "runbooks.json",
            [
                {
                    "id": "agent-10-trust",
                    "audience": "all",
                    "sequence": 10,
                    "title": "Treat this database as an application package",
                    "body_md": "Inspect and verify the capsule before executing its browser assets or embedded host. Trust is an explicit decision; internal hashes do not authenticate a publisher.",
                    "command_id": None,
                },
                {
                    "id": "agent-20-inspect",
                    "audience": "agent",
                    "sequence": 20,
                    "title": "Extract the hash-checked standalone host",
                    "body_md": "Use Python's standard library to extract the embedded host to a new cache path. Review that executable before running it; extraction does not authenticate the publisher.",
                    "command_id": "extract.embedded",
                },
                {
                    "id": "agent-30-verify",
                    "audience": "agent",
                    "sequence": 30,
                    "title": "Verify before execution",
                    "body_md": "Run the complete verifier and stop if any structural, integrity, endpoint, asset, or application check fails.",
                    "command_id": "verify.embedded",
                },
                {
                    "id": "agent-40-start",
                    "audience": "agent",
                    "sequence": 40,
                    "title": "Run with the Python loopback host",
                    "body_md": "After a deliberate trust decision, start the extracted generic host. It binds to 127.0.0.1 and exposes only declared named endpoints.",
                    "command_id": "start.embedded",
                },
                {
                    "id": "agent-50-confirm",
                    "audience": "agent",
                    "sequence": 50,
                    "title": "Confirm the matching healthy application",
                    "body_md": f"Status must report capsule {capsule_id} and a healthy capsule-http/0.2 loopback URL.",
                    "command_id": "status.embedded",
                },
                {
                    "id": "agent-60-stop",
                    "audience": "all",
                    "sequence": 60,
                    "title": "Stop only the matching host",
                    "body_md": "Use the recorded capsule-specific stop command instead of killing arbitrary Python processes.",
                    "command_id": "stop.embedded",
                },
            ],
        )
        _write_text(
            project / "source" / "docs" / "about.md",
            f"# {title.strip()}\n\n{summary.strip()}\n\nThe browser application is offline and can access SQLite only through declared named endpoints.",
        )
        _write_json(
            project / "source" / "docs.json",
            [
                {
                    "slug": "about",
                    "title": f"About {title.strip()}",
                    "media_type": "text/markdown",
                    "path": "docs/about.md",
                    "sequence": 10,
                }
            ],
        )
    except Exception:
        if project.exists():
            import shutil

            shutil.rmtree(project, ignore_errors=True)
        raise
    return {
        "ok": True,
        "project": str(project),
        "project_format": PROJECT_FORMATS[format_version],
        "format_version": format_version,
        "capsule_id": capsule_id,
        "app_id": app_id,
    }


def _generic_commands() -> list[dict[str, Any]]:
    extraction = (
        "import hashlib,pathlib,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); "
        "r=c.execute('SELECT content,sha256 FROM capsule_asset WHERE path=?',"
        "('bootstrap/capsule_host.py',)).fetchone(); r or sys.exit('embedded host missing'); "
        "b=r[0]; hashlib.sha256(b).hexdigest()==r[1] or sys.exit('embedded host hash mismatch'); "
        "p=pathlib.Path(sys.argv[2])/'capsule_host.py'; p.parent.mkdir(parents=True,exist_ok=True); "
        "f=p.open('xb'); f.write(b); f.close(); print(p)"
    )
    return [
        {
            "id": "extract.embedded",
            "purpose": "Integrity-check and exclusively extract the embedded Python host.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} -c "<embedded extraction>" "{capsule}" "{cache}"',
            "argv_json": ["{python}", "-c", extraction, "{capsule}", "{cache}"],
            "risk_class": "write",
            "success_condition": "A new hash-checked capsule_host.py is written without replacement.",
        },
        {
            "id": "inspect.embedded",
            "purpose": "Inspect identity and bounded inventories with the reviewed extracted host.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" inspect "{capsule}"',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "inspect", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "JSON reports one current manifest and bounded inventories.",
        },
        {
            "id": "instructions.embedded",
            "purpose": "Read the capsule-owned START_HERE runbook without repository access.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" instructions "{capsule}"',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "instructions", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "The capsule identity, runbook, commands, prompts, and documents are printed.",
        },
        {
            "id": "verify.embedded",
            "purpose": "Verify with the reviewed extracted host.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" verify "{capsule}"',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "verify", "{capsule}"],
            "risk_class": "local-execute",
            "success_condition": "The extracted host exits zero and prints ok true.",
        },
        {
            "id": "start.embedded",
            "purpose": "Start with the reviewed extracted host after explicit trust.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" start "{capsule}" --trust-capsule',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "start", "{capsule}", "--trust-capsule"],
            "risk_class": "local-execute",
            "success_condition": "A healthy loopback URL is reported.",
        },
        {
            "id": "status.embedded",
            "purpose": "Confirm a database-only launch.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" status "{capsule}"',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "status", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "Status matches the capsule identity.",
        },
        {
            "id": "stop.embedded",
            "purpose": "Stop a database-only launch.",
            "platform": "any",
            "cwd": ".",
            "command_template": '{python} "{cache}/capsule_host.py" stop "{capsule}"',
            "argv_json": ["{python}", "{cache}/capsule_host.py", "stop", "{capsule}"],
            "risk_class": "local-execute",
            "success_condition": "The matching host is no longer running.",
        },
    ]


def _media_type(path: Path) -> str:
    media_type = MEDIA_TYPES.get(path.suffix.lower())
    if media_type is None:
        raise ProjectError(f"Unsupported app asset type: {path.name}")
    return media_type


def _domain_sql(project: Path) -> str:
    sql_text = (project / "domain.sql").read_text(encoding="utf-8")
    forbidden = {
        "capsule-prefixed objects": r"\bcapsule_[A-Za-z0-9_]*\b",
        "triggers": r"\bCREATE\s+(?:TEMP(?:ORARY)?\s+)?TRIGGER\b",
        "virtual tables": r"\bCREATE\s+VIRTUAL\s+TABLE\b",
        "attached databases": r"\b(?:ATTACH|DETACH)\b",
        "extension loading": r"\bload_extension\s*\(",
        "pragma changes": r"\bPRAGMA\b",
    }
    for label, pattern in forbidden.items():
        if re.search(pattern, sql_text, flags=re.IGNORECASE):
            raise ProjectError(f"domain.sql contains forbidden {label}")
    return sql_text


def _validate_domain_schema(connection: sqlite3.Connection) -> None:
    triggers = [
        str(row[0])
        for row in connection.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name"
        ).fetchall()
    ]
    if triggers:
        raise ProjectError("domain.sql created forbidden triggers: " + ", ".join(triggers))
    virtual_tables = sorted(
        str(row[1])
        for row in connection.execute("PRAGMA table_list").fetchall()
        if str(row[2]).casefold() == "virtual"
    )
    if virtual_tables:
        raise ProjectError(
            "domain.sql created forbidden virtual tables: " + ", ".join(virtual_tables)
        )


def _load_v03_data_contract(project: Path) -> dict[str, list[dict[str, Any]]]:
    path = project / "source" / "data-contract.json"
    value = _load_json(path)
    if not isinstance(value, dict):
        raise ProjectError(f"{path} must contain an object")
    collections = (
        "datasets",
        "tables",
        "dependencies",
        "migrations",
        "migration_steps",
        "migration_checks",
    )
    unknown = sorted(set(value) - set(collections))
    if unknown:
        raise ProjectError(
            "source/data-contract.json has unknown fields: " + ", ".join(unknown)
        )
    result: dict[str, list[dict[str, Any]]] = {}
    for collection in collections:
        rows = value.get(collection)
        if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
            raise ProjectError(
                f"source/data-contract.json field {collection!r} must be an array of objects"
            )
        result[collection] = rows
    if not result["datasets"] or not result["tables"]:
        raise ProjectError("A v0.3 data contract requires datasets and classified tables")
    return result


def _validate_v03_data_contract(
    connection: sqlite3.Connection,
    contract: Mapping[str, list[dict[str, Any]]],
) -> None:
    domain_tables = {
        name
        for row in connection.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name"
        ).fetchall()
        for name in (str(row[0]),)
        if not name.startswith("capsule_") and not name.startswith("sqlite_")
    }
    if len(contract["datasets"]) > 256 or len(contract["tables"]) > 4096:
        raise ProjectError("v0.3 data contract exceeds the host dataset/table ceiling")
    dataset_ids = [row.get("id") for row in contract["datasets"]]
    dataset_id_pattern = re.compile(r"^[A-Za-z_][A-Za-z0-9_.:-]{0,255}$")
    if not all(
        isinstance(value, str)
        and len(value.encode("utf-8")) <= 256
        and dataset_id_pattern.fullmatch(value)
        for value in dataset_ids
    ):
        raise ProjectError("Every v0.3 dataset requires a bounded portable id")
    if len(dataset_ids) != len(set(dataset_ids)):
        raise ProjectError("v0.3 dataset ids must be unique")
    known_datasets = set(dataset_ids)

    tables_per_dataset = {dataset: 0 for dataset in known_datasets}
    for row in contract["datasets"]:
        description = row.get("description")
        if not isinstance(description, str) or not (1 <= len(description.encode("utf-8")) <= 2048):
            raise ProjectError("v0.3 dataset descriptions must be 1..2048 UTF-8 bytes")
        required = row.get("required")
        fork_policy = row.get("fork_policy")
        compare_policy = row.get("compare_policy")
        reconcile_policy = row.get("reconcile_policy")
        if required not in (0, 1):
            raise ProjectError("v0.3 dataset required must be 0 or 1")
        if required == 1 and fork_policy == "omit":
            raise ProjectError("A required dataset cannot use fork_policy 'omit'")
        if reconcile_policy == "three-way" and compare_policy not in ("row", "field"):
            raise ProjectError(
                "Three-way reconciliation requires row or field comparison"
            )

    declared_tables: set[str] = set()
    dataset_by_table: dict[str, str] = {}
    for row in contract["tables"]:
        table = row.get("table_name")
        dataset = row.get("dataset_id")
        if not isinstance(table, str) or table not in domain_tables:
            raise ProjectError(f"Data contract references unknown domain table: {table!r}")
        if table in declared_tables:
            raise ProjectError(f"Domain table is classified more than once: {table!r}")
        if dataset not in known_datasets:
            raise ProjectError(f"Data contract references unknown dataset: {dataset!r}")
        tables_per_dataset[dataset] += 1
        if tables_per_dataset[dataset] > 256:
            raise ProjectError("A v0.3 dataset cannot classify more than 256 tables")
        if len(table.encode("utf-8")) > 256:
            raise ProjectError("v0.3 table names are limited to 256 UTF-8 bytes")
        declared_tables.add(table)
        dataset_by_table[table] = dataset
        column_rows = connection.execute(
            f"PRAGMA table_xinfo({_quote_identifier(table)})"
        ).fetchall()
        columns = {str(column[1]) for column in column_rows}
        if len(column_rows) > 256 or any(
            not name or len(name.encode("utf-8")) > 256 for name in columns
        ):
            raise ProjectError("v0.3 tables/columns exceed the host schema ceiling")
        actual_primary_key = [
            str(column[1])
            for column in sorted(
                column_rows,
                key=lambda column: int(column[5]) if int(column[5]) else 1_000_000,
            )
            if int(column[5])
        ]
        if not actual_primary_key:
            raise ProjectError(
                f"V0.3 domain table {table!r} must declare an explicit primary key"
            )
        primary_key = row.get("primary_key_json")
        if primary_key != actual_primary_key:
            raise ProjectError(
                f"Data contract primary key for {table!r} must be {actual_primary_key!r}"
            )
        if len(primary_key) > 16 or any(len(name.encode("utf-8")) > 256 for name in primary_key):
            raise ProjectError("v0.3 primary keys exceed the host column ceiling")
        xinfo = {str(column[1]): column for column in column_rows}
        table_sql = str(
            connection.execute(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1 COLLATE BINARY",
                (table,),
            ).fetchone()[0]
        )
        integer_alias = (
            len(primary_key) == 1
            and str(xinfo[primary_key[0]][2]).upper() == "INTEGER"
            and "WITHOUT ROWID" not in table_sql.upper()
        )
        if not integer_alias and any(not bool(xinfo[name][3]) for name in primary_key):
            raise ProjectError(
                f"V0.3 primary key columns for {table!r} must be NOT NULL"
            )
        if not integer_alias:
            primary_indexes = [
                row
                for row in connection.execute(
                    f"PRAGMA index_list({_quote_identifier(table)})"
                ).fetchall()
                if str(row[3]).casefold() == "pk"
            ]
            if len(primary_indexes) != 1:
                raise ProjectError(
                    f"V0.3 table {table!r} must expose exactly one primary-key index"
                )
            index_name = str(primary_indexes[0][1])
            key_rows = [
                row
                for row in connection.execute(
                    f"PRAGMA index_xinfo({_quote_identifier(index_name)})"
                ).fetchall()
                if bool(row[5])
            ]
            if [str(row[2]) for row in key_rows] != primary_key or any(
                bool(row[3]) or str(row[4]).upper() != "BINARY" for row in key_rows
            ):
                raise ProjectError(
                    f"V0.3 primary key for {table!r} must use ascending BINARY order"
                )
        for field in ("ignored_columns_json", "immutable_columns_json"):
            names = row.get(field)
            if not (
                isinstance(names, list)
                and len(names) <= 64
                and len(names) == len(set(names))
                and all(isinstance(name, str) and name in columns for name in names)
            ):
                raise ProjectError(
                    f"Data contract {field} for {table!r} must name unique existing columns"
                )
            if len(_json_dump(names).encode("utf-8")) > 16_384:
                raise ProjectError(f"Data contract {field} exceeds the host JSON ceiling")
        if set(row.get("ignored_columns_json", ())) & set(primary_key):
            raise ProjectError(
                f"Data contract ignored columns for {table!r} cannot overlap its primary key"
            )
    missing = sorted(domain_tables - declared_tables)
    if missing:
        raise ProjectError(
            "v0.3 requires every ordinary domain table to be classified: "
            + ", ".join(missing)
        )
    empty_datasets = sorted(
        dataset for dataset, count in tables_per_dataset.items() if count == 0
    )
    if empty_datasets:
        raise ProjectError(
            "Every v0.3 dataset must classify at least one table: "
            + ", ".join(empty_datasets)
        )

    edges: dict[str, set[str]] = {dataset: set() for dataset in known_datasets}
    for row in contract["dependencies"]:
        dataset = row.get("dataset_id")
        dependency = row.get("depends_on_dataset_id")
        if dataset not in known_datasets or dependency not in known_datasets:
            raise ProjectError("Data contract dependency references an unknown dataset")
        if dataset == dependency:
            raise ProjectError("A dataset cannot depend on itself")
        reason = row.get("reason")
        if not isinstance(reason, str) or not (1 <= len(reason.encode("utf-8")) <= 2048):
            raise ProjectError("Dataset dependency reasons must be 1..2048 UTF-8 bytes")
        if dependency in edges[dataset]:
            raise ProjectError("A dataset dependency may be declared only once")
        edges[dataset].add(dependency)
        if len(edges[dataset]) > 64:
            raise ProjectError("A dataset cannot declare more than 64 dependencies")
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(dataset: str) -> None:
        if dataset in visiting:
            raise ProjectError("Data contract dataset dependencies must be acyclic")
        if dataset in visited:
            return
        visiting.add(dataset)
        for dependency in edges[dataset]:
            visit(dependency)
        visiting.remove(dataset)
        visited.add(dataset)

    for dataset in sorted(known_datasets):
        visit(dataset)

    for child_table, child_dataset in sorted(dataset_by_table.items()):
        for foreign_key in connection.execute(
            f"PRAGMA foreign_key_list({_quote_identifier(child_table)})"
        ).fetchall():
            parent_table = str(foreign_key[2])
            parent_dataset = dataset_by_table.get(parent_table)
            if parent_dataset is None:
                raise ProjectError(
                    f"Foreign key from {child_table!r} references an unclassified table"
                )
            if parent_dataset == child_dataset:
                continue
            if parent_dataset not in edges[child_dataset]:
                raise ProjectError(
                    "Every cross-dataset foreign key requires a matching "
                    "child-to-parent dataset dependency"
                )
            on_update = str(foreign_key[5]).upper()
            on_delete = str(foreign_key[6]).upper()
            if on_update not in ("NO ACTION", "RESTRICT") or on_delete not in (
                "NO ACTION",
                "RESTRICT",
            ):
                raise ProjectError(
                    "Cross-dataset foreign keys must use NO ACTION or RESTRICT"
                )


def _seed_domain(connection: sqlite3.Connection, project: Path) -> int:
    seed = _load_json(project / "source" / "data" / "seed.json")
    if not isinstance(seed, dict):
        raise ProjectError("source/data/seed.json must map table names to row arrays")
    for table, rows in seed.items():
        _quote_identifier(table)
        if table.startswith("capsule_"):
            raise ProjectError("Seed data must not target platform tables")
        if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
            raise ProjectError(f"Seed table {table!r} must contain an array of row objects")

    # Deterministically seed referenced parents before children. Cycles still
    # require DEFERRABLE foreign keys, and self-references retain array order.
    remaining = {
        table: {
            str(row[2])
            for row in connection.execute(
                f"PRAGMA foreign_key_list({_quote_identifier(table)})"
            ).fetchall()
            if str(row[2]) in seed and str(row[2]) != table
        }
        for table in seed
    }
    table_order: list[str] = []
    emitted: set[str] = set()
    while remaining:
        ready = sorted(
            table for table, dependencies in remaining.items() if dependencies <= emitted
        )
        if not ready:
            # A cross-table cycle can succeed only when its foreign keys are
            # deferred. Preserve deterministic ordering and let SQLite enforce
            # that contract at commit.
            ready = [min(remaining)]
        for table in ready:
            table_order.append(table)
            emitted.add(table)
            del remaining[table]

    total = 0
    for table in table_order:
        rows = seed[table]
        for row in rows:
            prepared = {
                key: _json_dump(value) if isinstance(value, (dict, list)) else value
                for key, value in row.items()
            }
            _insert_mapping(connection, table, prepared)
            total += 1
    return total


class _DatasetStateDigest:
    def __init__(self) -> None:
        self.digest = hashlib.sha256()
        self.bytes = 0

    def raw(self, value: bytes) -> None:
        self.bytes += len(value)
        if self.bytes > MAX_TEMPLATE_STREAM_BYTES:
            raise ProjectError("Template dataset-state stream exceeds the host byte ceiling")
        self.digest.update(value)

    def u32(self, value: int) -> None:
        self.raw(struct.pack(">I", value))

    def u64(self, value: int) -> None:
        self.raw(struct.pack(">Q", value))

    def text(self, value: str) -> None:
        encoded = value.encode("utf-8")
        self.u64(len(encoded))
        self.raw(encoded)

    def sqlite_value(self, value: Any) -> None:
        if value is None:
            self.raw(b"\x00")
        elif isinstance(value, int):
            if not -(1 << 63) <= value < (1 << 63):
                raise ProjectError("Template dataset integer is outside SQLite signed-64")
            self.raw(b"\x01")
            self.raw(struct.pack(">q", value))
        elif isinstance(value, float):
            if not math.isfinite(value):
                raise ProjectError("Template dataset contains a non-finite REAL")
            self.raw(b"\x02")
            self.raw(struct.pack(">d", value))
        elif isinstance(value, str):
            self.raw(b"\x03")
            self.text(value)
        elif isinstance(value, bytes):
            self.raw(b"\x04")
            self.u64(len(value))
            self.raw(value)
        else:
            raise ProjectError("Template dataset contains an unsupported SQLite value")


def _template_dataset_state(
    connection: sqlite3.Connection,
    config: Mapping[str, Any],
    dataset: Mapping[str, Any],
    tables: list[Mapping[str, Any]],
) -> tuple[int, int, str]:
    stream = _DatasetStateDigest()
    stream.raw(DATASET_STREAM_CONTEXT)
    stream.text(str(config["app_id"]))
    stream.text(str(config["data_schema_id"]))
    stream.u64(int(config["data_schema_version"]))
    stream.text(str(dataset["id"]))
    stream.u32(len(tables))
    stored_rows = 0

    for table in tables:
        table_name = str(table["table_name"])
        primary_key = list(table["primary_key_json"])
        columns = [
            str(row[0])
            for row in connection.execute(
                "SELECT name FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT 257",
                (table_name,),
            ).fetchall()
        ]
        if not columns or len(columns) > 256:
            raise ProjectError("Template dataset table has an invalid column inventory")
        table_rows = int(
            connection.execute(
                f"SELECT count(*) FROM {_quote_identifier(table_name)}"
            ).fetchone()[0]
        )
        stored_rows += table_rows
        if stored_rows > MAX_TEMPLATE_ROWS:
            raise ProjectError("Template dataset rows exceed the host row ceiling")

        stream.u32(int(table["sequence"]))
        stream.text(table_name)
        stream.u32(len(primary_key))
        for column in primary_key:
            stream.text(str(column))
        stream.u32(len(columns))
        for column in columns:
            stream.text(column)
        stream.u64(table_rows)

        selected = ", ".join(_quote_identifier(column) for column in columns)
        ordering = ", ".join(
            f"{_quote_identifier(column)} COLLATE BINARY ASC" for column in primary_key
        )
        observed = 0
        for row in connection.execute(
            f"SELECT {selected} FROM {_quote_identifier(table_name)} ORDER BY {ordering}"
        ):
            observed += 1
            for value in row:
                stream.sqlite_value(value)
        if observed != table_rows:
            raise ProjectError("Template dataset changed during proof generation")
    return stored_rows, stream.bytes, stream.digest.hexdigest()


def _seed_template_state_proof(
    connection: sqlite3.Connection,
    project: Path,
    config: Mapping[str, Any],
) -> int:
    requested = config.get("template_state")
    if requested is None:
        return 0
    if connection.execute(
        "SELECT EXISTS(SELECT 1 FROM capsule_doc WHERE slug = ?1 COLLATE BINARY)",
        (TEMPLATE_STATE_DOC_SLUG,),
    ).fetchone()[0]:
        raise ProjectError(f"Embedded document slug {TEMPLATE_STATE_DOC_SLUG!r} is reserved")

    contract = _load_v03_data_contract(project)
    datasets = sorted(contract["datasets"], key=lambda row: str(row["id"]).encode("utf-8"))
    dataset_ids = [str(row["id"]) for row in datasets]
    if set(requested) != set(dataset_ids):
        raise ProjectError("template_state must classify every data-contract dataset exactly once")

    proof_datasets = []
    total_rows = 0
    total_bytes = 0
    for dataset in datasets:
        dataset_id = str(dataset["id"])
        tables = sorted(
            (row for row in contract["tables"] if row["dataset_id"] == dataset_id),
            key=lambda row: (int(row["sequence"]), str(row["table_name"]).encode("utf-8")),
        )
        rows, stream_bytes, state_sha256 = _template_dataset_state(
            connection, config, dataset, tables
        )
        total_rows += rows
        total_bytes += stream_bytes
        if total_rows > MAX_TEMPLATE_ROWS or total_bytes > MAX_TEMPLATE_STREAM_BYTES:
            raise ProjectError("Template state exceeds the host operation budget")
        disposition = requested[dataset_id]
        if disposition == "empty" and rows != 0:
            raise ProjectError(
                f"Template dataset {dataset_id!r} is declared empty but contains rows"
            )
        proof_datasets.append(
            {
                "dataset_id": dataset_id,
                "disposition": disposition,
                "stored_row_count": rows,
                "state_sha256": state_sha256,
            }
        )

    proof = {
        "profile": TEMPLATE_STATE_PROFILE,
        "app_id": config["app_id"],
        "app_version": config["app_version"],
        "data_schema_id": config["data_schema_id"],
        "data_schema_version": config["data_schema_version"],
        "dataset_state_profile": DATASET_STATE_PROFILE,
        "mutable_platform_state_profile": TEMPLATE_PLATFORM_RESET_PROFILE,
        "datasets": proof_datasets,
    }
    content = _json_dump(proof)
    if len(content.encode("utf-8")) > 256 * 1024:
        raise ProjectError("Template-state proof exceeds the host proof ceiling")
    _insert_mapping(
        connection,
        "capsule_doc",
        {
            "slug": TEMPLATE_STATE_DOC_SLUG,
            "title": "SQLite Capsule authenticated template state",
            "media_type": "application/vnd.sqlite-capsule.template-state+json",
            "content": content,
            "sequence": 0,
        },
    )
    return 1


def _seed_platform(
    connection: sqlite3.Connection,
    project: Path,
    resources: Path,
    config: Mapping[str, Any],
) -> dict[str, int]:
    format_version = str(config.get("format_version", DEFAULT_FORMAT_VERSION))
    if format_version == "0.2":
        manifest = {
            "id": 1,
            "format_id": FORMAT_ID,
            "format_version": format_version,
            "capsule_id": config["capsule_id"],
            "title": config["title"],
            "summary": config["summary"],
            "app_id": config["app_id"],
            "app_version": config["app_version"],
            "entry_asset": config["entry_asset"],
            "runtime_protocol": RUNTIME_PROTOCOL,
            "permissions_json": config["permissions_json"],
            "created_at": config["created_at"],
            "updated_at": config["updated_at"],
        }
        data_contract = None
    else:
        manifest = {
            "id": 1,
            "format_id": FORMAT_ID,
            "format_version": format_version,
            "app_id": config["app_id"],
            "app_version": config["app_version"],
            "entry_asset": config["entry_asset"],
            "runtime_protocol": RUNTIME_PROTOCOL,
            "permissions_json": config["permissions_json"],
            "data_schema_id": config["data_schema_id"],
            "data_schema_version": config["data_schema_version"],
            "minimum_host_profile": V03_HOST_PROFILE,
            "released_at": config["released_at"],
        }
        data_contract = _load_v03_data_contract(project)
        _validate_v03_data_contract(connection, data_contract)
    _insert_mapping(connection, "capsule_manifest", manifest, json_fields=("permissions_json",))
    if format_version == "0.3":
        _insert_mapping(
            connection,
            "capsule_application",
            {
                "id": 1,
                "name": config["title"],
                "description": config["summary"],
                "category": config["application_category"],
                "icon_asset": None,
                "release_notes_doc": None,
            },
        )
        _insert_mapping(
            connection,
            "capsule_instance",
            {
                "id": 1,
                "capsule_id": config["capsule_id"],
                "revision_id": config["revision_id"],
                "title": config["title"],
                "description": config["summary"],
                "document_kind": config["document_kind"],
                "tags_json": config["tags_json"],
                "icon_asset_id": None,
                "cover_asset_id": None,
                "created_at": config["created_at"],
                "content_updated_at": config["content_updated_at"],
            },
            json_fields=("tags_json",),
        )

    app_root = project / "source" / "app"
    asset_paths = sorted(path for path in app_root.rglob("*") if path.is_file())
    reserved = app_root / "capsule-client.js"
    if reserved.is_file():
        raise ProjectError("source/app/capsule-client.js is reserved for the bundled client")
    asset_specs = [
        (f"app/{path.relative_to(app_root).as_posix()}", path, _media_type(path))
        for path in asset_paths
    ]
    asset_specs.extend(
        [
            (
                "app/capsule-client.js",
                resources / "runtime" / "browser" / "capsule-client.js",
                "text/javascript; charset=utf-8",
            ),
            (
                "bootstrap/capsule_host.py",
                resources / "runtime" / "capsule_host.py",
                "text/x-python; charset=utf-8",
            ),
        ]
    )
    non_executable_assets = set(config.get("non_executable_assets", []))
    user_asset_paths = {
        f"app/{path.relative_to(app_root).as_posix()}" for path in asset_paths
    }
    unknown_overrides = sorted(non_executable_assets - user_asset_paths)
    if unknown_overrides:
        raise ProjectError(
            "non_executable_assets references missing app assets: "
            + ", ".join(unknown_overrides)
        )
    for asset_path, source_path, media_type in asset_specs:
        content = source_path.read_bytes()
        _insert_mapping(
            connection,
            "capsule_asset",
            {
                "path": asset_path,
                "media_type": media_type,
                "content": content,
                "sha256": _sha256(content),
                "executable": int(
                    asset_path not in non_executable_assets
                    and source_path.suffix.lower() in {".html", ".js", ".mjs", ".py", ".wasm"}
                ),
                "cache_policy": "no-store",
                "description": f"Embedded source for {asset_path}.",
            },
        )

    commands = _generic_commands()
    for command in commands:
        _insert_mapping(connection, "capsule_command", command, json_fields=("argv_json",))

    runbooks = _load_array(project / "source" / "runbooks.json")
    for runbook in runbooks:
        _insert_mapping(connection, "capsule_runbook", runbook)

    docs = _load_array(project / "source" / "docs.json")
    for document in docs:
        prepared = dict(document)
        relative = prepared.pop("path", None)
        source = _resolve_within(project / "source", relative)
        if not source.is_file():
            raise ProjectError(f"Embedded document not found: {relative!r}")
        prepared["content"] = source.read_text(encoding="utf-8")
        _insert_mapping(connection, "capsule_doc", prepared)

    endpoints = _load_array(project / "source" / "endpoints.json")
    for endpoint_source in endpoints:
        endpoint = dict(endpoint_source)
        steps = endpoint.pop("steps", [])
        _insert_mapping(connection, "capsule_endpoint", endpoint, json_fields=("parameters_json",))
        if not isinstance(steps, list):
            raise ProjectError(f"Endpoint {endpoint.get('name')!r} steps must be an array")
        for step in steps:
            if not isinstance(step, dict):
                raise ProjectError(f"Endpoint {endpoint.get('name')!r} has an invalid step")
            _insert_mapping(
                connection,
                "capsule_endpoint_step",
                {"endpoint_name": endpoint["name"], **step},
            )

    checks = _load_array(project / "source" / "checks.json")
    for check in checks:
        _insert_mapping(connection, "capsule_check", check, json_fields=("expected_json",))

    prompts = _load_array(project / "source" / "prompts.json")
    for prompt in prompts:
        _insert_mapping(connection, "capsule_prompt", prompt)

    dataset_count = 0
    dataset_table_count = 0
    if data_contract is not None:
        for dataset in data_contract["datasets"]:
            _insert_mapping(connection, "capsule_dataset", dataset)
        for table in data_contract["tables"]:
            _insert_mapping(
                connection,
                "capsule_dataset_table",
                table,
                json_fields=(
                    "primary_key_json",
                    "ignored_columns_json",
                    "immutable_columns_json",
                ),
            )
        for dependency in data_contract["dependencies"]:
            _insert_mapping(connection, "capsule_dataset_dependency", dependency)
        for migration in data_contract["migrations"]:
            _insert_mapping(connection, "capsule_migration", migration)
        for step in data_contract["migration_steps"]:
            _insert_mapping(
                connection,
                "capsule_migration_step",
                step,
                json_fields=("definition_json",),
            )
        for check in data_contract["migration_checks"]:
            _insert_mapping(
                connection,
                "capsule_migration_check",
                check,
                json_fields=("definition_json",),
            )
        dataset_count = len(data_contract["datasets"])
        dataset_table_count = len(data_contract["tables"])

    return {
        "assets": len(asset_specs),
        "commands": len(commands),
        "runbooks": len(runbooks),
        "documents": len(docs),
        "endpoints": len(endpoints),
        "checks": len(checks),
        "prompts": len(prompts),
        "datasets": dataset_count,
        "dataset_tables": dataset_table_count,
    }


def build_project(
    project: Path,
    output: Path,
    *,
    resource_root: Path | None = None,
    replace: bool = False,
) -> dict[str, Any]:
    project = project.expanduser().resolve()
    output = output.expanduser().resolve()
    config = _project_config(project)
    resources = _resource_root(resource_root)
    if output.exists() and not replace:
        raise ProjectError(f"Refusing to replace existing capsule: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{output.name}.", suffix=".tmp", dir=output.parent
    )
    os.close(descriptor)
    temporary = Path(temporary_name)
    temporary.unlink(missing_ok=True)
    try:
        connection = sqlite3.connect(temporary)
        try:
            connection.execute("PRAGMA page_size = 4096")
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute("PRAGMA foreign_keys = ON")
            connection.executescript(
                (
                    resources
                    / "format"
                    / f"capsule-v{config.get('format_version', DEFAULT_FORMAT_VERSION)}.sql"
                ).read_text(encoding="utf-8")
            )
            connection.executescript(_domain_sql(project))
            _validate_domain_schema(connection)
            inventory = _seed_platform(connection, project, resources, config)
            inventory["seed_rows"] = _seed_domain(connection, project)
            inventory["template_state_proofs"] = _seed_template_state_proof(
                connection, project, config
            )
            connection.commit()
            foreign_keys = connection.execute("PRAGMA foreign_key_check").fetchall()
            if foreign_keys:
                raise ProjectError(f"Generated capsule has foreign-key errors: {foreign_keys}")
            connection.execute("VACUUM")
        finally:
            connection.close()

        CapsuleDatabase = _load_capsule_host(resources).CapsuleDatabase

        with CapsuleDatabase(temporary, read_only=True) as capsule:
            verification = capsule.verify()
        if not verification["ok"]:
            raise ProjectError(
                "Generated capsule failed verification: " + "; ".join(verification["errors"])
            )
        temporary.replace(output)
        return {
            "ok": True,
            "project": str(project),
            "output": str(output),
            "bytes": output.stat().st_size,
            "sha256": _sha256_file(output),
            "manifest": verification["manifest"],
            "warnings": verification["warnings"],
            "inventory": inventory,
        }
    finally:
        temporary.unlink(missing_ok=True)


def check_project(
    project: Path,
    output: Path,
    *,
    resource_root: Path | None = None,
) -> dict[str, Any]:
    output = output.expanduser().resolve()
    if not output.is_file():
        raise ProjectError(f"Generated capsule not found: {output}")
    current = _sha256_file(output)
    with tempfile.TemporaryDirectory() as directory:
        expected_path = Path(directory) / output.name
        expected = build_project(project, expected_path, resource_root=resource_root)
        expected_hash = expected["sha256"]
    return {
        "ok": current == expected_hash,
        "project": str(project.expanduser().resolve()),
        "output": str(output),
        "current_sha256": current,
        "expected_sha256": expected_hash,
    }


def run_bundled_host(arguments: Iterable[str], *, resource_root: Path | None = None) -> int:
    resources = _resource_root(resource_root)
    host = resources / "runtime" / "capsule_host.py"
    completed = subprocess.run([sys.executable, str(host), *arguments], check=False)
    return completed.returncode


def run_bundled_conformance(
    capsule: Path,
    *,
    resource_root: Path | None = None,
) -> int:
    resources = _resource_root(resource_root)
    checker = resources / "format" / "capsule_conformance.py"
    completed = subprocess.run(
        [
            sys.executable,
            str(checker),
            str(capsule.expanduser().resolve()),
        ],
        check=False,
    )
    return completed.returncode


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    init_parser = subparsers.add_parser("init", help="Create a new reviewable capsule project")
    init_parser.add_argument("project", type=Path)
    init_parser.add_argument("--title", required=True)
    init_parser.add_argument("--app-id", required=True)
    init_parser.add_argument("--summary")
    init_parser.add_argument("--app-version", default="0.1.0")
    init_parser.add_argument(
        "--format-version",
        choices=SUPPORTED_FORMAT_VERSIONS,
        default=DEFAULT_FORMAT_VERSION,
        help="Capsule artifact format to author (default: 0.2)",
    )
    init_parser.add_argument(
        "--template",
        action="store_true",
        help="Author a v0.3 clean-template proof from the deterministic seed state",
    )

    build_parser = subparsers.add_parser("build", help="Build and verify a capsule project")
    build_parser.add_argument("project", type=Path)
    build_parser.add_argument("output", type=Path)
    build_parser.add_argument(
        "--resource-root",
        type=Path,
        help="Development override for the plugin's bundled format/runtime resources",
    )
    build_parser.add_argument("--replace", action="store_true")

    check_parser = subparsers.add_parser("check", help="Check generated capsule freshness")
    check_parser.add_argument("project", type=Path)
    check_parser.add_argument("output", type=Path)
    check_parser.add_argument(
        "--resource-root",
        type=Path,
        help="Development override for the plugin's bundled format/runtime resources",
    )

    host_parser = subparsers.add_parser(
        "host",
        help="Run the bundled inspect, instructions, verify, start, status, or stop host command",
    )
    host_parser.add_argument("--resource-root", type=Path)
    host_parser.add_argument("host_args", nargs=argparse.REMAINDER)

    conformance_parser = subparsers.add_parser(
        "conformance",
        help="Check a capsule with the bundled independent conformance specification",
    )
    conformance_parser.add_argument("capsule", type=Path)
    conformance_parser.add_argument("--resource-root", type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    try:
        if args.command == "host":
            if not args.host_args:
                raise ProjectError("host requires a bundled host command and its arguments")
            return run_bundled_host(args.host_args, resource_root=args.resource_root)
        if args.command == "conformance":
            return run_bundled_conformance(args.capsule, resource_root=args.resource_root)
        if args.command == "init":
            result = init_project(
                args.project,
                title=args.title,
                app_id=args.app_id,
                summary=args.summary,
                app_version=args.app_version,
                format_version=args.format_version,
                template=args.template,
            )
        elif args.command == "build":
            result = build_project(
                args.project,
                args.output,
                resource_root=args.resource_root,
                replace=args.replace,
            )
        elif args.command == "check":
            result = check_project(args.project, args.output, resource_root=args.resource_root)
        else:  # pragma: no cover - argparse prevents this
            raise ProjectError(f"Unknown command: {args.command}")
    except (ProjectError, OSError, sqlite3.DatabaseError, ValueError) as exc:
        print(_json_dump({"ok": False, "error": str(exc)}, indent=2), file=sys.stderr)
        return 2
    print(_json_dump(result, indent=2))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

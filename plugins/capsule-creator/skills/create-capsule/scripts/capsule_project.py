#!/usr/bin/env python3
"""Scaffold and deterministically build reviewable SQLite Capsule projects."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping

PROJECT_FORMAT = "org.sqlite-capsule.source-project/0.2"
FORMAT_ID = "org.sqlite-capsule"
FORMAT_VERSION = "0.2"
RUNTIME_PROTOCOL = "capsule-http/0.2"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
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
    ".png": "image/png",
    ".svg": "image/svg+xml",
    ".txt": "text/plain; charset=utf-8",
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
    if not IDENTIFIER.fullmatch(value):
        raise ProjectError(f"Unsafe SQLite identifier: {value!r}")
    return '"' + value + '"'


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


def _repo_root(explicit: Path | None = None) -> Path:
    candidates: list[Path] = []
    if explicit is not None:
        candidates.append(explicit.expanduser().resolve())
    candidates.extend([Path.cwd().resolve(), *Path.cwd().resolve().parents])
    script = Path(__file__).resolve()
    candidates.extend([script.parent, *script.parents])
    seen: set[Path] = set()
    for candidate in candidates:
        if candidate in seen:
            continue
        seen.add(candidate)
        if (
            (candidate / "format" / "capsule-v0.2.sql").is_file()
            and (candidate / "runtime" / "capsule_host.py").is_file()
            and (candidate / "runtime" / "browser" / "capsule-client.js").is_file()
            and (candidate / "tools" / "capsule.py").is_file()
        ):
            return candidate
    raise ProjectError(
        "Could not locate the SQLite Capsule repository root; pass --repo-root"
    )


def _project_config(project: Path) -> dict[str, Any]:
    project = project.expanduser().resolve()
    missing = [relative for relative in REQUIRED_PROJECT_FILES if not (project / relative).is_file()]
    if missing:
        raise ProjectError("Capsule project is incomplete: " + ", ".join(missing))
    config = _load_json(project / "capsule-project.json")
    if not isinstance(config, dict):
        raise ProjectError("capsule-project.json must contain an object")
    if config.get("project_format") != PROJECT_FORMAT:
        raise ProjectError(f"Unsupported project format: {config.get('project_format')!r}")
    required_strings = (
        "capsule_id",
        "title",
        "summary",
        "app_id",
        "app_version",
        "entry_asset",
        "created_at",
        "updated_at",
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
  <main class="shell">
    <header>
      <p class="eyebrow">SQLite Capsule</p>
      <h1>{title}</h1>
      <p id="summary">Loading the embedded application…</p>
    </header>
    <form id="new-item-form">
      <label for="new-item">Add an item</label>
      <div class="form-row">
        <input id="new-item" name="title" maxlength="120" required autocomplete="off">
        <button type="submit">Add</button>
      </div>
    </form>
    <p id="status" role="status" aria-live="polite"></p>
    <ul id="items" aria-label="Items"></ul>
  </main>
  <script src="/app/capsule-client.js"></script>
  <script src="/app/app.js"></script>
</body>
</html>"""


STARTER_STYLES = """:root {
  color-scheme: light dark;
  font-family: Inter, ui-sans-serif, system-ui, sans-serif;
  background: #10151f;
  color: #eef2ff;
}
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; background: radial-gradient(circle at top, #26334b, #10151f 55%); }
.shell { width: min(720px, calc(100% - 32px)); margin: 0 auto; padding: 64px 0; }
header { margin-bottom: 32px; }
.eyebrow { color: #9db8ff; font-size: .78rem; font-weight: 750; letter-spacing: .16em; text-transform: uppercase; }
h1 { margin: 8px 0; font-size: clamp(2.2rem, 7vw, 4.6rem); line-height: .98; }
#summary { max-width: 58ch; color: #bdc9df; line-height: 1.6; }
form, li { border: 1px solid #3a4860; background: #182131; border-radius: 18px; }
form { padding: 20px; }
label { display: block; margin-bottom: 10px; font-weight: 700; }
.form-row { display: flex; gap: 10px; }
input, button { font: inherit; border-radius: 10px; }
input { flex: 1; min-width: 0; padding: 12px 14px; border: 1px solid #53627a; background: #0f1622; color: inherit; }
button { padding: 11px 16px; border: 0; background: #90adff; color: #0a1020; font-weight: 800; cursor: pointer; }
#status { min-height: 24px; color: #f0c674; }
ul { display: grid; gap: 10px; padding: 0; list-style: none; }
li { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 16px; }
li.done span { color: #8491a7; text-decoration: line-through; }
li button { background: #2b3750; color: #dce6ff; }
@media (max-width: 520px) { .shell { padding: 36px 0; } .form-row { align-items: stretch; flex-direction: column; } }
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
) -> dict[str, Any]:
    project = project.expanduser().resolve()
    if project.exists():
        raise ProjectError(f"Refusing to replace existing path: {project}")
    if not title.strip():
        raise ProjectError("title must not be empty")
    if not APP_ID.fullmatch(app_id):
        raise ProjectError("app-id must be a lowercase dotted or hyphenated identifier")
    timestamp = _utc_now()
    capsule_id = f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, app_id)}"
    summary = summary or f"A self-describing offline {title} application stored in SQLite."
    config = {
        "project_format": PROJECT_FORMAT,
        "capsule_id": capsule_id,
        "title": title.strip(),
        "summary": summary.strip(),
        "app_id": app_id,
        "app_version": app_version,
        "entry_asset": "app/index.html",
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
        "updated_at": timestamp,
    }
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
                    "title": "Inspect identity and capabilities",
                    "body_md": "Confirm the current format identity, requested capabilities, offline design, asset inventory, named endpoints, checks, and embedded documentation.",
                    "command_id": "inspect.repo",
                },
                {
                    "id": "agent-30-verify",
                    "audience": "agent",
                    "sequence": 30,
                    "title": "Verify before execution",
                    "body_md": "Run the complete verifier and stop if any structural, integrity, endpoint, asset, or application check fails.",
                    "command_id": "verify.repo",
                },
                {
                    "id": "agent-40-start",
                    "audience": "agent",
                    "sequence": 40,
                    "title": "Run with the Python loopback host",
                    "body_md": "After a deliberate trust decision, start the generic repository host. It binds to 127.0.0.1 and exposes only declared named endpoints.",
                    "command_id": "start.repo",
                },
                {
                    "id": "agent-50-confirm",
                    "audience": "agent",
                    "sequence": 50,
                    "title": "Confirm the matching healthy application",
                    "body_md": f"Status must report capsule {capsule_id} and a healthy capsule-http/0.2 loopback URL.",
                    "command_id": "status.repo",
                },
                {
                    "id": "agent-60-stop",
                    "audience": "all",
                    "sequence": 60,
                    "title": "Stop only the matching host",
                    "body_md": "Use the recorded capsule-specific stop command instead of killing arbitrary Python processes.",
                    "command_id": "stop.repo",
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
        "project_format": PROJECT_FORMAT,
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
            "id": "inspect.repo",
            "purpose": "Inspect this capsule with the repository host.",
            "platform": "any",
            "cwd": "{repo}",
            "command_template": '{python} tools/capsule.py inspect "{capsule}"',
            "argv_json": ["{python}", "tools/capsule.py", "inspect", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "JSON reports one current capsule manifest and bounded inventories.",
        },
        {
            "id": "verify.repo",
            "purpose": "Verify format, integrity, assets, endpoints, and application checks.",
            "platform": "any",
            "cwd": "{repo}",
            "command_template": '{python} tools/capsule.py verify "{capsule}"',
            "argv_json": ["{python}", "tools/capsule.py", "verify", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "The command exits zero and prints ok true.",
        },
        {
            "id": "start.repo",
            "purpose": "Launch this capsule with the Python loopback host after explicit trust.",
            "platform": "any",
            "cwd": "{repo}",
            "command_template": '{python} tools/capsule.py start "{capsule}" --trust-capsule',
            "argv_json": ["{python}", "tools/capsule.py", "start", "{capsule}", "--trust-capsule"],
            "risk_class": "local-execute",
            "success_condition": "A healthy http://127.0.0.1 URL is reported.",
        },
        {
            "id": "status.repo",
            "purpose": "Confirm the matching detached loopback host.",
            "platform": "any",
            "cwd": "{repo}",
            "command_template": '{python} tools/capsule.py status "{capsule}"',
            "argv_json": ["{python}", "tools/capsule.py", "status", "{capsule}"],
            "risk_class": "read-only",
            "success_condition": "Status reports running true and the expected capsule identity.",
        },
        {
            "id": "stop.repo",
            "purpose": "Stop the matching detached loopback host.",
            "platform": "any",
            "cwd": "{repo}",
            "command_template": '{python} tools/capsule.py stop "{capsule}"',
            "argv_json": ["{python}", "tools/capsule.py", "stop", "{capsule}"],
            "risk_class": "local-execute",
            "success_condition": "Status reports that the matching host is no longer running.",
        },
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


def _seed_domain(connection: sqlite3.Connection, project: Path) -> int:
    seed = _load_json(project / "source" / "data" / "seed.json")
    if not isinstance(seed, dict):
        raise ProjectError("source/data/seed.json must map table names to row arrays")
    total = 0
    for table in sorted(seed):
        _quote_identifier(table)
        if table.startswith("capsule_"):
            raise ProjectError("Seed data must not target platform tables")
        rows = seed[table]
        if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
            raise ProjectError(f"Seed table {table!r} must contain an array of row objects")
        for row in rows:
            prepared = {
                key: _json_dump(value) if isinstance(value, (dict, list)) else value
                for key, value in row.items()
            }
            _insert_mapping(connection, table, prepared)
            total += 1
    return total


def _seed_platform(
    connection: sqlite3.Connection,
    project: Path,
    repository: Path,
    config: Mapping[str, Any],
) -> dict[str, int]:
    manifest = {
        "id": 1,
        "format_id": FORMAT_ID,
        "format_version": FORMAT_VERSION,
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
    _insert_mapping(connection, "capsule_manifest", manifest, json_fields=("permissions_json",))

    app_root = project / "source" / "app"
    asset_paths = sorted(path for path in app_root.rglob("*") if path.is_file())
    reserved = app_root / "capsule-client.js"
    if reserved.is_file():
        raise ProjectError("source/app/capsule-client.js is reserved for the repository client")
    asset_specs = [
        (f"app/{path.relative_to(app_root).as_posix()}", path, _media_type(path))
        for path in asset_paths
    ]
    asset_specs.extend(
        [
            (
                "app/capsule-client.js",
                repository / "runtime" / "browser" / "capsule-client.js",
                "text/javascript; charset=utf-8",
            ),
            (
                "bootstrap/capsule_host.py",
                repository / "runtime" / "capsule_host.py",
                "text/x-python; charset=utf-8",
            ),
        ]
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
                "executable": int(source_path.suffix.lower() in {".html", ".js", ".py"}),
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

    return {
        "assets": len(asset_specs),
        "commands": len(commands),
        "runbooks": len(runbooks),
        "documents": len(docs),
        "endpoints": len(endpoints),
        "checks": len(checks),
        "prompts": len(prompts),
    }


def build_project(
    project: Path,
    output: Path,
    *,
    repo_root: Path | None = None,
    replace: bool = False,
) -> dict[str, Any]:
    project = project.expanduser().resolve()
    output = output.expanduser().resolve()
    config = _project_config(project)
    repository = _repo_root(repo_root)
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
            connection.executescript((repository / "format" / "capsule-v0.2.sql").read_text())
            connection.executescript(_domain_sql(project))
            inventory = _seed_platform(connection, project, repository, config)
            inventory["seed_rows"] = _seed_domain(connection, project)
            connection.commit()
            foreign_keys = connection.execute("PRAGMA foreign_key_check").fetchall()
            if foreign_keys:
                raise ProjectError(f"Generated capsule has foreign-key errors: {foreign_keys}")
            connection.execute("VACUUM")
        finally:
            connection.close()

        if str(repository) not in sys.path:
            sys.path.insert(0, str(repository))
        from runtime.capsule_host import CapsuleDatabase

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
    repo_root: Path | None = None,
) -> dict[str, Any]:
    output = output.expanduser().resolve()
    if not output.is_file():
        raise ProjectError(f"Generated capsule not found: {output}")
    current = _sha256_file(output)
    with tempfile.TemporaryDirectory() as directory:
        expected_path = Path(directory) / output.name
        expected = build_project(project, expected_path, repo_root=repo_root)
        expected_hash = expected["sha256"]
    return {
        "ok": current == expected_hash,
        "project": str(project.expanduser().resolve()),
        "output": str(output),
        "current_sha256": current,
        "expected_sha256": expected_hash,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    init_parser = subparsers.add_parser("init", help="Create a new reviewable capsule project")
    init_parser.add_argument("project", type=Path)
    init_parser.add_argument("--title", required=True)
    init_parser.add_argument("--app-id", required=True)
    init_parser.add_argument("--summary")
    init_parser.add_argument("--app-version", default="0.1.0")

    build_parser = subparsers.add_parser("build", help="Build and verify a capsule project")
    build_parser.add_argument("project", type=Path)
    build_parser.add_argument("output", type=Path)
    build_parser.add_argument("--repo-root", type=Path)
    build_parser.add_argument("--replace", action="store_true")

    check_parser = subparsers.add_parser("check", help="Check generated capsule freshness")
    check_parser.add_argument("project", type=Path)
    check_parser.add_argument("output", type=Path)
    check_parser.add_argument("--repo-root", type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    try:
        if args.command == "init":
            result = init_project(
                args.project,
                title=args.title,
                app_id=args.app_id,
                summary=args.summary,
                app_version=args.app_version,
            )
        elif args.command == "build":
            result = build_project(
                args.project,
                args.output,
                repo_root=args.repo_root,
                replace=args.replace,
            )
        elif args.command == "check":
            result = check_project(args.project, args.output, repo_root=args.repo_root)
        else:  # pragma: no cover - argparse prevents this
            raise ProjectError(f"Unknown command: {args.command}")
    except (ProjectError, OSError, sqlite3.DatabaseError, ValueError) as exc:
        print(_json_dump({"ok": False, "error": str(exc)}, indent=2), file=sys.stderr)
        return 2
    print(_json_dump(result, indent=2))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

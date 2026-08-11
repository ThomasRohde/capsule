# Reviewable capsule source

Use this reference for every project created by `scripts/capsule_project.py`.
The builder and all resources it needs live inside this plugin; a SQLite
Capsule repository checkout is not required.

## Source map

| Path | Ownership |
| --- | --- |
| `capsule-project.json` | Stable identity, version, summary, entry asset, UTC timestamps, permissions |
| `domain.sql` | Application tables, indexes, and views |
| `source/data/seed.json` | Deterministic seed rows keyed by domain table |
| `source/app/` | Offline HTML, CSS, JavaScript, images, JSON, fonts, and WASM |
| `source/endpoints.json` | Named, parameterised reads and writes |
| `source/checks.json` | Bounded read-only application invariants |
| `source/runbooks.json` | Ordered human/agent/runtime instructions |
| `source/prompts.json` | High-value agent tasks carried with the artifact |
| `source/docs.json` + `source/docs/` | Embedded human-readable documentation |

Do not hand-edit a built `.capsule.sqlite`. Change these files and rebuild.

## Stable identity

Keep `capsule_id`, `app_id`, and `created_at` stable across releases. Increment
`app_version` and `updated_at` intentionally. `app_id` is lowercase and dotted
or hyphenated, for example `org.example.field-notes`. `entry_asset` is normally
`app/index.html`.

Declare only capabilities the enabled endpoints require:

```json
{
  "database.read": {"required": true, "reason": "Read notes through named endpoints."},
  "database.write": {"required": true, "reason": "Save note edits."},
  "network": {"required": false, "value": "none", "reason": "Fully offline."}
}
```

Omit `database.write` for read-only applications. `network.value` must remain
`none`.

## Domain model

Use ordinary SQLite tables, indexes, and views. Prefer text IDs that the UI can
create with `crypto.randomUUID()`, explicit UTC timestamps, foreign keys, and
`CHECK` constraints that preserve domain invariants. Add indexes for every
stable sort/filter path. Treat JSON columns as text with `json_valid(...)`.

`domain.sql` must not create `capsule_` objects, triggers, virtual tables,
attached databases, extension calls, or PRAGMA changes. Platform structure is
owned by the bundled format snapshot.

`seed.json` contains complete rows. The builder deterministically orders seeded
tables so referenced parents are inserted before children. Put a self-referenced
parent earlier in that table's row array. Cross-table cycles require every
participating foreign key to be `DEFERRABLE INITIALLY DEFERRED`. Keep seed IDs
and timestamps stable so two builds from unchanged source are byte-identical.

## Named endpoints

The browser never receives a SQLite handle or arbitrary SQL method. Every user
action maps to a declared name.

```json
{
  "name": "note.list",
  "operation": "read",
  "sql_text": "SELECT id, title, updated_at FROM note ORDER BY updated_at DESC, id",
  "parameters_json": {},
  "result_mode": "rows",
  "description": "List notes in deterministic recency order.",
  "enabled": 1
}
```

```json
{
  "name": "note.rename",
  "operation": "write",
  "sql_text": "UPDATE note SET title = :title, updated_at = :updated_at WHERE id = :id",
  "parameters_json": {
    "id": {"type": "string", "required": true},
    "title": {"type": "string", "required": true},
    "updated_at": {"type": "string", "required": true}
  },
  "result_mode": "changes",
  "description": "Rename exactly one note.",
  "enabled": 1
}
```

Parameter types are `string`, `number`, `integer`, `boolean`, or `json`.
Rules may add boolean `required`, boolean `nullable`, and a type-correct
`default`. Unknown parameters and invalid types are rejected. Add SQL-level
length, enum, ownership, and range constraints because parameter types alone
do not express them.

Result modes are `rows`, `row`, `scalar`, and `changes`. A compound write may
add `steps` with sequences 1–16 and optional `required_changes`; all steps and
the generated change-log entry share one transaction. Use it for workflows
that must succeed or roll back as a unit.

## Browser application

Reference assets with root-relative `/app/...` paths. The builder injects
`/app/capsule-client.js`; do not provide a file with that reserved name.

```js
const client = globalThis.SQLiteCapsuleClient;
const manifest = await client.manifest();
const notes = await client.read("note.list", {});
await client.write("note.rename", {id, title, updated_at: new Date().toISOString()});
```

Render database text with `textContent`, not `innerHTML`. Give every write a
visible pending, success, and failure state. After a successful write, reload
authoritative data from the named read rather than assuming the optimistic
state is canonical.

## Checks and embedded guidance

Every important invariant deserves a check. `result_mode: "empty"` with
`expected_json: []` is clearest for violation queries. Error checks block
verification; warnings and info retain review evidence.

Write `START_HERE` runbooks as an ordered trust-to-stop path. Prompts should
describe useful tasks grounded in the actual domain, not generic marketing.
Docs should explain the product, data vocabulary, safe operations, limitations,
and recovery behavior. The built artifact already carries standalone extract,
inspect, verify, start, status, and stop commands.

## Build lifecycle

```text
python <skill>/scripts/capsule_project.py init <project> --title "…" --app-id org.example.app
python <skill>/scripts/capsule_project.py build <project> <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py host instructions <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py host verify <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py conformance <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py check <project> <app.capsule.sqlite>
```

Use `--replace` only after resolving the exact generated target. Use `host
start … --trust-capsule` only after inspecting executable assets and making an
explicit trust decision.

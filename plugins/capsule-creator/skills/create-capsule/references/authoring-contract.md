# Reviewable capsule source

Use this reference for every project created by `scripts/capsule_project.py`.
The builder and all resources it needs live inside this plugin; a SQLite
Capsule repository checkout is not required.

## Source map

| Path | Ownership |
| --- | --- |
| `capsule-project.json` | Stable identity, version, summary, entry asset, UTC timestamps, permissions, executable-asset overrides |
| `domain.sql` | Application tables, indexes, and views |
| `source/data/seed.json` | Deterministic seed rows keyed by domain table |
| `source/data-contract.json` | V0.3-only dataset classification and restricted migration declarations |
| `source/app/` | Offline HTML, CSS, JavaScript, images, JSON, fonts, and WASM |
| `source/endpoints.json` | Named, parameterised reads and writes |
| `source/checks.json` | Bounded read-only application invariants |
| `source/runbooks.json` | Ordered human/agent/runtime instructions |
| `source/prompts.json` | High-value agent tasks carried with the artifact |
| `source/docs.json` + `source/docs/` | Embedded human-readable documentation |

Do not hand-edit a built `.capsule.sqlite`. Change these files and rebuild.

## Stable identity

The default source project contract and output remain v0.2. Its `capsule_id` is
the legacy `urn:uuid:...` identity; keep it, `app_id`, and `created_at` stable
across releases. Increment `app_version` and `updated_at` intentionally.

An explicit v0.3 project uses source contract
`org.sqlite-capsule.source-project/0.3`. It separates stable application
identity from mutable instance identity: `capsule_id` and `revision_id` are
canonical RFC 4122 UUID strings without an `urn:` prefix. Keep `capsule_id` and
`created_at` stable. Advance `revision_id` and `content_updated_at` for instance
content changes, and advance `app_version` and `released_at` for application
releases. Never silently rewrite a signed v0.2 capsule as v0.3.

For both versions, `app_id` is lowercase and dotted or hyphenated, for example
`org.example.field-notes`. `entry_asset` is normally `app/index.html`.

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

## V0.3 lifecycle data contract

Only projects initialized with `--format-version 0.3` carry
`source/data-contract.json`. Every ordinary domain table must occur exactly once
under `tables`, and `primary_key_json` must match the ordered SQLite primary key.
`ignored_columns_json` and `immutable_columns_json` may name only real columns.
Every ordinary table is limited to 256 actual columns, including generated
columns reported by `PRAGMA table_xinfo`, and every SQLite table or column name
is limited to 256 UTF-8 bytes. Each compact serialized ignored/immutable column
array is also limited to 16,384 UTF-8 bytes in addition to its 64-item ceiling.
Dataset dependencies must be acyclic.
Every dataset classifies at least one table. A required dataset cannot use
`fork_policy: "omit"`, and three-way reconciliation requires row or field
comparison. Ignored columns cannot overlap the primary key. Non-`INTEGER
PRIMARY KEY` key columns must be explicitly `NOT NULL`; the workspace accepts
only deterministic ascending BINARY primary-key order.

`compare_policy` is a signed disclosure boundary in the native trusted host:

- `ignore` reports only bounded declared inventory and row counts;
- `summary` adds same/different dataset/table digests but no row classes;
- `row` adds added/removed/changed/unchanged counts and bounded row-digest pages;
- `field` alone permits bounded scalar field projections.

Ignored columns never enter digests or detail. Sensitive datasets remain
counts-only until an explicit trusted-shell reveal. BLOBs are always shown as length/hash
rather than raw bytes. Pick the least-disclosing policy that
supports the application's reconciliation needs; do not use `field` merely for
authoring convenience.

`reconcile_policy` is a separate signed transformation ceiling:

| Policy | Reconciliation meaning |
| --- | --- |
| `ignore` | Never emit or apply operations for this dataset. |
| `forbid` | Reject the whole reconciliation transform. |
| `manual` | Permit only explicit two-way insert/delete/replace/set-fields decisions reviewed from Compare. |
| `three-way` | Permit clean-change classification only with a separately pinned, fully verified ancestor; requires `compare_policy` `row` or `field`. |

Mutable lineage claims do not prove an ancestor. Ignored columns never enter
row/value preconditions. Primary-key and immutable columns cannot be changed by
set-fields, and a three-way immutable-field conflict permits keep-target only.
The host always begins from a private target copy, retains the target capsule
and signed application identity, and publishes a new revision to a new path.
The standalone authoring plugin validates declarations but has no reconcile
executor or Tauri dependency.

Application-compartment expansion is separate from dataset comparison. It is a
fixed, value-free host projection of bounded counts and digests; Capsule
metadata cannot select its tables or columns or turn it into a value disclosure
surface.

```json
{
  "datasets": [{
    "id": "notes",
    "role": "user-content",
    "description": "User-authored notes.",
    "fork_policy": "copy",
    "compare_policy": "field",
    "reconcile_policy": "three-way",
    "upgrade_policy": "copy",
    "sensitivity": "normal",
    "required": 1
  }],
  "tables": [{
    "dataset_id": "notes",
    "table_name": "note",
    "sequence": 10,
    "primary_key_json": ["id"],
    "ignored_columns_json": [],
    "immutable_columns_json": ["id"]
  }],
  "dependencies": [],
  "migrations": [],
  "migration_steps": [],
  "migration_checks": []
}
```

Migration steps are declarations interpreted by the lifecycle host, never SQL
or application endpoints. The only v0.3 operations are `copy_rows`,
`copy_dataset`, and `discard_dataset`; their `definition_json` is explicit data.

### Clean template authoring

Use `init ... --format-version 0.3 --template` only when the deterministic
source seed is an intentional clean release state. This writes a reviewable
`template_state` map in `capsule-project.json`; every dataset must be classified
as `seed` or `empty`. The builder rejects an `empty` dataset that has rows and
derives the reserved `org.sqlite-capsule.template-state` document from the
actual generated database after seeding. It streams every declared table and
stored value in primary-key order under
`org.sqlite-capsule.dataset-state/1`, so authors do not supply counts or hashes.

The generated document is part of the signed application compartment, but an
unsigned build is still an unsigned template candidate. Template creation is
enabled only after native signing authenticates that proof and the lifecycle
host reproduces all dataset digests from the exact verified snapshot. A title,
tag, `document_kind`, or ordinary signed application release is never a clean
template designation.

### Native copy and fork truth table

The native lifecycle host treats authoring policy as signed authority, not UI
advice. Exact and compact duplicates accept verified v0.2 or v0.3 sources,
whether unsigned or fully validly signed; if any signature envelope is present,
the complete inventory must be valid and digest-matching. Fork, template and
selective-fork require signed v0.3 plus the exhaustive data contract.

For fork and selective-fork, `copy` is copied and cannot be weakened to omit;
`omit` is omitted; `prompt` requires a closed include/omit choice (sensitive
include also requires explicit confirmation); and `forbid` rejects the operation.
The first one-source executor deliberately rejects `reset` for fork and selective-fork
because a working source cannot authenticate clean reset rows. Template creation
instead resets every non-forbidden dataset to the exact
state authenticated by the reproduced template-state proof; any `forbid`
dataset rejects template creation. Required datasets and dependency closure are
always enforced, and actual cross-dataset foreign keys must be restrictive and
covered by declarations.

Omitted data is not merely deleted: the host clears the applicable mutable
instance/profile/media, grants, change log, prior lineage and sequence state,
then compacts the owner-private output and proves zero freelist pages before
create-new publication. The authoring plugin does not itself execute lifecycle copies
and never depends on the Tauri client.

For reconciliation, the current native executor additionally requires the
complete foreign-key graph to be acyclic and all update/delete actions to be
`NO ACTION` or `RESTRICT`. It supports restrictive acyclic edges within one
dataset and declared cross-dataset dependencies, orders writes parent-first
and deletes child-first, and checks all foreign keys before publication.
Cascades, `SET NULL`, `SET DEFAULT`, self-references and cycles remain valid
SQLite schema choices but are an explicit `unsupported_operation` limitation
for this reconcile profile.

### Native same-schema application upgrade truth table

M07 consumes two separately verified signed v0.3 files: the working source and
an intentionally clean signed target release. They must have the same
application ID, data-schema ID/version, exact physical schema and signed
dataset/table/key/dependency structure. The target `app_version` must have
strictly greater SemVer 2.0.0 precedence. One exact Ed25519 key ID must be
explicitly accepted and valid in
both complete signature inventories; publisher names, key labels and mutable
lineage never select authority. The target's signed template-state proof must
reproduce before any working rows are applied.

| `upgrade_policy` | M07 same-schema action |
| --- | --- |
| `copy` | Replace the clean target dataset with the working source state. |
| `target` | Keep the authenticated clean target state. |
| `rebuild` | Keep the authenticated clean target state for application rebuild. |
| `omit` | Require and retain an empty target dataset. |
| `migrate` | Reject; restricted data-schema migration starts in M08. |
| `forbid` | Reject the entire upgrade. |

The host begins from a private target copy, preserves the target application,
assets and exact signature inventory, preserves the working capsule/instance
identity and profile, clears grants/change history/old lineage, and publishes a
new revision with `upgraded-from` and `application-release` parents. The
standalone plugin authors and verifies eligible releases but has no upgrade
executor, Tauri surface or publisher-trust store.

## Domain model

Use ordinary SQLite tables, indexes, and views. Every table must declare an
explicit primary key; implicit `rowid` identity is not a portable authoring
contract. Prefer text IDs that the UI can
create with `crypto.randomUUID()`, explicit UTC timestamps, foreign keys, and
`CHECK` constraints that preserve domain invariants. Add indexes for every
stable sort/filter path. Treat JSON columns as text with `json_valid(...)`.

`domain.sql` must not create `capsule_` objects, triggers, virtual tables,
attached databases, extension calls, or PRAGMA changes. Platform structure is
owned by the bundled format snapshot.

App files ending in `.html`, `.js`, `.mjs`, `.py`, or `.wasm` are marked
executable by default. If such a file is pure content, list its `app/...` path in
the optional `non_executable_assets` array in `capsule-project.json`. Overrides
must name existing project assets and cannot include the entry asset.

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
python <skill>/scripts/capsule_project.py init <project-v03> --title "…" --app-id org.example.app-v03 --format-version 0.3
python <skill>/scripts/capsule_project.py build <project> <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py host instructions <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py host verify <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py conformance <app.capsule.sqlite>
python <skill>/scripts/capsule_project.py check <project> <app.capsule.sqlite>
```

Use `--replace` only after resolving the exact generated target. Use `host
start … --trust-capsule` only after inspecting executable assets and making an
explicit trust decision.

# SQLite Capsule formats 0.2 and 0.3

Status: **current repository contracts**. Format 0.2 remains the compatibility
default; format 0.3 adds separate signed application-release, mutable instance,
lineage, and data-contract identities. Hosts dispatch only after matching the
complete fixed format tuple and never rewrite a signed v0.2 capsule in place.

The normative schema is [`../format/capsule-v0.2.sql`](../format/capsule-v0.2.sql).
The independent machine-readable contract is
[`../format/capsule-v0.2.conformance.json`](../format/capsule-v0.2.conformance.json).
The v0.3 normative schema and machine-readable contract are
[`../format/capsule-v0.3.sql`](../format/capsule-v0.3.sql) and
[`../format/capsule-v0.3.conformance.json`](../format/capsule-v0.3.conformance.json).
The Python and native hosts require the exact same format identity.
The Python and native hosts perform the full structural profile. The browser
export host performs the reduced verification profile documented in
[`html-export-contract.md`](html-export-contract.md) before executing assets.

## Format 0.2 identity

A capsule is an ordinary SQLite database with these fixed values:

```sql
PRAGMA application_id = 1129337676; -- 0x4350534c, "CPSL"
PRAGMA user_version = 2;
```

The single `capsule_manifest` row must declare:

| Field | Required value |
| --- | --- |
| `format_id` | `org.sqlite-capsule` |
| `format_version` | `0.2` |
| `runtime_protocol` | `capsule-http/0.2` |

Hosts reject any other tuple for active execution. They do not guess at older or
future semantics and the repository supplies no migration command.

## Format 0.3 identity and compartments

Format 0.3 retains `application_id = 1129337676`, uses `user_version = 3`, and
requires the manifest tuple `org.sqlite-capsule`, `0.3`, and
`capsule-http/0.2`. Exactly one application row identifies a signed application
release, while exactly one instance row carries the mutable capsule UUID,
revision UUID, display profile, timestamps, tags, and instance icon. Dataset
and migration declarations describe a separate data-schema identity. UUIDs,
UTC-second timestamps, JSON shapes, text bounds, row counts, application icons,
and all foreign keys are checked before content is released.

Successful named writes in v0.3 update domain data, generate a fresh host-owned
UUIDv4 revision, set `content_updated_at`, and append the change-log row in the
same transaction. A failed write advances none of them. The application icon is
signed and must resolve to a hash-valid PNG or WebP asset no larger than 512 KiB;
the optional instance icon remains mutable and is subject to the same bounded
media check. Generic inspection validates bytes and metadata without decoding
the image.

## Platform objects

The current format requires these tables:

```text
capsule_manifest
capsule_grant
capsule_asset
capsule_command
capsule_runbook
capsule_doc
capsule_endpoint
capsule_endpoint_step
capsule_check
capsule_prompt
capsule_change_log
```

It also requires the `START_HERE` view. The normative SQL defines exact columns,
primary keys, foreign keys, checks, and indexes. Verification checks the runtime
shape before reading application content. All platform content tables except
`capsule_grant`, `capsule_endpoint_step`, and `capsule_change_log` must be
non-empty.

Triggers and virtual tables are not permitted anywhere in a portable capsule.
Platform tables and SQLite's internal namespace are host-protected from
application endpoints.

`capsule_change_log.endpoint_name` deliberately has no foreign key to
`capsule_endpoint`: audit rows survive endpoint removal or renaming and remain
historical evidence rather than live endpoint declarations.

## Manifest and permissions

Exactly one manifest row exists with `id = 1`. It names the capsule and
application, identifies the entry asset, declares requested capabilities as a
JSON object, and records UTC timestamps.

`capsule_grant` records an explicit `allow`, `deny`, or `prompt` decision
per capability. Requested permissions and grants are inspection data; native
execution policy remains host-owned. A capsule cannot grant itself authority.

## Assets

`capsule_asset` stores all runtime files as bytes with a media type, executable
flag, `no-store` cache policy, and lowercase SHA-256 digest.

Asset paths are relative forward-slash paths. Absolute paths, backslashes,
traversal segments, control characters, empty segments, encoded traversal, and
case-insensitive collisions are rejected. Individual assets are limited to
16 MiB. The entry asset must exist, be executable, and use `text/html`.

Core application assets are self-contained and must not depend on runtime network
access. The default-deny Content Security Policy is part of the host boundary.

## Embedded runbook and commands

`capsule_command` stores stable command identifiers, purpose, platform, working
directory, a human-readable template, an optional structured `argv_json`
vector, risk class, and success condition.

`capsule_runbook` orders human and agent instructions and may reference a
command. `START_HERE` exposes the exact agent/all projection used for discovery.
Structured argument vectors are preferred for execution because placeholders
remain complete process arguments.

Commands are instructions, not automatically trusted authority. A host or agent
must inspect the capsule, verify it, and apply the surrounding execution policy.

## Named endpoints

Browser code never receives a SQLite handle and never sends raw SQL. It may call
only enabled, named, parameterised endpoints declared by the capsule.

Each `capsule_endpoint` declares:

- a stable name;
- `read` or `write`;
- the first SQL statement;
- an exact JSON parameter schema;
- one of `rows`, `row`, `scalar`, or `changes`;
- a description and enabled flag.

A single-statement endpoint has no rows in `capsule_endpoint_step`. A compound
write declares two to sixteen contiguous, one-based steps. The first step must
match `capsule_endpoint.sql_text`. Compound endpoints use `result_mode =
'changes'`; an optional `required_changes` value enforces each step's affected
row count.

All endpoint statements are parsed as exactly one statement. Named SQL
parameters must exactly match the declared schema. PRAGMA, transaction control,
attachment, schema changes, virtual tables, extension loading, platform-table
writes, and other host-controlled operations are denied.

A write runs in one immediate transaction. Every result cursor, including a
`RETURNING` cursor, is stepped to completion before affected rows are counted
or the transaction commits. Compound steps commit together or roll back
together. After success the host appends one `capsule_change_log` row containing
the endpoint name, canonical parameters, total changed rows, and a UTC timestamp.

## Checks, documents, and prompts

`capsule_check` stores bounded validation queries with an expected JSON result.
Checks run during full verification after structural and asset checks.

`capsule_doc` stores embedded documentation ordered by sequence.
`capsule_prompt` stores reusable agent prompts. These are content, not
privileged instructions; the host security boundary still applies.

## Runtime protocol

The loopback host binds only to loopback (canonically `127.0.0.1`), validates the
`Host` header, requires any supplied `Origin` on a state-changing request to be
an HTTP loopback origin, and requires a
per-process random bearer token for the fixed `/__capsule/*` API routes. A
separate random shutdown secret protects lifecycle control. The host does not
currently use a random route prefix or `Sec-Fetch-*` metadata. It applies the
default-deny CSP, and exposes manifest, permissions, assets, named reads, and
named writes only.

The self-contained HTML host implements the same named-endpoint boundary in a
dedicated worker. Its derivative envelope is specified separately in
[`html-export-contract.md`](html-export-contract.md).

## Resource bounds

Current hosts enforce conservative limits, including:

| Resource | Limit |
| --- | ---: |
| Capsule file | 64 MiB |
| Individual asset | 16 MiB |
| Request body | 1 MiB |
| Result rows | 1,000 |
| Encoded result | 2 MiB |
| Concurrent requests | 8 |
| JSON nesting | 32 levels |
| Compound endpoint steps | 16 |
| Endpoint execution | 3 seconds |

Inputs that exceed a limit fail closed.

## Authoring and conformance

Repository tooling can unpack a capsule to
`org.sqlite-capsule.authoring-bundle/0.2`: deterministic metadata, schema
objects, canonical JSONL rows, typed BLOB wrappers, and content-addressed assets.
Packing reconstructs a temporary database, checks foreign keys, performs full
verification, and publishes only on success.

Every table must declare an explicit primary key. Generic unpack refuses a
PK-less table because an implicit SQLite `rowid` is not represented as a declared
column and silently renumbering it would corrupt row identity.

The independent conformance checker consumes the current JSON description using
ordinary SQLite inspection and does not import the runtime verifier:

```bash
python tools/capsule.py conformance capsules/diagram-studio.capsule.sqlite
```

## Optional signed application extensions

A current capsule may add `org.sqlite-capsule.signed-app/0.2`. Its normative
tables are in [`../format/capsule-signed-app-v0.2.sql`](../format/capsule-signed-app-v0.2.sql)
and its independent shape is in
[`../format/capsule-signed-app-v0.2.schema.json`](../format/capsule-signed-app-v0.2.schema.json).

The signature covers immutable platform/application schema and executable
declarations while excluding domain rows, grants, change history, and signature
envelopes. Internal hashes and a valid signature are distinct from local publisher
trust; the host reports them separately. Deterministic interoperability vectors
live under `compatibility/signed-app-v0.2/`.

Format 0.3 uses the distinct
`org.sqlite-capsule.signed-app/0.3` profile, schema
[`../format/capsule-signed-app-v0.3.sql`](../format/capsule-signed-app-v0.3.sql),
and independent envelope description
[`../format/capsule-signed-app-v0.3.schema.json`](../format/capsule-signed-app-v0.3.schema.json).
Its v2 canonical-stream and signature contexts cover application-controlled
schema, release metadata, assets, endpoints, checks, datasets, migration
declarations, and publisher metadata. They deliberately exclude instance
profile and icons, lineage, grants, change history, signature envelopes, and
ordinary domain rows. Interoperability vectors and the mutation matrix live in
`compatibility/signed-app-v0.3/`. The v0.2 stream, contexts, table order, and
golden bytes remain unchanged.

### Authenticated clean-template state

Format 0.3 may designate an intentionally clean seed release with one reserved
signed `capsule_doc` row, slug `org.sqlite-capsule.template-state`, whose
canonical JSON profile is `org.sqlite-capsule.template-state/1`. The record is
exhaustive over the signed dataset contract and binds each dataset's `seed` or
`empty` disposition, stored row count, and streaming
`org.sqlite-capsule.dataset-state/1` SHA-256. The stream length-frames the
application/schema/dataset identity, signed table and primary-key declarations,
all actual columns, and every SQLite value in ascending BINARY primary-key
order. It preserves storage classes and IEEE-754 signed zero, includes ignored
and generated columns, and rejects non-finite values.

The `dataset-state/1` byte grammar is normative. Its complete stream, with no
trailing bytes, is:

```text
raw("SQLite Capsule dataset-state canonical stream v1\0")
text(app_id)
text(data_schema_id)
u64(data_schema_version)
text(dataset_id)
u32(table_count)
repeat table_count times:
  u32(capsule_dataset_table.sequence)
  text(table_name)
  u32(primary_key_column_count)
  repeat primary_key_column_count times: text(primary_key_column_name)
  u32(actual_column_count)
  repeat actual_column_count times: text(actual_column_name)
  u64(stored_row_count)
  repeat stored_row_count times, in row order:
    repeat actual_column_count times: value(sqlite_value)
```

`u32` and `u64` are unsigned big-endian integers of exactly four and eight
bytes. `text(s)` is `u64(len(UTF8(s))) || UTF8(s)` using the raw valid UTF-8
bytes; there is no Unicode normalization. Tables are ordered by signed
`sequence` ascending, then raw UTF-8 table-name bytes in SQLite `BINARY` order.
Actual columns are every `PRAGMA table_xinfo` row in `cid` order, including
ignored columns and both VIRTUAL and STORED generated columns. Primary-key
names are the signed JSON array order. Rows are ordered by that complete key,
each term `COLLATE BINARY ASC`; the v0.3 contract requires this to be a unique,
non-null stable key, so there is no row-order tie. Composite keys compare their
terms from left to right under SQLite's native storage-class ordering and
`BINARY` text comparison.

The value grammar is exactly:

```text
NULL    = 0x00
INTEGER = 0x01 || i64be(value)
REAL    = 0x02 || ieee754_binary64_bits_be(value)
TEXT    = 0x03 || u64(byte_count) || raw_valid_utf8
BLOB    = 0x04 || u64(byte_count) || raw_bytes
```

`i64be` is signed two's-complement big-endian. REAL preserves the exact SQLite
binary64 bit pattern, including negative zero; non-finite values are rejected.
The table header, including a zero row count, is always present for an empty
table. The dataset proof's `stored_row_count` is the sum of all table row
counts. Independent canonical bytes, sizes and SHA-256 values are frozen in
`compatibility/template-state-v1/vectors.json`; the JSON proof envelope is
defined by `docs/plans/capsule-lifecycle/contracts/template-state-v1.schema.json`.

Because ordinary domain rows are excluded from the application signature, the
signed proof is a claim rather than a cached result: the lifecycle host accepts
it only after reproducing every count and digest from the same pinned, verified
snapshot. An ordinary signed release, title, tag or document kind never implies
clean-template status. Independent Python/Rust vectors live under
`compatibility/template-state-v1/`.

### Compact logical-state digest

Compact duplicate uses `org.sqlite-capsule.compact-logical-state/1`. It is a
nested SHA-256 construction, not a serialized capsule format. All length frames
are `u64be(byte_count) || bytes`; all fixed counts are unsigned `u64be`. The
top-level digest receives, in order:

```text
frame("org.sqlite-capsule.compact-logical-state/1")
for pragma in application_id, user_version, auto_vacuum, default_cache_size:
  frame(ASCII pragma name) || i64be(persisted value)
frame("encoding") || frame(raw UTF-8 PRAGMA encoding value)
u64be(sqlite_schema_row_count)
for every sqlite_schema row in BINARY (type, name, tbl_name, sql) order:
  frame(type) || frame(name) || frame(tbl_name)
  0x00                                      if sql is NULL
  0x01 || frame(raw UTF-8 sql)              otherwise
u64be(logical_table_count)
for every logical table in raw UTF-8 BINARY name order:
  frame("table") || frame(table_name)
  u64be(logical_column_count)
  if the table exposes an implicit rowid through an unshadowed SQL alias:
    frame("org.sqlite-capsule.compact.pseudo-rowid/1")
  frame(column_name) for each PRAGMA table_xinfo row in cid order
  u64be(stored_row_count)
  raw 32-byte row SHA-256 values, sorted lexicographically, with duplicates
```

Each row SHA-256 independently receives:

```text
frame("row") || frame(table_name) || u64be(logical_column_count)
if the table exposes an implicit rowid through an unshadowed SQL alias:
  frame("org.sqlite-capsule.compact.pseudo-rowid/1") || INTEGER(rowid)
for each actual column in cid order:
  frame(column_name) || typed_value
```

`logical_column_count` is the `PRAGMA table_xinfo` row count plus one exactly
when the reserved pseudo-rowid field is present. The pseudo-rowid field is the
first logical column in both the table header and every row frame; actual
columns then follow in `table_xinfo.cid` order.

`typed_value` uses the exact NULL/INTEGER/REAL/TEXT/BLOB tags and payload grammar
defined above for `dataset-state/1`. UTF-8 and IEEE-754 bits are not normalized.
Sorting fixed row hashes makes scan order irrelevant while the stored row count
and repeated hashes preserve multiplicity. `sqlite_schema` coverage includes
tables, indexes (including NULL-SQL implicit autoindexes), views and all other
admitted schema objects; only physical `rootpage` is omitted. Logical row
coverage includes every non-`sqlite_*` table, `sqlite_sequence`, and supported
`sqlite_stat1` through `sqlite_stat4`; an unknown `sqlite_stat*` fails closed.
Other SQLite-internal tables are represented by schema only because their rows
are engine-maintained physical state.

Implicit rowid is logical when SQLite exposes it: SQL, views or endpoints may
refer to that value, and VACUUM may renumber it. The profile therefore prepends
the reserved pseudo-column `org.sqlite-capsule.compact.pseudo-rowid/1` and its
signed 64-bit integer value to every row of a rowid table when one of `_rowid_`,
`rowid`, or `oid` remains unshadowed, choosing in that order. A WITHOUT ROWID
table has no such field. If all three names are declared columns, SQLite exposes
no independent internal rowid through SQL, so the profile omits it.

The digest binds `application_id`, `user_version`, `encoding`, `auto_vacuum`
and `default_cache_size`. Physical/volatile `rootpage`, `schema_version`,
`page_count`, `freelist_count`, journal/locking/cache settings and row order are
excluded. `page_size` must nevertheless be identical before and after the
operation. A successful compact output also has `freelist_count = 0`, DELETE
journal mode, no journal/WAL/SHM sidecar, the same exhaustive capsule identity
and signature state, and a freshly reproduced logical digest. Inputs remain
bounded to 64 MiB; the digest is bounded to 4,096 schema objects, 256 columns
per table, 100,000 total rows, 512 MiB of framed logical input, and the one
operation deadline/cancellation budget.

### Compare key and row digests

M05 comparison uses `org.sqlite-capsule.compare-key/1` and
`org.sqlite-capsule.compare-row/1`. These are logical digest profiles, not a
SQLite storage format. Every `frame(text)` is
`u64be(UTF-8 byte count) || raw UTF-8 bytes`; counts are unsigned `u32be`.
Names and TEXT values are valid UTF-8 and receive no Unicode normalization.

The exact key frame is:

```text
frame("org.sqlite-capsule.compare-key/1")
frame(table_name)
u32be(primary_key_column_count)
for each signed-declared primary-key column in order:
  frame(column_name)
  typed_value
```

The exact row frame repeats the key fields under its distinct profile and then
adds every compared column in actual `PRAGMA table_xinfo.cid` order after
removing only signed-declared ignored columns:

```text
frame("org.sqlite-capsule.compare-row/1")
frame(table_name)
u32be(primary_key_column_count)
for each signed-declared primary-key column in order:
  frame(column_name)
  typed_value
u32be(compared_column_count)
for each compared column in canonical order:
  frame(column_name)
  typed_value
```

`typed_value` is exactly:

```text
00                                  NULL
01 || i64be(value)                  INTEGER
02 || IEEE-754 binary64 bits        REAL
03 || u64be(byte count) || UTF-8    TEXT
04 || u64be(byte count) || bytes    BLOB
```

Non-finite REAL values and invalid UTF-8 fail closed. Positive and negative
zero retain different REAL bit patterns. INTEGER `1`, REAL `1.0`, TEXT `"1"`
and BLOB `X'31'` remain distinct. Key and row digests are SHA-256 of their exact
frames. Independent Python/Rust bytes and digests, including composite/mixed
keys, integer bounds, signed zero, combining Unicode and hostile values, are
frozen under `compatibility/compare-row-v1/`.

Comparison reports are summary-first. `compare_policy=ignore` exposes only the
bounded declared inventory and row counts; `summary` adds same/different
canonical table and dataset digests but no row classifications; `row` adds
added/removed/changed/unchanged row counts and paginated row digests; only
`field` permits paginated field projections. Ignored columns never enter row or
field comparison. A sensitive dataset exposes summary counts only until the
trusted shell records an explicit, session-bound reveal action. Before that
action the host does not emit keys, row digests, field digests, storage classes,
or masked field pages. BLOB values are never rendered as bytes: even after a
reveal they project only storage class, byte count and SHA-256.

Detail pagination uses consumed in-memory cursors bound to the exact left/right
file digests, dataset/table position, applied limits and disclosure state. The
trusted shell maps these to random opaque tokens; browser code never supplies a
SQLite identifier, ordering expression, offset, path or SQL fragment.

### Reconciliation review and precondition contract

`reconcile_policy=ignore` contributes no operations, `forbid` blocks a
transform, `manual` permits explicit two-way selections, and `three-way`
permits automatic clean-change classification only when an exact verified
ancestor is supplied. Three-way requires `compare_policy=row` or `field`.
Ignored columns never become operations. Primary-key and immutable columns
cannot be changed by set-fields; an immutable three-way conflict has only the
keep-target resolution.

The first reconciliation executor requires the complete signed-contract
foreign-key graph to be acyclic and every declared SQLite foreign key to use
`NO ACTION` or `RESTRICT` for both update and delete actions. It supports
restrictive acyclic relationships within one dataset and across datasets;
cross-dataset edges must also match the signed dependency declarations.
Insert, replace and set-fields operations are ordered parent-first, deletes are
ordered child-first, and stable canonical key bytes break ties. The private
transaction defers constraints and must finish with an empty
`foreign_key_check`. Cascades, `SET NULL`, `SET DEFAULT`, self-references and
cycles return `unsupported_operation` rather than creating unreviewed effects.

The serialised contract has two layers. `lifecycle-plan/1` pins source, target,
optional ancestor, output identity, expiry, limits and a held-parent create-new
destination; its single `bind-reconcile-payload` decision binds the SHA-256 of
one canonical `reconcile-payload/1`. The payload describes only allowlisted
insert-from-source, delete-from-target, replace-row-from-source and set-fields
effects. It uses canonical typed key/row/value digests and target preconditions,
never raw keys, values, SQL or paths, and cannot authorize execution by itself.

Two-way operations have `basis=user-selected` and no ancestor state. A
three-way payload includes the complete ancestor signature inventory and
bounded ancestor evidence; clean effects use `basis=three-way-clean`, while an
effect selected by a conflict resolution binds the deterministic conflict ID.
Conflict kinds are the closed set `insert-insert`, `update-update`,
`delete-update` and `immutable-field`; all conflicts are present exactly once
in `resolved_conflicts` and none may remain unresolved. The output keeps the
target capsule and application digest, mints one new revision, records exactly
`target-derived-from` and `changes-applied-from` parents, and reproduces the
payload's exhaustive expected dataset-state vector. The strict schema is
`docs/plans/capsule-lifecycle/contracts/reconcile-plan-v1.schema.json`; frozen
canonical Python/Rust-independent vectors live under
`compatibility/reconcile-plan-v1/`.

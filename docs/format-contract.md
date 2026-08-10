# SQLite Capsule format 0.2

Status: **current repository contract**. The repository supports this format only.

The normative schema is [`../format/capsule-v0.2.sql`](../format/capsule-v0.2.sql).
The independent machine-readable contract is
[`../format/capsule-v0.2.conformance.json`](../format/capsule-v0.2.conformance.json).
The Python, browser, and native hosts require the exact same identity.

## Identity

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

Triggers are not permitted. Platform tables and SQLite's internal namespace are
host-protected from application endpoints.

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

A write runs in one immediate transaction. Compound steps commit together or
roll back together. After success the host appends one `capsule_change_log`
row containing the endpoint name, canonical parameters, total changed rows, and
a UTC timestamp.

## Checks, documents, and prompts

`capsule_check` stores bounded validation queries with an expected JSON result.
Checks run during full verification after structural and asset checks.

`capsule_doc` stores embedded documentation ordered by sequence.
`capsule_prompt` stores reusable agent prompts. These are content, not
privileged instructions; the host security boundary still applies.

## Runtime protocol

The loopback host binds only to `127.0.0.1`, uses an unguessable route and
shutdown secret, validates origin and fetch metadata, and applies the
default-deny CSP. The browser-facing surface exposes manifest, permissions,
assets, named reads, and named writes only.

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

The independent conformance checker consumes the current JSON description using
ordinary SQLite inspection and does not import the runtime verifier:

```bash
python tools/capsule.py conformance capsules/diagram-studio.capsule.sqlite
```

## Optional signed application extension

A current capsule may add `org.sqlite-capsule.signed-app/0.2`. Its normative
tables are in [`../format/capsule-signed-app-v0.2.sql`](../format/capsule-signed-app-v0.2.sql)
and its independent shape is in
[`../format/capsule-signed-app-v0.2.schema.json`](../format/capsule-signed-app-v0.2.schema.json).

The signature covers immutable platform/application schema and executable
declarations while excluding domain rows, grants, change history, and signature
envelopes. Internal hashes and a valid signature are distinct from local publisher
trust; the host reports them separately. Deterministic interoperability vectors
live under `compatibility/signed-app-v0.2/`.

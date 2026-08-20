# Capsule 0.2 and 0.3 format and runtime

This is the implementation-facing map for the bundled format snapshots. The
normative machine inputs are the matching pairs
`assets/format/capsule-v0.2.sql` plus
`assets/format/capsule-v0.2.conformance.json`, and
`assets/format/capsule-v0.3.sql` plus
`assets/format/capsule-v0.3.conformance.json`. The bundled runtime and
independent conformance checker dispatch from SQLite `user_version`; they do not
guess from the filename.

V0.2 remains the default authoring output. V0.3 is opt-in through
`capsule_project.py init ... --format-version 0.3`.

## V0.2 identity

- SQLite `application_id`: `1129337676`
- SQLite `user_version`: `2`
- Manifest `format_id`: `org.sqlite-capsule`
- Manifest `format_version`: `0.2`
- Runtime protocol: `capsule-http/0.2`
- Canonical entry instruction surface: `START_HERE`

One database is the canonical application: domain state, UI assets, declared
interfaces, checks, documentation, prompts, runbook, and change history.

## V0.3 identity and lifecycle declarations

- SQLite `application_id`: `1129337676`
- SQLite `user_version`: `3`
- Manifest `format_id` / `format_version`: `org.sqlite-capsule` / `0.3`
- Minimum host profile: `org.sqlite-capsule.host-profile/0.3`
- Runtime protocol: `capsule-http/0.2`
- Canonical entry instruction surface: `START_HERE`

V0.3 deliberately removes mutable document identity and display metadata from
the application manifest. Exactly one `capsule_application` row describes the
application release. Exactly one `capsule_instance` row carries the stable
capsule UUID, current revision UUID, instance title/description/kind/tags, and
instance timestamps. Both UUIDs are canonical lowercase RFC 4122 strings.

The manifest also names `data_schema_id` and positive
`data_schema_version`. `capsule_dataset` declares lifecycle policy;
`capsule_dataset_table` classifies ordinary domain tables with ordered primary
keys and ignored/immutable columns; dependencies are acyclic. Optional migration
rows use only the host-interpreted `org.sqlite-capsule.migration-ops/1`
operations `copy_rows`, `copy_dataset`, and `discard_dataset`. A lifecycle host
must never execute an application endpoint as a migration.

Every actual cross-dataset foreign key must have a matching child-dataset to
parent-dataset dependency declaration. The first lifecycle transform profile
accepts only `NO ACTION` or `RESTRICT` for cross-dataset update/delete actions;
`CASCADE`, `SET NULL` and `SET DEFAULT` fail closed because they could make one
dataset decision mutate another implicitly.

The optional clean-template designation is the canonical signed document
`org.sqlite-capsule.template-state` with profile
`org.sqlite-capsule.template-state/1`. Its exhaustive, BINARY-ordered dataset
records bind actual row counts and `org.sqlite-capsule.dataset-state/1`
streaming digests. Domain rows are excluded from the application signature, so
the host reproduces these claims against the same verified snapshot; the
application signature by itself does not authenticate clean seed state.
The bundled `assets/format/template-state-v1.schema.json` is the
machine-readable proof-envelope contract and must remain byte-identical to the
repository's frozen lifecycle contract. The native host remains authoritative
for reproducing the dataset-state byte stream and accepting template authority.
Semantic fork/template execution also requires every present signature envelope
to be valid and digest-matching. It re-derives signed dataset actions at review,
prepare, transform and post-publication verification; renderer choices cannot
weaken `copy`, bypass `forbid`, invent reset rows or omit dependencies. The
initial one-source profile returns `unsupported_operation` for fork/selective
`reset` policies until a separately retained clean source can supply reset
authority. Template mode uses only the exhaustive signed template-state proof
and rejects any dataset whose signed policy is `forbid`.

Reconciliation is also host-only. `manual` authorizes explicit two-way
insert/delete/replace/set-fields review; `three-way` authorizes automatic clean
classification only with a separately pinned, fully verified ancestor and a
row/field compare policy. `ignore` emits no effects and `forbid` blocks the
transform. The value-free canonical plan binds row/value precondition digests
but contains no raw keys, values, SQL or paths. All conflicts must be resolved,
immutable conflicts keep the target, and execution publishes only a new
target-derived copy while preserving the signed application compartment. The
browser app and this standalone plugin never receive or execute lifecycle
reconciliation authority.

The current native reconciliation profile supports only acyclic foreign-key
graphs whose update and delete actions are `NO ACTION` or `RESTRICT`.
Restrictive acyclic edges within one dataset are supported, as are
cross-dataset edges with matching signed dependencies. The executor applies
writes parent-first and deletes child-first, then requires a clean final
foreign-key check. `CASCADE`, `SET NULL`, `SET DEFAULT`, self-references and
cycles are valid SQLite authoring choices but make reconciliation unavailable
with `unsupported_operation`; this is a host capability limit, not permission
for the plugin to weaken the declared schema.

## Platform objects

| Object | Contract |
| --- | --- |
| `capsule_manifest` | Exactly one identity row with `id = 1` |
| `capsule_asset` | Path-addressed BLOBs, media types, SHA-256, executable flag, no-store policy |
| `capsule_endpoint` | Named read/write SQL plus parameter schema and result mode |
| `capsule_endpoint_step` | Ordered atomic write steps, maximum 16 |
| `capsule_check` | Declared read-only invariants with expected JSON |
| `capsule_runbook` | Ordered instructions linked to optional commands |
| `capsule_command` | Structured argv, risk class, and success condition |
| `capsule_doc` | Ordered embedded documentation |
| `capsule_prompt` | Ordered domain-aware agent tasks |
| `capsule_grant` | Allow, deny, or prompt decisions for requested capabilities |
| `capsule_change_log` | Host-authored audit row for successful writes |
| `START_HERE` | Agent/all runbooks joined to command metadata |

V0.3 adds `capsule_application`, `capsule_instance_asset`,
`capsule_instance`, the dataset and restricted-migration tables, and lineage
tables. A successful v0.3 named write changes domain rows, appends its change-log
row, advances `capsule_instance.revision_id`, and updates
`content_updated_at` in the same transaction. Application release rows and
signed declarations are not mutable application data.

Domain objects must not use the `capsule_` prefix. The builder owns platform
rows and computes asset hashes from source bytes.

## Runtime boundary

The bundled Python host uses only the standard library. It binds to
`127.0.0.1`, requires explicit trust before serving executable assets, uses a
per-process browser token for API calls, and exposes only:

- manifest and permission reports;
- enabled named read endpoints;
- enabled named write endpoints;
- verified `/app/...` assets;
- health and bounded lifecycle operations.

It does not expose arbitrary SQL, files, subprocesses, environment variables,
native IPC, Tauri, or the network. Reads run under SQLite query-only mode and
an authorizer. Writes may insert/update/delete domain tables only; DDL, PRAGMA,
attach/detach, virtual tables, platform-table mutations, and extension loading
are denied. Portable capsule schema is trigger-free and virtual-table-free.
Declared foreign-key actions remain portable: the native host bounds SQLite's
internal cascade machinery to 32 levels even though schema triggers are rejected.

## Resource ceilings

| Resource | Limit |
| --- | ---: |
| Capsule file | 64 MiB |
| One embedded asset | 16 MiB |
| One v0.3 instance icon/cover | 512 KiB |
| One request | 1 MiB |
| Result rows | 1,000 |
| Encoded result | 2 MiB |
| Compound endpoint steps | 16 |
| Concurrent requests | 8 |

Design pagination and summaries before these ceilings become user-visible
errors. Avoid unbounded `SELECT *`; return only fields needed by the screen.

## Verification layers

The bundled verifier checks SQLite identity, the version-specific exact required tables and columns,
foreign keys, manifest identity and permissions, path/media/header safety,
asset sizes and hashes, entry asset presence, endpoint syntax/parameters,
compound steps, v0.3 application/instance metadata and instance media, declared
checks, and bounded output behavior. The separate conformance checker compares
the artifact with the matching data-only specification so a bug in the builder
is less likely to validate itself.

Verification is integrity evidence, not publisher authentication. Never infer
authenticity from a passing internal hash.

## Signed application v0.3 compartment

Publisher signing is a separate create-new release operation under
`org.sqlite-capsule.signed-app/0.3`; authoring or a named write never signs
implicitly. The v2 canonical stream signs the manifest, application display
profile, executable assets/declarations, datasets, restricted migration
declarations, publisher row, and every non-internal schema object. It excludes
instance-profile and instance-icon rows, lineage, grants, change history,
signature envelopes, and ordinary domain rows. Excluding a table's mutable rows
does not exclude its schema: changing domain structure still invalidates the
application signature.

A valid signature authenticates only the exact application compartment and
does not itself trust the publisher or authorize execution. Use the repository's
v0.3 compatibility vector to check that instance/domain mutations preserve the
old signature while any signed row or schema mutation makes the old digest
mismatch. V0.2 uses a distinct immutable canonical/signature context and must
never be rewritten to v0.3 implicitly.

## Cabinet and safe artwork

The native Capsule Cabinet treats all capsule display text and media as
untrusted. Author application and instance icons as PNG or WebP assets only,
with correct declared SHA-256, media type, and dimensions. Each source must be
at most 512 KiB and at most 1024 by 1024 pixels; animation, malformed data,
dimension mismatch, decode allocations above 4 MiB, SVG, remote URLs, and data
references fall back to host artwork. The host decodes from its exact verified
snapshot and re-encodes a static PNG before trusted-shell display.

Do not design authoring flows that depend on Cabinet recents, remembered badges,
or instance artwork as trust evidence. Cabinet entries are last-observed local
hints; a fresh pinned inspection, signature evaluation, capability decision,
and explicit open still control execution. The standalone plugin remains
independent of the native/Tauri client and validates only the portable capsule
contract.

## Content Security Policy

The loopback response is default-deny: self-hosted scripts and styles only,
self-only connections, no frames, objects, forms, or base URI, and no remote
fonts/images. `wasm-unsafe-eval` is present solely so reviewed, same-origin WASM
assets can instantiate; JavaScript `eval()` remains disallowed.

## Standalone artifact lifecycle

Every built capsule embeds `bootstrap/capsule_host.py`. Its runbook first
extracts that BLOB with Python `sqlite3`, verifies its SHA-256, and refuses to
replace an existing cache file. Review the extracted executable, then use it
to inspect and verify. Only after an explicit trust decision may it start the
loopback application. Status and stop commands are capsule-specific; never
kill arbitrary Python processes.

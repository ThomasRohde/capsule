# Capsule 0.2 format and runtime

This is the implementation-facing map for the bundled
`org.sqlite-capsule/0.2` snapshot. The normative machine inputs are
`assets/format/capsule-v0.2.sql` and
`assets/format/capsule-v0.2.conformance.json`.

## Identity

- SQLite `application_id`: `1129337676`
- SQLite `user_version`: `2`
- Manifest `format_id`: `org.sqlite-capsule`
- Manifest `format_version`: `0.2`
- Runtime protocol: `capsule-http/0.2`
- Canonical entry instruction surface: `START_HERE`

One database is the canonical application: domain state, UI assets, declared
interfaces, checks, documentation, prompts, runbook, and change history.

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
are denied.

## Resource ceilings

| Resource | Limit |
| --- | ---: |
| Capsule file | 64 MiB |
| One embedded asset | 16 MiB |
| One request | 1 MiB |
| Result rows | 1,000 |
| Encoded result | 2 MiB |
| Compound endpoint steps | 16 |
| Concurrent requests | 8 |

Design pagination and summaries before these ceilings become user-visible
errors. Avoid unbounded `SELECT *`; return only fields needed by the screen.

## Verification layers

The bundled verifier checks SQLite identity, exact required tables and columns,
foreign keys, manifest identity and permissions, path/media/header safety,
asset sizes and hashes, entry asset presence, endpoint syntax/parameters,
compound steps, application checks, and bounded output behavior. The separate
conformance checker compares the artifact with a data-only specification so a
bug in the builder is less likely to validate itself.

Verification is integrity evidence, not publisher authentication. Never infer
authenticity from a passing internal hash.

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

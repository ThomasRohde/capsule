# SQLite Capsule HTML export contract 0.2

This document defines the first browser-only derivative envelope for a verified
SQLite Capsule. The machine-readable metadata shape is
`format/capsule-html-export-v0.2.schema.json`.

## Identity

- Contract ID: `org.sqlite-capsule.html-export`
- Contract version: `0.2`
- Supported source capsule profile: v0.2
- Required output media type: `text/html; charset=utf-8`

The envelope version and source capsule format version are independent. Exporting
a v0.2 capsule does not create a v0.3 capsule.

## Required blocks

A conforming initial export contains exactly one of each of these elements:

| Element ID | Type | Meaning |
| --- | --- | --- |
| `sqlite-capsule-export-metadata` | `application/json` | Canonical metadata matching the JSON schema |
| `sqlite-capsule-database` | `application/octet-stream` | Base64 of deterministic gzip-compressed capsule bytes |
| `sqlite-capsule-sqlite-js` | `application/octet-stream` | Base64 of deterministic gzip-compressed pinned SQLite JavaScript |
| `sqlite-capsule-sqlite-wasm` | `application/octet-stream` | Base64 of deterministic gzip-compressed SQLite WASM bytes |
| `sqlite-capsule-worker` | `application/octet-stream` | Base64 of deterministic gzip-compressed generic worker source |
| `sqlite-capsule-third-party-notices` | `text/plain` | Auditable upstream notice plus the complete Apache-2.0 license |
| `sqlite-capsule-loader` | executable classic script | Generic top-level loader and save controller |

Payload blocks are inert. Metadata text is escaped so it cannot terminate its
element. A verifier rejects duplicate, missing, malformed, oversized, or
unexpectedly compressed blocks before executing anything.

## Profiles

- `view` permits verified reads and a reader/presentation surface. It denies named
  writes and mutating browser capabilities.
- `interactive` permits verified reads and local, non-persistent UI interactions.
  It denies named writes.
- `editable` permits verified named writes against the in-memory working database
  and exposes explicit HTML revision saving.

The effective permission set is the intersection of the capsule declaration, the
profile ceiling, browser feature availability, and any user grant. A UI which
hides controls does not relax worker enforcement.

## Runtime boundary

The top-level export shell owns identity, provenance, dirty state, save controls,
and the SQLite worker. The capsule entry document runs in a sandboxed child
document. It receives no worker handle, SQLite object, SQL method, database bytes,
or privileged serialisation method.

The reviewed SQLite JavaScript payload remains the exact pinned upstream ES
module. After its digest is verified, the loader applies one fail-closed,
shape-checked conversion of its four `import.meta.url` expressions and export
footer, then starts the result as a blob-backed classic worker. This avoids the
module-fetch CORS path on opaque `file://` origins without fetching, weakening
the worker boundary, or silently accepting a changed upstream module shape.

The public bridge supports only:

- `manifest()`;
- `permissions()`;
- `read(name, parameters)`;
- `write(name, parameters)`.

Messages are bounded and tied to the exact child window and a per-load nonce.
Unknown methods and fields fail closed.

Worker request IDs are positive and strictly increasing. Each message type has
an exact allowed-field set; unknown types, fields, reused IDs, pre-verification
requests, oversized parameters, and invalid endpoint names are rejected. The
loader queues worker operations so save verification/serialisation cannot race a
write. A generation counter preserves dirty state if another committed write
arrives while the shell compresses or writes a revision.

## Verification before execution

The worker imports the database bytes and performs at least:

- application ID and user version checks;
- `PRAGMA integrity_check` and foreign-key checks;
- required table/view/column-presence checks;
- one compatible manifest and validated entry asset;
- asset path, case-collision, media-type, cache-policy, size, and SHA-256 verification;
- endpoint parameter/result declarations and statement compilation;
- trigger and virtual-table policy checks;
- enabled-endpoint to permission consistency checks;
- capsule application checks.

The entry asset is not returned or executed until verification succeeds. Endpoint
execution applies the same parameter, authoriser, transaction, change-log, result,
and resource limits as the compatible Python host.

This browser profile intentionally does not duplicate the full Python/native
shape checker: it does not re-check every declared column's SQLite type,
`NOT NULL`/primary-key ordinal, foreign-key declaration, or the exact
`START_HERE` projection. The exporter verifies the source with the full Python
profile before creating revision 1, and sanctioned endpoints cannot mutate
platform schema. `verify-html` again performs full Python verification over the
embedded database. The reduced in-browser pass is a fail-closed execution gate,
not a claim that all hosts implement byte-for-byte identical verification code.

The exporter resolves only static capsule-local script, stylesheet, image/media,
and poster references from the entry document. Missing, escaping, or remote
references fail export. Tokens which may indicate dynamic loading are reported
but never followed; the sandbox bridge and default-deny CSP remain authoritative.

## Compression and limits

Initial exports use gzip with `mtime=0` and no original filename. SHA-256 is
recorded for uncompressed database bytes, every compressed block, the vendored
runtime source, and the WASM binary. Base64 expansion is bounded before decode.
Decompression writes into an exactly declared buffer and cancels the stream as
soon as output would exceed that declaration, rather than materialising an
unbounded response before checking its length.

The initial implementation supports source capsules up to the generic host limit
of 64 MiB and individual assets up to 16 MiB. A browser may impose a lower
documented policy when required to bound simultaneous compressed, decompressed,
and SQLite heap copies.

The application bridge accepts at most eight concurrent named-endpoint requests,
matching the compatible Python host's request bound. The dedicated worker then
serialises accepted operations. Bridge and worker request IDs are positive and
strictly increasing, message fields are exact, and endpoint result JSON follows
the HTTP bridge's observable JSON-number behavior for signed SQLite integers.

## Provenance and revisions

Revision 1 records the source capsule SHA-256 and has a null parent database
digest. A saved editable revision retains that immutable source digest, increments
`revision`, records the previous revision's database digest as
`parent_database_sha256`, and records the new current and compressed payload
digests.

For deterministic initial exports, metadata `created_at` is the source
manifest's `updated_at`: it identifies the release snapshot rather than the wall
clock time of export. Browser-saved revisions replace it with their actual save
time.

The in-document hashes are component integrity evidence. The exporter and saved
revision writer may report an external whole-HTML digest, but the document does
not contain a recursive digest of itself. Publisher identity remains out of scope.

## Save behavior

Save is available only in `editable` exports and only after a committed named
write. The shell serialises the committed database over a private control path,
re-verifies it, builds a clean envelope revision, and then:

1. uses `showSaveFilePicker()` after explicit user activation when available; or
2. downloads the complete HTML revision through a Blob URL.

Cancellation or failure preserves dirty state. The live child DOM, blob URLs,
focus state, and transient UI are never serialised into the saved file. Reopening
the new HTML must reproduce the edit and provenance chain.

## Offline and origin behavior

The required runtime path works under `file://` and ordinary static hosting with
no network request, service worker, OPFS, SharedArrayBuffer, COOP/COEP header, or
file-picker API. Optional secure-origin features are feature-detected and have a
tested fallback. `network: none` remains effective for all profiles.

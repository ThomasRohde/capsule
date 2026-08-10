# ADR 0013: Browser host and self-contained HTML export contract

- Status: Accepted
- Date: 2026-08-08

## Context

The self-contained export requires one HTML file which opens with only a browser while keeping
SQLite canonical and preserving the named-endpoint security boundary. Local
`file://` documents cannot rely on sibling fetches, service workers, response
headers, origin storage, or user-visible file handles. OPFS is useful under a
secure origin but is not the distributed file, and browser file-picker support is
not portable.

Running SQLite and application code in one JavaScript realm would also weaken the
current host boundary: capsule application code could reach raw database APIs or
cover host identity UI.

## Decision

1. Define `org.sqlite-capsule.html-export` version `0.2` as a derivative envelope
   around an unchanged source capsule. It is versioned independently from the
   SQLite Capsule format.
2. Embed the SQLite WASM JavaScript, WASM binary, compressed database, generic
   worker host, loader, profile, and provenance in the one HTML file. Runtime
   network access is denied.
3. Run SQLite and endpoint enforcement in a dedicated worker. Run the capsule
   application in a sandboxed child document. The parent export shell exposes
   only manifest, effective permissions, named reads, and named writes through a
   validated message bridge.
4. Use an in-memory database as the portable `file://` baseline. OPFS may provide
   an optional recoverable working copy on compatible secure origins, but it is
   never the sole save path or the provenance authority.
5. Define `view`, `interactive`, and `editable` profiles. The host denies database
   writes for both read-only profiles independently of application UI.
6. Saving an editable export creates a new revision of the HTML envelope. A
   user-selected writable file is used only after explicit activation and
   permission; otherwise the revision is downloaded. Opening a local HTML file
   does not imply a handle to that file.
7. Record immutable source capsule provenance plus current and parent database
   payload digests. Integrity hashes do not establish publisher identity.
8. Pin and vendor the official SQLite WASM distribution. Export, verification,
   and tests do not fetch runtime dependencies.

## Consequences

- The Python host and browser host share the v0.2 capsule contract but have
  different transports.
- A host-neutral application client is required; direct loopback URL construction
  is no longer an application assumption.
- The browser host must prove verifier and endpoint parity against shared fixtures.
- `file://` works without origin persistence, but unsaved edits can be lost; the
  shell must show dirty state and make saving explicit.
- Static hosting may offer OPFS and direct file writing after capability detection,
  while the download branch remains fully supported.
- Self-contained HTML is convenient distribution, not a replacement authoring
  source and not proof of publisher authenticity.

# ADR 0026: Host-owned Cabinet cache and safe metadata rendering

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

The Cabinet needs recent paths and bounded previews, but capsule metadata is
untrusted and mutable. Storing it beside grants or treating an icon/title as
publisher identity would weaken the existing trust boundary.

## Decision

Cabinet persistence is host-local, owner-protected, versioned, rebuildable and
logically separate from the trust/revocation database. It may cache a canonical
recent ID, path identity, bounded instance/application labels, a verified
thumbnail digest, last-opened time and last observed status. It stores no trust
decision, capability grant, raw comparison value, operation credential or
canonical capsule metadata. Opening always performs fresh inspection.

The trusted shell renders profile text as text. If restricted Markdown is added,
raw HTML, images and automatic links remain disabled. Application identity,
publisher state, instance profile and file identity are visibly distinct.
Every text ceiling is enforced over UTF-8 bytes by the host in addition to any
SQLite/JSON-schema character constraint.

All application icons, instance icons and instance covers rendered by the
trusted shell use PNG or WebP only. A signed application icon path must resolve
to a signed `capsule_asset`; instance pointers resolve to
`capsule_instance_asset`. Signing authenticates bytes but does not make media
decoding safe. Compressed bytes are limited to 512 KiB, declared and decoded
dimensions to 1024 by 1024, and decoded pixels/memory are bounded before
allocation. The host checks the content hash and actual media dimensions and
serves only a selection-bound trusted-shell token or a decoded/re-encoded
derivative. Invalid media uses a deterministic host fallback. SVG and remote
references are not accepted.

The Cabinet, image token and lifecycle reports are available only to the
bundled `main` Tauri WebView through exact commands/capabilities. The standalone
raw Wry renderer remains outside Tauri's capability graph and receives none of
them.

## Consequences

- Cabinet cache loss never loses user data or trust decisions.
- Metadata spoofing cannot replace publisher/signature status.
- M01 defines the safe instance asset pointers; M03 adds storage/UI and negative
  window/capability tests.

# ADR 0017: Native runtime and custom-protocol bridge

## Status

Accepted on 2026-08-08 for the product-independent native runtime and bridge.
The implementation and Windows automated tests are complete. Live unlocked
Windows UI evidence and equivalent WKWebView/WebKitGTK acceptance remain part
of ADR 0014's cross-platform gate; file-handle identity pinning and external
replacement handling are defined by ADR 0018.

## Context

ADR 0014 selected a raw Wry child because a Tauri-created child inherited the
generic Tauri bootstrap. ADR 0016 established that a valid signature, publisher
trust, and effective capabilities are separate decisions and that executable
assets must remain locked until all required decisions allow.

The native host now needs to run the same generic capsule application as the
Python and SQLite WASM hosts. Moving Diagram Studio queries or behavior into
Rust would violate the format boundary. Exposing SQLite, SQL text, database
bytes, paths, Tauri commands, or generic IPC to the child would violate the
privilege boundary.

## Decision

### Independent verified runtime

Add `capsule-runtime` as an independent Rust implementation of the current v0.2
machine conformance and named-endpoint contracts. Construction requires shared
launch evidence plus an `executable_allowed` policy decision, re-inspects the
source, opens it read-only or read/write according to the effective database
grant, hardens SQLite, and completes conformance before returning assets or
endpoints.

The runtime validates required/optional objects, exact columns and keys,
discovery views, foreign keys, trigger absence, asset paths/hashes/media/cache
policy, command argument shapes, endpoint declarations and compilation, and
every declared check. SQLite extension loading is disabled;
`trusted_schema=OFF`, foreign keys, limits, a busy timeout, and a progress
deadline are applied.

Named reads and writes accept only declared endpoint names and typed argument
objects. The runtime reconciles exact placeholders, permits one SQL statement
per step, uses the SQLite authorizer, applies query-only reads, bounds results
to 1,000 rows and 2 MiB, and rejects result BLOBs. Compound writes have at most
16 steps, run in one immediate transaction, enforce exact row-count
preconditions, deny writes to platform tables, and add exactly one
`capsule_change_log` record. Failure rolls the whole operation back.

No public runtime method accepts arbitrary SQL, a database handle, a
filesystem path, trust state, backup state, or update state.

### Child origin and protocol

After the launch decision allows execution, create a fresh 32-byte OS-random
base64url session and a `ProtocolSession`. Serve verified assets and RPC from
`capsule://app`; WebView2 maps that custom scheme to the isolated
`http://capsule.app` origin. Before activation, the origin serves only the
host-owned locked probe and rejects session/RPC access.

The child discovers the session on its own origin and sends versioned JSON
requests containing the secret, an exactly increasing sequence, a unique
bounded ID, one of four method names, and exact parameters. The only operations
are `manifest`, `permissions`, named `read`, and named `write`. Successful and
accepted-error responses repeat version, sequence, and ID but never the secret;
pre-authentication parser failures return a stable bounded error without
echoing attacker fields.

Verified assets have safe decoded relative paths, their declared hashes are
rechecked, and responses use `no-store`, a default-deny CSP, Permissions Policy,
`nosniff`, no-referrer, and same-origin resource policy. Navigation is confined
to the custom origin; new windows are denied. The child remains incognito with
clipboard and developer tools disabled and has no Wry IPC handler or Tauri
initialization.

The shared browser client prefers an injected HTML-export bridge, then the
native custom protocol, then the existing loopback HTTP topology. All three
present the same host-neutral manifest/permissions/read/write API.

## Consequences

- Failed inspection, conformance, trust, required capability, session, path,
  endpoint, authorizer, row-count, timeout, or result-limit checks release no
  additional authority and have no partial write effect.
- Capsule applications remain portable; the Rust runtime contains no Diagram
  Studio table names, routes, scenes, shape logic, or UI behavior.
- The session secret authenticates requests from the activated child protocol
  context; it is not publisher identity and does not grant capabilities beyond
  the already computed launch decision.
- `VerifiedCapsule::open` currently re-inspects by path and then opens SQLite.
  The lifecycle layer pins and compares the opened file handle/identity to close the
  remaining replacement window before native distribution.
- Clipboard, user-selected files, download, fullscreen, camera, and microphone
  stay denied in the renderer until capability-specific host mediation exists.
- Windows automated runtime/bridge tests do not substitute for the still-open
  unlocked UI, WKWebView, WebKitGTK, accessibility, and clean-install evidence.

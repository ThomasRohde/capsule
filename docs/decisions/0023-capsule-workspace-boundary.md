# ADR 0023: Product-independent capsule workspace boundary

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

The live `capsule-lifecycle` crate owns source identity, filesystem
classification, writer leases, backup/checkpoint inventory and restore. Adding
comparison, fork, reconciliation and upgrade policy there would mix document
transformation semantics with recovery mechanics.

The raw application renderer currently reaches only the verified runtime's
manifest, permissions and named read/write protocol. It must not acquire a
filesystem or lifecycle service by dependency accident.

## Decision

Add a product-independent `capsule-workspace` crate in M02. It owns v0.3
profiles, data contracts, lineage, lifecycle plans, bounded comparison,
create-new transforms, output validation and publication reports.

Dependency direction is one way:

```text
capsule-core -----------\
capsule-crypto ----------+--> capsule-workspace --> capsule-cli
capsule-lifecycle -------+                       --> trusted Tauri shell
capsule-runtime verify --/
```

`capsule-workspace` consumes read-only pinned sources and reusable verification
primitives. It never invokes named application endpoints, opens a writable
input, persists trust, performs network discovery, knows Diagram Studio tables,
or exposes a handle to the raw renderer. `capsule-lifecycle` remains responsible
for pinning, leases, backup/recovery and platform file primitives.

The CLI and trusted Tauri commands use the same planner/executor. API and JSON
reports use validated newtypes, bounded collections and the stable lifecycle
error catalogue. No UI path receives SQL or unchecked table, column or
filesystem strings.

## Consequences

- M02 adds the crate and compile-time dependency tests before Tauri lifecycle
  commands exist.
- Verification code may be factored into a lower shared module, but no existing
  crate may depend back on `capsule-workspace`.
- The raw-renderer negative tests inspect both the dependency graph and Tauri
  capability/window registration.


# Security and trust model

## Security objective

Lifecycle features must increase user control without creating a second,
less-reviewed execution path around the existing native trust boundary.

## Trust zones

| Zone | Trust | Permitted lifecycle role |
| --- | --- | --- |
| Rust host core | Trusted | Inspect, plan, transform, validate and publish |
| Bundled Tauri shell | Trusted presentation | Request exact host operations and render bounded reports |
| Raw Wry renderer | Untrusted capsule code | None |
| Source capsule | Untrusted input | Read-only, pinned and bounded |
| Target release capsule | Untrusted until verified | Read-only source of signed application and migration declarations |
| Output temporary file | Host-controlled | Mutable until complete verification |
| Published output | User-owned | Created only after all gates pass |
| Host-local stores | Protected | Trust, audit, recents, operation/session state |

## Non-negotiable invariants

### SI-1: No input mutation

Every input connection is opened read-only. Files are pinned against
replacement using the repository's existing lifecycle mechanisms. No lifecycle
operation writes sidecars beside an input.

### SI-2: No execution during lifecycle inspection

Overview, copy planning, comparison and upgrade planning do not execute:

- application assets;
- endpoints;
- prompts or runbook commands;
- application-supplied SQL outside already defined read-only structural checks;
- migration code.

Declared application checks are executed only through the existing verifier
under its authoriser and bounds.

### SI-3: Create-new output

Destination selection is host-owned. The final path must not exist. Work occurs
in a private owner-protected temporary path. Publication is atomic where the
platform permits and fails closed otherwise.

### SI-4: Signature preservation

Fork, profile edits, lineage and data reconciliation must not mutate any byte or
schema object in the signed application compartment. The computed application
digest of an output must equal the expected release digest.

### SI-5: Safe metadata rendering

All metadata is bounded UTF-8 text. The host escapes it. Descriptions support
plain text or a deliberately restricted Markdown renderer with raw HTML,
scripts, images and automatic links disabled.

Icons/covers:

- no remote references;
- PNG or WebP only in the first release;
- compressed size at most 512 KiB;
- decoded dimensions at most 1024×1024;
- decoded pixels and memory bounded before allocation;
- hash checked before decode;
- no SVG until a separately reviewed sanitisation profile exists;
- deterministic fallback when invalid.

### SI-6: Renderer isolation

No lifecycle Tauri command, event emit permission, filesystem path, comparison
session, output handle or migration report is available to the raw Wry window.
Native capability files and integration tests must prove this negatively.

### SI-7: Bounded comparison

Comparison is streaming and paginated. Default reports contain counts, digests
and bounded labels rather than full sensitive values. Detail pages have strict
row, byte and time limits. Sensitive datasets require explicit disclosure.

### SI-8: Plan rebinding

Execution rechecks source identity, length, hash, capsule/revision identity,
application digest, schema version and destination freshness. Any mismatch
returns `stale_plan`.

Rebinding is coupled to one plan/execute stable-snapshot protocol. The host
rejects WAL/shared-memory/rollback-journal sidecars, copies pinned raw database
bytes to private create-new storage without opening SQLite against the source,
and binds the exact snapshot SHA-256 plus verified logical identities in the
plan. Execution repeats and compares that snapshot. Source identity, length,
main-file SHA-256 and sidecar absence are checked around capture and immediately
before publication. The held pin is not assumed to block same-object writes on
Windows; a same-size or change-capture-restore ABA mutation fails closed and
discards every unpublished artefact.

The destination token binds the reparse-free canonical parent filesystem
identity and validated leaf. Temporary creation and no-replace publication are
relative to a held parent handle; parent substitution, junctions, symlinks,
reparse points, alternate data streams and source aliases fail closed.

### SI-9: Restricted migrations

Migration declarations are signed as application metadata but signatures do not
make arbitrary code safe. The engine accepts only allowlisted declarative
operations over typed values. It never loads extensions, invokes scripts,
attaches an untrusted database to a writable connection, or accepts SQL from
the UI.

### SI-10: Publisher continuity

An application upgrade requires:

- same `app_id`;
- target signature valid;
- same accepted signing key for the first implementation; or
- a future explicit, signed and host-supported key-delegation chain.

A different publisher is an import into another application and is outside this
programme.

## Threats and controls

| Threat | Control |
| --- | --- |
| Capsule spoofs a trusted app title/icon | Separate signed app identity, publisher status and untrusted instance profile visually |
| Source replaced after preview | Pinned identity plus digest rebinding at execute |
| Crafted huge icon decompresses explosively | Compressed and decoded limits; safe decoder boundary |
| Compare leaks sensitive data | Summary-first, dataset sensitivity, explicit reveal, bounded pages |
| Malicious schema exhausts compare | Object, row, byte and deadline limits; primary-key requirement |
| Fork invalidates signature | v0.3 compartment split and output digest equality |
| "Blank" fork deletes required seed data | Signed dataset/template policy; no inferred deletion |
| Merge corrupts target | Apply only to new target-derived copy; transactional validation |
| Migration alters application compartment | Target release is copied clean; migration writes domain/instance only |
| Migration DSL becomes a programming language | Small allowlist, no loops, no SQL, bounded rows and transforms |
| Stale or ambiguous migration path | Exact source/target schema IDs, unique acyclic path |
| Raw app invokes lifecycle | No command registration/capability; negative window tests |
| Output appears complete after crash | In-progress marker/private temp; publish only after sync and verification |
| Audit stores private row values | Redacted operation summaries and hashes by default |

## Required adversarial tests

- symlink, hard-link/reparse and replacement races;
- source changes between plan and execute;
- same-object, same-size source writes during snapshot, transformation and the
  final prepublication window;
- change-capture-restore ABA writes, including domain-only row changes;
- source WAL/shared-memory/rollback-journal state and attempted sidecar creation;
- destination parent substitution, junction/reparse traversal and alternate
  data-stream leaves;
- destination created by another process before publish;
- invalid/oversized image metadata and decompression bombs;
- unknown platform tables and future format versions;
- duplicate dataset/table declarations;
- missing primary keys and unstable collations;
- malformed, recursive or ambiguous migration graph;
- migration writes outside declared datasets;
- app digest changes after fork/reconcile/upgrade;
- target release with increased permissions;
- source/target from different publishers;
- raw renderer attempts every lifecycle command;
- process termination at each durable output stage.

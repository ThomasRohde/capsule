# ADR 0024: Lifecycle plan, execute and create-new publication

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

The live signing path already demonstrates a safe shape: inspect a read-only
source, prepare a same-directory private temporary copy, rebind the reviewed
digest, verify the result and publish with `persist_noclobber`. The generic
authoring packer, by contrast, intentionally has an explicit replacement mode
and therefore is not a lifecycle publication primitive.

Lifecycle transformations must survive source races, destination races and
crashes without mutating or partially replacing a user's input.

## Decision

Every transforming lifecycle operation has separate plan and execute phases.
Planning pins inputs read-only and returns an immutable bounded plan that binds
filesystem identity, byte length, file SHA-256, the SHA-256 of an exact private
input snapshot, logical identities, application digest, schema
identity/version, choices, limits, destination reservation and expiry.

Plans use `org.sqlite-capsule.lifecycle-plan/1`. Their canonical JSON is UTF-8
without BOM, has no insignificant whitespace, rejects duplicate keys and
floating-point numbers, preserves array order, uses minimal integers and JSON
escaping, performs no Unicode normalization, and orders object keys by Unicode
scalar value. Timestamps are exact UTC seconds. The `plan_digest` member is
omitted while SHA-256 is calculated. M02 must publish Python/Rust byte vectors,
including non-BMP keys, before this profile is used as a stable API.

Execution accepts only a current-host prepared plan and one-use destination
token. It rechecks every bound input and refuses `stale_plan` on any mismatch.
The recheck is not a sufficient read-consistency boundary: the live Windows
pin deliberately permits another process to keep writing the same file.

Planning and execution use the same stable-snapshot procedure. The host rejects
an input with an adjacent WAL, shared-memory or rollback-journal file instead
of opening the source through SQLite or recovering it in place. While holding
the pinned source, it copies raw main-database bytes into private create-new
storage and calculates `snapshot_sha256`, with source identity, length, main-file
SHA-256 and sidecar absence checked before and after capture. Only the private
snapshot is opened through SQLite, exhaustively verified and used to derive the
logical bindings. The plan binds that exact snapshot digest.

Execution repeats the capture and requires the new `snapshot_sha256` and every
logical binding to equal the plan. All subsequent reads for the transform come
only from that private snapshot, never from the live path or handle.
Immediately before publication, execution rechecks source identity, length,
main-file SHA-256 and sidecar absence once more. Any disagreement returns
`stale_plan`, destroys the unpublished output and snapshots, and publishes
nothing. This detects same-object, same-size and change-capture-restore ABA
writes; file identity alone is not treated as a content lock. A future profile
may support a complete WAL-state snapshot only after it has an equally exact,
no-source-sidecar capture contract.

Execution creates a private temporary output with create-new semantics,
transforms only that output from the verified snapshots, syncs it, and validates
all of the following before publish:

- SQLite integrity and foreign keys;
- the exact format structural profile;
- signature envelope and expected application digest;
- declared application checks under the existing read-only authoriser/bounds;
- operation-specific identity, data-contract, lineage and migration rules.

The one-use destination token binds the canonical parent directory's stable
object identity (platform, volume/device and file ID/inode; never mutable
directory timestamps) and a single validated leaf name. The host rejects alternate data
streams, symlink/junction/reparse traversal, source aliases and a changed
parent. It creates the private output and performs final no-replace publication
relative to the held parent-directory handle, then revalidates that same parent
identity. Path re-resolution is not the authority after the token is minted.

Publication uses a no-replace primitive equivalent to the signing path's
`persist_noclobber`, followed by containing-directory sync where supported and
a fresh reopen/verification. An existing or aliased destination fails. The
authoring packer's `--replace`, `Path.replace`, in-place `VACUUM`, and restore
replacement are not available to lifecycle execution.

If the final reopen fails after a successful no-replace rename, the host records
`postpublish_verification_failed`, writes a host-owned quarantine marker or moves
the exact output to a create-new quarantine name when the platform can prove
that transition, and never reports success. It does not replace another path or
fall back to using the suspect output.

Private temporary and in-progress state is removed or quarantined on failure;
inputs are never repaired. A stale plan is never silently recomputed.

A serialised CLI plan is untrusted input, not authority. The executor validates
its schema, canonical digest, operation semantics, policy choices and every
source/output binding again; a caller who edits and recomputes a digest gains no
ability outside the signed contract and host policy. Tauri execution uses only
an in-memory opaque plan handle and one-use confirmation nonce, never a
renderer-supplied plan JSON or path.

## Consequences

- Copy, fork, reconcile and upgrade share one publication state machine and
  stable safe error codes.
- Operation tests hash every source before and after and inject failures at
  every durable stage.
- Race tests perform same-object, same-size writes before snapshot capture,
  change-capture-restore ABA writes during capture, mutations during transform,
  and writes immediately before publication; every case fails closed and never
  publishes mixed or unreviewed state.
- Destination tests substitute the parent and exercise symlink, junction,
  reparse-point, alternate-data-stream and source-alias cases.
- A post-publication reopen failure is reported as quarantined, never success.

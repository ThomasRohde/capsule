# Compatibility and rollout

## Version support matrix

| Source | Open/run | Duplicate | Fork | Compare | Reconcile | Upgrade |
| --- | --- | --- | --- | --- | --- | --- |
| v0.2 unsigned | Existing policy | Yes | Not signature-preserving; authoring conversion only | Metadata/schema or legacy adapter | No generic v0.2 reconcile | To signed v0.3 target with declared legacy migration |
| v0.2 signed | Existing policy | Yes | No | Metadata/schema or legacy adapter | No | To signed v0.3 target with declared legacy migration |
| v0.3 unsigned | Existing explicit trust policy | Yes | Yes, output remains unsigned | Yes if data contract complete | Yes if compatible | To compatible v0.3 target |
| v0.3 signed | Full | Yes | Yes, signature preserved | Full | Full | Full |

## Rollout stages

### Stage A: read-only understanding

Ship v0.3 inspection and Overview while keeping lifecycle writes disabled behind
an internal feature flag. Validate metadata safety and compatibility.

### Stage B: copy/fork

Enable Duplicate, Compact duplicate, Fork and Template creation. No compare
writes yet.

### Stage C: compare

Enable read-only compare and exportable bounded reports.

### Stage D: reconcile

Enable apply-to-copy after native crash and conflict testing.

### Stage E: application upgrade

Enable same-schema upgrade, then declarative migrations after separate
qualification.

Feature flags must be host-owned compile-time or protected-local configuration,
never capsule-controlled.

## Format publication

A v0.3 release requires:

- checked SQL;
- independent conformance JSON;
- Python and Rust verifier support;
- signed-app v0.3 canonical test vectors;
- good/bad compatibility fixtures;
- updated docs and ADRs;
- updated creator plugin snapshot;
- Diagram Studio v0.3 reference capsule;
- no semantic change to v0.2.

## Deprecation

Do not deprecate v0.2 in this programme. It remains a distinct read/run profile.
New authoring defaults may move to v0.3 only after the creator plugin and both
hosts are complete.

## User communication

Distinguish:

- `Host update available` — updates installed native software.
- `Application upgrade available/selected` — user chose a local newer capsule
  release.
- `Format conversion` — a new output is created; the original remains intact.
- `Trust required` — exact target application release has not yet been granted.

## Telemetry

No network telemetry is introduced. Local audit should record operation type,
digests, counts, result and stable error codes, not paths in redacted exports or
domain values.

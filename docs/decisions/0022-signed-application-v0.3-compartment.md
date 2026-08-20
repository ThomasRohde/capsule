# ADR 0022: Signed application v0.3 compartment and canonical stream

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

The live signed-app/0.2 implementation signs every non-internal SQLite schema
record and the rows of an exhaustive platform-table allowlist. Domain rows,
grants, change history and signature envelopes are excluded. This means table
schema and table rows are separate signature decisions: a mutable table's schema
is still immutable application definition even when its rows are user-owned.

The lifecycle draft correctly separates application and instance rows, but its
"signed tables" and "mutable tables" wording did not make that schema/row
distinction explicit.

## Decision

Signed v0.3 applications use profile
`org.sqlite-capsule.signed-app/0.3`, with exact contexts:

```text
SQLite Capsule signed-app canonical stream v2\0
SQLite Capsule signed-app signature v2\0
```

V0.2 retains its existing profile and v1 contexts.

The v0.3 signature-envelope column order remains the reviewed v0.2 order
(`key_id`, `algorithm`, `public_key`, `application_digest`, `signature`,
`signed_at`). The new profile is not a reason to introduce gratuitous framing
drift.

The v0.3 application digest includes:

1. every non-internal SQLite schema record, ordered and framed exactly as in the
   reviewed v0.2 canonical writer, including domain schema and the schema of
   mutable platform tables;
2. rows from the exhaustive v0.3 application allowlist: manifest, application
   profile, executable assets, commands, runbooks, docs, endpoints and steps,
   checks, prompts, data-contract tables, migration declarations and publisher;
3. table rows in declared primary-key order with the existing explicit field
   names, storage-class tags and length framing; and
4. canonical JSON in declared JSON columns using the same duplicate-key
   rejection, 1 MiB per-value limit and RFC 8785 canonicalizer as v0.2.

The digest excludes rows from `capsule_instance`,
`capsule_instance_asset`, `capsule_lineage_event`,
`capsule_lineage_parent`, `capsule_grant`, `capsule_change_log`,
`capsule_signature`, and every ordinary domain table. It does not exclude their
schema records. Unknown `capsule_*` objects fail closed.

SHA-256 of the framed stream is the application digest. Ed25519 signs the new
signature context, the 32-byte digest, the length of `signed_at`, and the exact
UTC-seconds timestamp, matching the established v0.2 message shape under the
new context.

## Required proof

M01 must publish independent Rust and Python vectors that start from one signed
v0.3 fixture and isolate every included and excluded class. Mutating instance
profile rows, safe instance-asset rows, lineage, grants, change history or
domain rows must preserve the digest and signature. Mutating any schema object
or signed row must change the digest. Unknown platform objects and cross-profile
contexts must be rejected.

## Consequences

- Fork, profile edits, lineage updates and domain reconciliation can preserve a
  publisher signature without making mutable schema attacker-controlled.
- The current `capsule-crypto` writer is the compatibility baseline; M01 adds
  version dispatch rather than changing its v0.2 constants.
- Cryptographic validity remains distinct from publisher trust, revocation and
  capability grants.

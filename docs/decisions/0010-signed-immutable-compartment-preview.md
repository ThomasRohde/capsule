# ADR 0010: Signed immutable application-compartment preview

## Status

Accepted as a preview design contract; cryptographic verification is not
implemented by the current format.

## Decision

Define a future signature boundary with two compartments. The immutable
application compartment covers canonical application assets, endpoint
declarations, platform schema, and release identity. The mutable data
compartment covers domain rows, change history, and user-editable presentation
state. A future signature envelope names a publisher key and signs a stable
SHA-256 content digest.

The preview is recorded in
`format/capsule-signed-compartment-preview.json`. It is descriptive metadata,
not a trust grant. Hosts continue to use explicit local trust and internal
hash checks, and must not claim publisher authenticity.

## Consequences

- User edits do not inherently invalidate application provenance.
- Canonicalization and compartment boundaries become reviewable before a key
  format is selected.
- Unsigned and modified files remain ordinary inspectable SQLite databases.
- A future signing implementation must define key policy, revocation,
  canonicalization, and modified-after-signature behavior before execution is
  enabled.

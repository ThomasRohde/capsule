# ADR 0015: Signed application compartment

## Status

Accepted for the current format and signed-app profile.

## Context

A whole-file signature would be invalidated by ordinary user edits. The host
instead needs publisher authenticity for executable and declarative application
content while leaving mutable domain data mutable.

## Decision

The optional profile is `org.sqlite-capsule.signed-app/0.2`. Current capsules
add one `capsule_publisher` row and zero or more `capsule_signature` rows
using `format/capsule-signed-app-v0.2.sql`.

The canonical stream includes exact non-internal schema records and rows from
`capsule_manifest`, `capsule_asset`, `capsule_command`,
`capsule_runbook`, `capsule_doc`, `capsule_endpoint`,
`capsule_endpoint_step`, `capsule_check`, `capsule_prompt`, and
`capsule_publisher`. It excludes domain rows, `capsule_grant`,
`capsule_change_log`, and signature envelopes. Unknown `capsule_` tables are
rejected.

JSON columns are canonicalised with duplicate keys rejected. The byte stream
uses explicit type tags and length framing; the application digest is SHA-256.
Ed25519 signs the digest plus the authenticated publisher-asserted UTC
`signed_at` value. A valid signature proves key possession only. Host-local
policy separately decides publisher identity, trust, and revocation.

Signing verifies the source, creates a separate destination through SQLite's
backup API, installs or checks the extension, appends the signature, reopens the
result, and verifies it. Private keys never enter capsules, logs, renderer state,
or diagnostic bundles. Repository seeds are public test material only.

## Consequences

- Domain writes, grants, and change-log rows preserve the application digest.
- Application, endpoint, publisher, or schema changes invalidate it.
- Schema text is exact signed evidence.
- Unsigned or untrusted capsules remain inspectable, but execution is a separate
  host-policy decision.
- Algorithm agility requires a new profile.

## Primary references

- [RFC 8032: EdDSA and Ed25519](https://www.rfc-editor.org/rfc/rfc8032)
- [RFC 8785: JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785)
- [SQLite backup API](https://www.sqlite.org/backup.html)

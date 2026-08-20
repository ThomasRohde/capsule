# ADR 0021: Format v0.3 and capsule instance identity

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

Format v0.2 stores application-release and user-owned instance fields in the
same `capsule_manifest` row. Signed-app/0.2 therefore authenticates the title,
summary, capsule ID and mutable timestamps as application release content. A
fork or profile edit cannot preserve that application digest.

Changing the meaning of the existing row would make already-signed v0.2 files
ambiguous and would conflict with the current exact version dispatch in the
Python and Rust verifiers.

## Decision

Lifecycle-aware capsules use a distinct format tuple:

```text
PRAGMA application_id = 1129337676
PRAGMA user_version = 3
format_id = org.sqlite-capsule
format_version = 0.3
runtime_protocol = capsule-http/0.2
```

The runtime protocol remains `capsule-http/0.2` because the named-endpoint
application bridge does not gain lifecycle methods. Format dispatch checks the
complete tuple and does not infer a profile from optional tables.

`capsule_manifest` and `capsule_application` contain immutable application
release identity and display metadata. `capsule_instance` contains the mutable
capsule ID, revision ID, title, description, document kind, tags, safe
instance-asset pointers and instance timestamps. Domain rows and lineage are
also user-owned instance state.

Capsule, revision, event, plan and operation IDs use lowercase RFC 4122
hyphenated UUID text without an `urn:` prefix. Hosts generate UUIDv4 values for
new lifecycle identities. Readers accept canonical RFC 4122 versions 1-5 so
valid imported v0.3 authoring output is not needlessly narrowed. Platform times
are exact UTC RFC 3339 seconds (`YYYY-MM-DDTHH:MM:SSZ`).

`revision_id` identifies the exact logical content revision, not merely a
lifecycle branch. Every successful v0.3 named write causes the trusted host to
set a fresh UUIDv4 `revision_id` and `content_updated_at` in the same SQLite
transaction as the domain change and change-log append. Application SQL and the
raw renderer cannot choose either value. Failed/rolled-back writes change
neither. Ordinary writes do not invent lineage events; lifecycle operations do.

Identity effects are fixed:

| Operation | capsule ID | revision ID |
| --- | --- | --- |
| exact or compact duplicate | preserve | preserve |
| fork or template creation | new | new |
| reconcile to target-derived copy | preserve target | new |
| application upgrade | preserve working capsule | new |

Format v0.2 remains a separate supported read/run profile. It may be duplicated
without changing logical identity. A signed v0.2 capsule is never rewritten to
look like v0.3. Moving user state from v0.2 to v0.3 would require a create-new
upgrade into a clean signed v0.3 release with an explicit compatible legacy
migration declaration. That declaration is not representable by the current
draft and the operation is unavailable until M08 accepts a separate versioned
legacy-adapter ADR/schema. No host may infer v0.2 dataset semantics.

## Consequences

- Mutable instance/profile/domain changes no longer require publisher signing.
- Host UI must visually separate application, publisher, instance and file
  identity.
- Python, Rust, conformance, authoring and plugin surfaces need explicit v0.2
  and v0.3 dispatch rather than optional-table probing.
- Existing v0.2 fixtures and signature vectors remain byte-for-byte normative.
- The cross-host 64 MiB capsule size policy and verification-phase correction
  are recorded in [ADR 0028](0028-verification-phases-and-size-policy.md).

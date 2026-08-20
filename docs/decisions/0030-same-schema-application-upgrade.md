# ADR 0030: Same-schema application upgrade is a clean-release rebase

## Status

Accepted on 2026-08-20 by lifecycle milestone M07.

## Context

An application upgrade must replace executable application state without
turning a working Capsule, mutable metadata or a renderer-supplied plan into
publisher authority. The draft upgrade contract mixed same-schema upgrade with
M08 migration, exposed an output path in review data and did not bind complete
signature inventories, clean target state or exhaustive dataset states.

The v0.3 application signature authenticates application assets, endpoints,
permissions, the data contract and the reserved template-state document. It
does not authenticate ordinary domain rows, mutable instance/profile rows,
grants, change logs or lineage. A generally valid target signature alone is
therefore not evidence that a release Capsule contains clean target rows.

## Decision

M07 is the versioned `application-upgrade-same-schema-v1` operation. It starts
from an exact retained snapshot of a clean target release and carries permitted
working state into that snapshot. It never patches the working Capsule and
never mutates either input.

Admission requires all of the following:

- both inputs are exhaustively verified v0.3 Capsules with complete valid
  signature inventories;
- application IDs match;
- the target application version has strictly greater SemVer 2.0.0 precedence;
  build metadata does not create a newer release, and malformed versions are
  unsupported rather than compared lexically;
- one host-accepted Ed25519 key ID is cryptographically valid and digest
  matching in both exact signature inventories; display publisher metadata,
  key names and lineage never choose this authority;
- data-schema ID/version, signed dataset/table structure, declared keys,
  dependencies and the physical table/index/trigger schema match exactly;
- the target's signed `org.sqlite-capsule.template-state/1` proof reproduces
  against the retained target rows.

Publisher rotation, delegation, cross-application import and schema migration
are not inferred. They fail closed outside M07.

Capability comparison canonicalizes the complete signed permissions object.
The review lists added, removed and changed top-level capabilities and binds
both object digests. Added or changed authority requires a separate explicit
confirmation. Removal alone does not.

The target release's signed upgrade policy supplies one exhaustive action per
dataset:

- `copy` replaces clean target rows with the exact compatible working state;
- `target` retains the clean target rows;
- `rebuild` retains the authenticated clean target state without running
  application code;
- `omit` is accepted only when the authenticated target state is empty;
- `migrate` is reserved for M08 and `forbid` blocks M07.

Source and target dataset states use the existing streaming
`org.sqlite-capsule.dataset-state/1` profile. The review and lifecycle plan bind
source, target and expected row count/digest for every dataset. Final validation
recomputes every expected state.

The output preserves the working `capsule_id`, title, description, document
kind, tags, timestamps and referenced instance icon/cover assets, and mints a
new revision. Target application/profile rows, assets, endpoints, documents,
data contract, application digest and the complete target signature inventory
remain exact. Grants and change logs are cleared. Existing lineage is replaced
by one `application-upgrade` event with exactly two ordered parents:
`upgraded-from` the working input and `application-release` the clean target,
each bound to its capsule/revision identity and exact file SHA-256.

Execution uses non-serializable Rust typestates and the held-parent,
create-new/no-replace publication state machine. It rebinds both inputs and the
destination before staging, before publication and after reopening the result.
The serializable `org.sqlite-capsule.upgrade-plan/1` object is bounded review
evidence only. It carries no path, SQL, row value, handle, nonce or executable
authority.

The CLI separates `plan-upgrade` from `upgrade`. Execution requires an exact
publisher-key confirmation string and, when applicable, an additional
capability-change confirmation. The Tauri main shell uses opaque candidate and
destination tokens plus an expiring one-use nonce. The raw Wry application
renderer receives no upgrade command, event, path, token or report.

## Consequences

- M07 can replace a signed application release while retaining compatible user
  state and preserving exact target publisher evidence.
- A target release must be intentionally authored as a clean template; an
  ordinary signed working Capsule is not accepted as a release source.
- Same schema means more than a matching version number. Contract and physical
  schema drift are rejected rather than handed to SQLite coercion.
- The M08 migration executor can extend the admission and dataset policy model
  without weakening this operation or reinterpreting an M07 plan.
- Upgrade fixtures, contracts, creator guidance and trusted-shell tests must
  remain synchronized with this decision.

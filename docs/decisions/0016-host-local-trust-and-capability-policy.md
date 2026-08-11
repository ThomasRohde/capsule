# ADR 0016: Host-local trust and capability policy

## Status

Accepted on 2026-08-08 for the native trust model and host/CLI surfaces. This
decision does not imply that platform installer signing,
cross-platform renderer evidence, host-owned revocation refresh, recovery, or
updates are complete.

## Context

A publisher signature authenticates application bytes; it does not express the
recipient's trust, grant capabilities, prove current revocation status, or
authorise execution. Storing those decisions in the capsule would let a copied
or externally modified database carry or edit its own authority. A browser/OS
permission prompt is also a separate layer and cannot silently create a host
grant.

The desktop shell therefore needs protected state outside every capsule and a
deterministic policy shared by its UI and administrative CLI. Missing policy
information must prompt or deny, never allow.

## Decision

### Protected store

The native host keeps one versioned SQLite trust store under Tauri's
per-application local-data directory. The administrative CLI accepts an
explicit store path. A capsule database is never attached to this connection,
and capsule SQL, assets, runbooks, and text cannot address it.

Schema version 2 atomically migrates v1 and uses `STRICT` tables for publishers, publisher keys,
delegations, exact signed releases, exact-file local exceptions, observed
capsule identities, application-digest capability grants, revocation-bundle
metadata and active remote key/release/root records, backup inventory, and audit
events. Creation/migration is atomic,
foreign keys are enabled, `trusted_schema` is disabled, durability is `FULL`,
and the store is checked before use.

New POSIX store directories and files use modes 0700 and 0600. Windows applies
a protected DACL containing only the current process-token owner with full
control to the dedicated directory and database/WAL/SHM/backup files. Existing
arbitrary ancestor directories are not rewritten. An unsupported platform that
cannot apply either protection fails closed.

No private signing key is stored. Redacted export includes all decision,
identity, revocation, backup, and audit categories but omits public-key bytes,
private material, the store path, and backup paths. Reset requires the exact
confirmation `ERASE-TRUST-DECISIONS`; the CLI first creates and verifies a new
SQLite backup and never deletes capsules or prior backups.

### Trust states

The evaluator distinguishes:

- unverified;
- structurally verified unsigned;
- signature valid with unknown publisher;
- signed and trusted publisher;
- locally trusted exact file or signed release;
- modified after signature;
- invalid signature;
- denied by the user; and
- revoked.

Cryptographic validity, publisher presence, publisher trust, local trust,
revocation status, grants, and `executable_allowed` remain separate fields.
Publisher IDs/names are signed display data, while key IDs are recomputed from
the exact Ed25519 public key before publisher trust can be recorded. A key
cannot be rebound to another publisher. Local and verified remote revocation
override publisher, release, grant, and exact-file trust. Until a verified
revocation bundle exists, a non-revoked decision reports `not_checked`; an
installed bundle reports `fresh` or `stale`, while every known revoked
key/release blocks in either state. See [ADR
0019](0019-signed-host-updates-and-revocations.md).

### Capabilities and first-open choices

Effective authority is the most restrictive result across:

1. runtime support;
2. the verified signed-manifest request;
3. host policy;
4. trust/revocation state;
5. an exact capsule/application-digest grant or current allow-once selection;
6. an operating-system/per-use decision where applicable.

Database read/write, clipboard read/write, user-selected file read,
download/export, fullscreen, camera, and microphone remain distinct. General
filesystem, shell/process, arbitrary URL, and unrestricted network authority
are unsupported and deny. Optional prompts do not block execution when every
required capability allows, but they are not silently upgraded to allow.

The trusted main WebView owns the prompt and may submit only one bounded action
through its allowlisted command:

- `allow once` sets session trust and the selected capabilities only in memory;
- `always` is enabled only for a currently valid signature, records trust for
  the exact capsule/application digest/key, and records allow or deny for every
  requested capability;
- `deny` persists an exact-release deny when a current signature exists, or an
  exact-file deny otherwise;
- `cancel` records no trust or capability decision.

On a later launch, a still-valid signature for the same capsule ID,
application ID, application digest, and public key automatically reuses the
stored exact-release decision and complete capability grants. The host opens
the verified runtime and releases the application window without asking for
the same decision again. Any missing grant or changed identity, digest, key, or
permission request returns to the locked first-open review.

A changed capsule ID, application ID, signed application digest, public key, or
permission request does not inherit the previous exact grant. Unsigned local
trust remains an explicit exact-file exception and is visually distinct from a
trusted publisher.

The raw capsule child has no Tauri initialization or IPC handler and cannot
invoke these commands. The policy layer records and presents the release
decision. The implemented runtime bridge keeps the child locked and reports
`assets_released: false` until `executable_allowed` is true, then opens the
independently verified runtime and materialises only its verified entry asset.

The same trusted main surface exposes only five bounded administration reads or
mutations: recent audit inspection, full redacted export, exact-confirmed
forgetting of the current file/release decision, exact-fingerprint revocation of
the current trusted key, and exact-confirmed reset. Forgetting deletes only the
current exact-file row and the current application-digest release/grant rows,
preserves publisher trust, revocations, backups, other capsules/digests and audit
history, records its own audit event, and returns to a prompt without granting
authority. UI reset first creates and verifies a new owner-protected backup in
the application's local backup directory. No command accepts a caller-chosen
filesystem path.

## Consequences

- Copying a grant into a capsule or editing capsule-controlled data cannot add
  authority.
- A valid signature can still be blocked, prompted, denied, or revoked.
- `allow once` can appear in audit evidence without becoming a reusable grant.
- Persistent capability grants require a signed application digest; unsigned
  files cannot receive the first-open UI's `always` choice.
- Exact local trust is intentionally less convenient than publisher trust and
  must retain a visible local/developer warning.
- Host-owned revocation refresh, production root custody, updater installation
  and rollback, and release evidence remain later delivery work.

# Product requirements

## Personas

### Capsule user

Owns one or more capsule files and wants to open, duplicate, organise, compare
or upgrade them without understanding SQLite internals.

### Capsule publisher

Builds and signs an embedded application release, data contract and migration
path. Does not own a user's mutable instance metadata or domain data.

### Capsule reviewer

Inspects identity, permissions, application changes, data differences, lineage
and migration plans before allowing execution or publication.

### Coding agent

Needs deterministic machine-readable contracts, dry-run plans, stable errors,
bounded reports and exact validation evidence.

## Functional requirements

### FR-1: Capsule overview

Before executable assets are released, the trusted shell shall display bounded:

- capsule title and description;
- application name, description, version and category;
- safe icon/cover, or a deterministic fallback;
- capsule ID and revision ID;
- application ID and exact release digest;
- publisher and signature status;
- data schema identity/version;
- file path, size and last filesystem modification time;
- requested capability summary;
- lineage summary;
- format compatibility and available lifecycle actions.

Display metadata is untrusted content. It must never be presented as verified
publisher identity unless it is in the signed application compartment and is
labelled accordingly.

### FR-2: Duplicate

Create a SQLite-consistent new-path snapshot preserving logical capsule and
revision identity. The source remains unchanged. Verification must occur before
the destination is published.

### FR-3: Compact duplicate

Create a logically equivalent new-path copy with unused pages removed. Preserve
logical capsule and revision identity while recording that file bytes may
differ.

### FR-4: Fork with current data

Create a new capsule ID and revision ID using the same signed application
release and selected domain datasets. Preserve the application signature. Add a
lineage event referencing the source revision and digest.

### FR-5: Create from template

Create a new capsule instance from a clean application/template release.
Application-defined seed data is retained; user-content, history, derived and
cache datasets follow declared policies. The host must not infer blankness by
deleting arbitrary non-platform tables.

### FR-6: Selective fork

Allow dataset selection only where the signed data contract declares the
dataset forkable and its dependencies can be satisfied. Sensitive datasets are
off by default and require explicit consent.

### FR-7: Compare

Compare two capsules without executing assets, endpoints, prompts, runbooks or
declared application code. Present:

- identity and lineage;
- application release and capability differences;
- domain schema compatibility;
- dataset and table summaries;
- bounded row and field differences for declared compatible tables.

### FR-8: Reconcile to copy

Apply selected data changes to a new copy derived from the target. Never modify
source or target inputs. Revalidate the complete result and create
multiple-parent lineage.

### FR-9: Application upgrade

Given a working capsule and a clean newer application release:

- require the same application ID;
- require acceptable publisher continuity;
- preserve capsule identity and user profile;
- create a new revision;
- use target application assets, declarations, seed data and schema;
- copy or migrate domain datasets according to signed policies;
- preserve the target release's application digest;
- require a new trust/capability decision when the exact release or permissions
  change;
- leave both inputs unchanged.

### FR-10: Migration

Resolve an exact, acyclic and unambiguous path between data schema versions.
Execute only a restricted declarative migration language in the trusted host.
No arbitrary source-supplied code or SQL is executed.

### FR-11: Lineage

Every non-identical lifecycle output records:

- operation type;
- new capsule and revision identity;
- timestamp;
- immediate parent identities and file digests;
- application release digest;
- data schema identity/version;
- deterministic operation-plan digest;
- validation result summary.

Exact duplicates preserve the existing revision and need not create a new
lineage event.

### FR-12: Agent/CLI parity

Core lifecycle planning and execution shall be available through stable Rust CLI
JSON contracts for tests and agents. The Tauri shell shall consume the same
service layer rather than reimplement logic in JavaScript.

## Non-functional requirements

- Fail closed on unknown format, schema, signature, dataset or migration state.
- Inputs pinned against replacement and opened read-only.
- Bounded memory, rows, bytes, time and JSON sizes.
- No lifecycle command registered for the raw renderer.
- Create-new output and atomic final publication.
- Deterministic plan hashes and stable error codes.
- Full auditability without exposing sensitive row values by default.
- Keyboard-accessible and screen-reader-labelled trusted shell UI.
- Existing v0.2 open/run behaviour remains supported.
- Generated sources, standalone plugin and documentation remain synchronised.

## UX language

Use explicit verbs:

- `Duplicate`
- `Compact duplicate`
- `Fork with current data`
- `Create from template`
- `Compare`
- `Apply selected changes to a new copy`
- `Upgrade application`

Avoid ambiguous primary labels such as `Clone`, `Merge` or `Rebase`.

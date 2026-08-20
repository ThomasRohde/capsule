# ADR 0029: Copy operation profiles and authenticated template state

## Status

Accepted on 2026-08-13 by lifecycle milestone M04.

## Context

The M02 executor already publishes an exact byte copy of its verified private
snapshot. Earlier programme prose instead described an SQLite online backup
whose physical bytes could differ. Those are different user-visible promises
and cannot share one unversioned operation.

Semantic copy modes also need stronger authority than a recomputed lifecycle
plan. The v0.3 application signature authenticates policy and schema, but it
intentionally excludes ordinary domain rows and mutable instance, lineage,
grant and change-log rows. A signed working capsule is therefore not proof that
its seed state is clean or safe to use as a template.

## Decision

The first duplicate operation is `copy-exact-snapshot`. It copies the exact
verified private snapshot byte-for-byte, preserves every row and schema object,
preserves capsule and revision identity, preserves the complete signature
compartment, appends no lineage and requires the output SHA-256 to equal the
reviewed snapshot SHA-256. It never reopens or backs up the live source path.

Compact duplicate is a separate versioned operation. It preserves logical
identity and all logical state, but may change page layout and file SHA-256. It
must prove exhaustive logical equivalence and absence of deleted-page sentinel
data before publication. A compact implementation may use only an owner-private
intermediate and the existing held-parent, create-new publication state
machine; `VACUUM INTO` may not write directly to a renderer path or final name.

The first compact profile is
`org.sqlite-capsule.compact-logical-state/1`. It hashes persisted database
identity PRAGMAs, every `sqlite_schema` row except physical `rootpage`, every
actual column, and every row of every non-internal table, `sqlite_sequence`,
and supported `sqlite_stat*` table. Complete typed row frames are individually
hashed, sorted by their fixed SHA-256 bytes, and aggregated with multiplicity;
compact equivalence therefore does not depend on a primary key or scan order.
The exact grammar is normative in `docs/format-contract.md` and frozen by
cross-implementation vectors. `page_size` is an operation postcondition rather
than part of the logical digest. Publication additionally requires DELETE
journal mode, no journal/WAL/SHM sidecar, zero freelist pages, unchanged logical
identity/signature state, and repeated exhaustive capsule verification.

Exact and compact duplicate may accept an exhaustively verified v0.2 capsule or
an exhaustively verified v0.3 capsule with no invalid signature envelope. An
unsigned source remains visibly unsigned; duplication does not confer trust.
Any present invalid or digest-mismatching signature fails closed. These modes
do not interpret a data contract and do not rewrite v0.2.

Fork, selective fork and template creation require v0.3 plus at least one
cryptographically valid, digest-matching signature over the complete signed
application compartment. Host publisher trust is a separate state and must be
shown separately. V0.2 fork, selective fork and template creation return
`unsupported_operation`, not `unsupported_format`.

The executor never trusts renderer choices or edited plan decisions. It derives
one unique action for every dataset from the freshly verified signed contract
plus a validated, plan-bound choice and confirmation where the signed policy is
`prompt`. It validates dependency closure against both declared dependencies
and actual cross-dataset foreign keys, and binds the complete decision set into
the plan.

The initial workspace contract accepts a cross-dataset foreign key only when
the child dataset declares a dependency on the parent dataset and both update
and delete actions are `NO ACTION` or `RESTRICT`. `CASCADE`, `SET NULL` and
`SET DEFAULT` across datasets are rejected because they could make one dataset
decision silently mutate another. Same-dataset effects remain subject to the
operation-specific final-state proof.
Insertions are dependency-first; removals are dependent-first. A dependency
may not silently cause sensitive data to be copied.

Dataset actions mean:

- `copy`: copy every row in the dataset; a sensitive dataset requires explicit
  confirmation and is blocked, not silently changed to omission, when that
  confirmation is absent;
- `omit`: preserve the signed schema but copy no source rows;
- `reset`: use only authenticated clean state from a separately verified
  template source; it is unavailable when no such state exists;
- `prompt`: offer a bounded host-defined `copy`/`omit` choice, defaulting a
  sensitive dataset to `omit` and requiring explicit confirmation whenever a
  sensitive dataset is copied;
- `forbid`: reject any mode that would select or alter that dataset.

A required dataset may never resolve to omission. "Without data" never means
dropping tables or guessing from names.

Template creation remains unavailable until the selected clean release carries
a signed template-state proof. The proof is a strictly bounded, versioned
record in the signed application compartment and covers every dataset's
canonical logical state, row count and disposition. It identifies, but cannot
configure, one fixed host-owned mutable-platform-state profile. It is verified
against the actual clean source snapshot.
A general application signature, mutable `document_kind`, title or tag is not a
template designation. The proof format and canonical row-state digest require
synchronized contracts, vectors, creator-plugin support and hostile tests
before the operation is enabled.

For all non-identical outputs the host owns mutable platform policy:

- grants are cleared and never copied as authority;
- change-log rows are cleared;
- no prior lineage rows are copied; a bounded new lineage event and parent
  reference are generated from plan-bound values;
- sequence state is rebuilt from retained rows and cleared for omitted state;
- instance metadata and instance assets follow explicit plan-bound profile
  actions and never inherit publisher-trust styling or authority.

Selective removal may not be implemented as deletion followed by publication.
The final private output must be rebuilt or compacted so omitted sensitive bytes
are absent from live rows, freelist pages, change logs, lineage, instance media,
SQLite sequence state and temporary/sidecar files. Operation validation checks
the derived action and resulting state of every dataset and mutable platform
table before and after publication.

A clean source used to reset a dataset during a fork must have exactly the same
application digest, app ID/version and data-schema ID/version as the working
source. A later release belongs to the separately governed application-upgrade
operation.

Semantic creation rejects a partial, malformed, cryptographically invalid or
digest-mismatching signature inventory, even if another envelope is valid. A
future profile may explicitly define retained historical signatures; the first
profile does not infer that meaning.

The initial proof uses one reserved signed `capsule_doc` row, so the accepted
v0.3 SQL shape does not change:

```text
slug       = org.sqlite-capsule.template-state
title      = SQLite Capsule authenticated template state
media_type = application/vnd.sqlite-capsule.template-state+json
sequence   = 0
```

Its canonical JSON conforms to `org.sqlite-capsule.template-state/1`. Identity
and schema fields exactly match the signed manifest. Dataset records are
exhaustive, unique and ordered by BINARY dataset ID; each carries `seed` or
`empty`, its physical row count and an
`org.sqlite-capsule.dataset-state/1` SHA-256. The mutable-platform member is the
fixed identifier `org.sqlite-capsule.template-platform-reset/1`, never a set of
publisher-provided actions. The proof JSON itself is authenticated because all
`capsule_doc` rows are in signed-app/0.3.

Dataset-state v1 is a streaming, length-framed binary profile over app/schema
identity, the signed table/primary-key declarations, all actual table columns
and every stored value in deterministic BINARY primary-key order. It includes
ignored and generated columns, preserves IEEE-754 signed zero, rejects
non-finite values and performs no Unicode or domain-JSON normalization. Empty
table headers are included. Counts are bounded, unsigned big-endian integers;
SQLite integers are signed 64-bit big-endian and value tags distinguish NULL,
INTEGER, REAL, TEXT and BLOB. The stream is hashed incrementally under the same
source binding, deadline, cancellation, row and byte budgets as the operation.
The byte-for-byte grammar is normative in `docs/format-contract.md`; this ADR
does not define a looser alternative encoding. Implementations must match the
independent Python/Rust vectors in `compatibility/template-state-v1/`, including
empty-table headers, composite BINARY key ordering, raw UTF-8 and generated
columns.

Planning APIs are split by authority:

- `preview_copy` is deterministic review data and grants no authority;
- host-only `prepare_copy` consumes opaque source and held-destination
  capabilities and retains a current-host prepared plan;
- `execute_copy` consumes the retained plan ID and a session-bound, expiring,
  one-use confirmation nonce.

No Tauri or raw-Wry surface accepts a filesystem path, table name, SQL text,
serialized plan or arbitrary decision object. Progress is targeted only to the
trusted `main` shell.

## Consequences

- Exact-snapshot, compact-logical and semantic modes are executable through
  separate non-serializable typestates. The initial semantic profile supports
  fork, authenticated template creation and selective fork; fork/selective
  `reset` remains deliberately unavailable until a second retained clean source
  can provide authenticated reset rows.
- Semantic lifecycle-plan vectors are required in addition to canonical-JSON
  vectors; edited and recomputed decisions must fail operation validation.
- Tests hash every source before and after, inspect raw output bytes for
  sensitive sentinels, and cover cross-dataset foreign keys, ABA/source races,
  destination races and crash points.
- Material template-proof work must update the v0.3 contract documentation,
  independent vectors and the standalone `capsule-creator` plugin together.

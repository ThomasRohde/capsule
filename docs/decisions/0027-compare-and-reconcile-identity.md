# ADR 0027: Compare and reconcile identity

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

SQLite `rowid`, collation-dependent string conversion and JSON coercion do not
provide portable row identity. A two-snapshot diff also does not supply the
common base required to claim an automatic merge.

## Decision

Row comparison uses the signed data contract's ordered declared primary key.
PK-less tables, unsupported collations and keys that cannot be stably ordered
are unavailable for row comparison/reconciliation; `rowid` is never substituted.

The typed value encoding is versioned and exact:

```text
00                                  NULL
01 + 8-byte big-endian i64          INTEGER
02 + 8-byte IEEE-754 bits           REAL
03 + u64 length + UTF-8 bytes       TEXT
04 + u64 length + bytes             BLOB
```

Non-finite reals and invalid UTF-8 fail closed; signed zero bits are preserved;
no Unicode normalization or storage-class coercion occurs. Row keys include a
profile marker, length-framed table name, ordered PK column names and typed
values. Row digests add compared columns in contract/schema order. M02/M05 must
publish Python/Rust vectors for every storage class, composite keys, mixed types
and Unicode combining forms.

Comparison is execution-free, streaming, bounded and summary-first. Default
reports carry counts, digests, bounded labels and redaction/truncation state,
not full values. Detail pagination uses host-minted session/report-bound tokens;
sensitive details require explicit trusted-shell disclosure.

Compatibility is classified only after both retained inputs pass exhaustive
workspace admission. An invalid capsule therefore returns the stable bounded
workspace error envelope and never produces an `invalid-input` comparison
report. This keeps a report from laundering unverified metadata into a display
object while preserving explicit admission failure evidence.

Two-way reconciliation applies only explicit reviewed selections. Automatic
three-way classification requires an explicit verified ancestor. Every result
is a new target-derived copy, every selected row/field binds a target
precondition digest, and conflicts remain unresolved until explicitly chosen.

M06 freezes reconciliation authority as two canonical layers. The generic
`org.sqlite-capsule.lifecycle-plan/1` envelope is the only layer with pinned
filesystem inputs and a create-new destination. It has operation
`reconcile-to-copy`, input roles in exact `source`, `target`, optional
`ancestor` order, expected target capsule/application/schema identity with a
new revision, and exactly one `bind-reconcile-payload` decision containing only
the `reconcile_payload_digest` scalar. The payload is
`org.sqlite-capsule.reconcile-payload/1`; it contains no path, raw key, raw
value, SQL, nonce or output capability and is never independently executable.

Row evidence is tagged `{state:"absent"}` or
`{state:"present",row_digest:<sha256>}`. JSON null is never overloaded as row
absence because a present SQLite row containing SQL NULL still has a real row
digest. Operations have contiguous sequence numbers, digest-only stable keys,
closed action shapes and exact source/target/optional-ancestor row states.
Two-way operations always have the `user-selected` basis. Three-way operations
use `three-way-clean` or bind exactly one resolved conflict by its canonical
conflict ID. Executable payloads contain no unresolved conflict and no
`policy-forbidden` conflict; policy rejection remains an admission error.

`insert-source-row` binds an
`org.sqlite-capsule.reconcile-write-set/1` digest over all ordered stored source
columns and typed values, including declared PK, immutable and ignored columns,
but excluding generated columns. `replace-target-row-from-source` binds that
profile over only canonically differing mutable compared stored fields; target
PK, ignored and immutable columns are preserved and generated columns
recompute. It is an UPDATE, never SQLite `REPLACE` or `SELECT *`.
`set-target-fields-from-source` names a unique non-empty list in signed-column
order and binds both source and target typed-value digests per field.

The payload exhaustively binds source, target and planned output
`org.sqlite-capsule.dataset-state/1` counts/digests for every signed dataset,
plus the complete verified signature inventory for each input and exact
sensitive-dataset confirmation. Independent host checks enforce canonical
UTF-8, SHA-256 after omitting `payload_digest`, 16 MiB source/canonical byte
ceilings, nesting depth 32, 10,000 operations, 10,000 resolved conflicts, 256
datasets and 256 fields per field operation. These ceilings do not enlarge the
separate 1 MiB lifecycle-plan ceiling.

The payload also binds the exact reconciliation lineage event. Its UTC
`occurred_at` equals the lifecycle plan's `created_at`; its result identity
equals the lifecycle expected capsule/revision; and it has exactly two ordered
parents: target `target-derived-from`, then source `changes-applied-from`, each
with exact input file SHA-256 and capsule/revision IDs. A three-way ancestor is
bounded evidence in lineage details, never a third parent. Lineage details bind
the compare report, operation/resolution counts and the payload digest. To avoid
an impossible hash self-reference, the top-level `payload_digest` and its exact
equal alias at `lineage.details.payload_digest` are both omitted from payload
digest material, then both populated with and checked against the result.

Stable errors use `org.sqlite-capsule.lifecycle-errors/1`; safe messages omit
row values, SQL, secret material and unredacted paths. Internal causes remain
host-controlled diagnostics.

## Consequences

- Different limits or disclosure state produce a different report digest.
- Reconciliation never mutates either compared input or claims an implicit
  merge from two snapshots.
- The draft monolithic compare report must be split into a bounded summary and
  paginated detail contract in M05 before it becomes normative.

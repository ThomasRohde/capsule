# Compare and reconcile design

## Compatibility classification

Before row comparison, classify the pair.

| State | Allowed behaviour |
| --- | --- |
| Same app digest and data schema | Full declared data comparison |
| Same app ID, different release, same data schema | Application diff plus data comparison |
| Same app ID, migratable schema versions | Compare after temporary normalisation to a common schema |
| Same app ID, incompatible schema | Identity/application/schema summary only |
| Different app IDs | Generic metadata and schema inventory only |
| Invalid/unsupported signature or format | Bounded inspection only; no reconciliation |

## Four comparison layers

### 1. Identity and lineage

- file identity and hashes;
- capsule/revision IDs;
- common lineage event where available;
- parent chains and operations;
- format and data schema versions.

### 2. Application

- app ID/version;
- exact application digest;
- publisher key and identity;
- assets/endpoints/checks/docs summary;
- requested capability delta.

When digests match, collapse this layer to `same exact application release`.

### 3. Domain schema

- declared datasets and tables;
- columns, primary keys, foreign keys, indexes and constraints;
- compatible/added/removed/changed objects;
- migration compatibility.

### 4. Domain data

Summary first:

- row counts;
- table/dataset canonical digests;
- added, removed and changed row counts;
- ignored/derived/cache policy.

Details are fetched in bounded pages by dataset/table and stable primary-key
cursor. Field-level values are masked for sensitive datasets until explicit
reveal.

## Canonical row comparison

For each declared table:

1. verify the declared primary key matches actual SQLite schema;
2. reject `WITHOUT ROWID` or unusual collation cases only when the comparator
   cannot establish deterministic ordering; otherwise support them explicitly;
3. order by primary-key values with deterministic SQLite type handling;
4. encode each key and compared column with type tags and lengths;
5. exclude only signed-declared ignored columns;
6. hash rows and table streams using a versioned context;
7. stream-merge the two ordered iterators to classify add/remove/change.

Do not use `SELECT *` order or `rowid` as a portable identity.

## Compare session

A compare session is host-memory state containing:

- opaque random session ID;
- pinned left/right/base identities;
- compatibility classification;
- dataset inventory;
- computed summaries;
- page cursors;
- disclosure state for sensitive values;
- expiry and cancellation token.

Only the trusted shell receives the session handle. Any source change
invalidates it.

## Two-way reconciliation

A two-way comparison can safely apply explicit user-selected operations:

- insert source row into target-derived output;
- update selected target fields to source values;
- delete target row.

It cannot infer whether a missing row represents an intended deletion or an
independent addition. Automatic conflict-free merge is therefore not claimed.

## Three-way reconciliation

Three-way behaviour requires an explicit base snapshot or future trusted
changeset evidence. Lineage identity alone is insufficient if the ancestor
bytes are unavailable.

For each field:

- source == base and target != base → keep target;
- target == base and source != base → take source;
- source == target → take either;
- both differ from base and each other → conflict.

For row existence, apply equivalent add/delete rules. Unresolved conflicts block
execution.

SQLite Session Extension may be evaluated later for host-captured changesets,
but it is not required for the first implementation and cannot reconstruct a
base from two arbitrary final snapshots.

## Reconcile plan

Reconciliation authority is split into two canonical layers. The generic
`lifecycle-plan/1` envelope alone pins exact source, target and optional ancestor
files and owns the create-new destination. It binds one value-free
`reconcile-payload/1` by digest; it never embeds the payload in generic decision
parameters.

The payload includes:

- compare report digest, explicit source/target roles and optional exact
  ancestor evidence;
- selected changes with canonical row/value/write-set digests but no raw keys
  or values;
- explicit conflict resolutions;
- exhaustive expected source/target/output dataset-state digests;
- complete input signature inventories and sensitive confirmations;
- exact target-derived result identity and exactly two ordered lineage parents
  (target-derived-from, changes-applied-from), with any ancestor as bounded
  evidence rather than a third parent;
- payload digest.

The authoritative lifecycle plan binds the expected target-derived output
revision and preserved target application/schema identity. Neither serialized
layer contains selected raw values: the executor looks them up only through its
retained verified input capabilities and rechecks every digest. Durable audit
contains counts and digests only.

## Execution

1. Rebind all sources and plan.
2. Create a private SQLite-consistent copy of target.
3. Begin a transaction on output.
4. Apply changes through generated host SQL with quoted validated identifiers
   and bound values. The caller never provides SQL.
5. Verify immutable column and foreign-key policies.
6. Add revision and multiple-parent lineage.
7. Commit.
8. Run full output verification.
9. Publish to a new final path.

## Stable error codes

```text
incompatible_application
incompatible_schema
missing_primary_key
unsupported_collation
limit_exceeded
session_expired
conflicts_unresolved
stale_plan
immutable_column
verification_failed
```

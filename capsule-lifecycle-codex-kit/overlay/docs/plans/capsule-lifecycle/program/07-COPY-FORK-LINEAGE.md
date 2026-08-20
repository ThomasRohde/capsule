# Copy, fork and lineage semantics

## Operation taxonomy

### Exact duplicate

Purpose: transfer, snapshot or backup-like copy.

- Source is pinned and opened read-only.
- Destination is create-new.
- Use SQLite online backup for a consistent logical snapshot.
- Preserve `capsule_id`, `revision_id`, application digest and domain data.
- Do not append lineage because the logical revision is unchanged.
- Verify the destination before publication.
- The physical file SHA-256 may differ from the source.

### Compact duplicate

Purpose: remove unused pages and deleted-byte remnants.

- Use an equivalent safe compact-copy mechanism such as `VACUUM INTO` on a
  controlled read-only source connection or a host-managed rebuild.
- Preserve logical IDs and data.
- File bytes and page layout may differ.
- Verify logical equivalence and destination validity.
- Do not claim byte-for-byte identity.

### Fork with current data

Purpose: independent experimentation.

- Copy the source into a private output.
- Preserve the signed application compartment.
- Generate a new `capsule_id` and `revision_id`.
- Copy datasets according to signed fork policy and user choice.
- Add a `fork` lineage event with one parent.
- Preserve source file unchanged.
- Require output application digest equality with source.

### Create from template

Purpose: a clean new instance.

Preferred source is a clean, verified application/template release rather than
a working capsule. The result:

- has new capsule and revision IDs;
- uses target release seed/application data;
- omits user-content/history/cache according to the signed contract;
- rebuilds derived datasets;
- records `created-from-template` lineage;
- preserves the template application digest.

### Selective fork

Only available when:

- all selected datasets permit `copy` or `prompt`;
- omitted datasets permit `omit`, `reset` or `rebuild`;
- dependency closure is complete;
- sensitive datasets are explicitly selected;
- resulting application checks pass.

A signed contract may declare dependencies:

```text
diagram-edges depends on diagram-nodes
scenes depends on diagram-nodes and diagram-edges
```

The UI automatically selects required dependencies and explains why.

## Why "without data" is not table deletion

Non-platform tables may contain seed rows, settings, required taxonomies,
derived indexes or user data. The host cannot infer semantics from names or row
counts. A blank instance therefore requires one of:

1. a clean template/application release; or
2. a complete signed data contract whose reset/rebuild semantics are
   executable by the generic host.

When neither exists, only Duplicate and Fork with all current data are offered.

## Lineage model

Every new logical revision records one event.

Suggested fields:

```json
{
  "event_id": "urn:uuid:...",
  "sequence": 4,
  "operation": "fork",
  "result_capsule_id": "urn:uuid:...",
  "result_revision_id": "urn:uuid:...",
  "occurred_at": "2026-08-12T05:00:00Z",
  "application_digest": "<64 lowercase hex>",
  "data_schema_id": "org.sqlite-capsule.diagram-studio-data",
  "data_schema_version": 4,
  "plan_digest": "<64 lowercase hex>",
  "details": {}
}
```

Parents are separate ordered rows:

```json
{
  "event_id": "urn:uuid:...",
  "ordinal": 1,
  "relation": "forked-from",
  "parent_capsule_id": "urn:uuid:...",
  "parent_revision_id": "urn:uuid:...",
  "parent_file_sha256": "<64 lowercase hex>"
}
```

Operations:

```text
created
created-from-template
fork
reconcile
application-upgrade
import
```

`import` is reserved for future cross-application/publisher workflows and is not
implemented by this programme.

## Plan contract

A copy plan binds:

- operation profile/version;
- source path fingerprint and canonical identity;
- file size and SHA-256;
- source capsule/revision IDs;
- source application digest;
- source data schema;
- selected datasets and policies;
- generated output capsule/revision IDs, when applicable;
- selected destination parent and proposed file name;
- validation actions;
- plan digest.

The selected final path is supplied through a host-owned picker handle and is
bound at execution. Raw path strings from the WebView are not trusted.

## Failure codes

```text
unsupported_format
unsupported_operation
incomplete_data_contract
dataset_dependency
sensitive_dataset_confirmation
source_changed
destination_exists
signature_changed
validation_failed
publication_failed
```

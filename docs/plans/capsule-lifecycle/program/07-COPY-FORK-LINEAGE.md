# Copy, fork and lineage semantics

## Operation taxonomy

### Exact duplicate

Purpose: transfer, snapshot or backup-like copy.

- Source is pinned and opened read-only.
- Destination is create-new.
- Copy the exact verified private snapshot byte-for-byte. Do not reopen or back
  up the live source path.
- Preserve `capsule_id`, `revision_id`, application digest and domain data.
- Do not append lineage because the logical revision is unchanged.
- Verify the destination before publication.
- The physical file SHA-256 is identical to the verified snapshot. Any future
  logically equivalent backup/repack profile requires a versioned operation
  and separate validation rules.

### Compact duplicate

Purpose: remove unused pages and deleted-byte remnants.

- Copy the exact retained verified snapshot into owner-private same-filesystem
  staging, then run in-place `VACUUM` there. Do not target the final path or
  reopen the live source.
- Preserve logical IDs and data.
- File bytes and page layout may differ.
- Verify the versioned `compact-logical-state/1` digest, including observable
  implicit rowid, plus exhaustive destination validity before and after
  no-replace publication.
- Require unchanged page size, DELETE journal mode, no sidecars and zero
  freelist pages.
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
a working capsule. A general application signature does not authenticate
ordinary template rows, so the source must also carry the signed template-state
proof defined by ADR 0029 and that proof must match the verified snapshot. Until
that profile is implemented, this operation is unavailable. The result:

- has new capsule and revision IDs;
- uses target release seed/application data;
- copies only template state authenticated by that proof and the signed
  contract;
- resets mutable grants, change log, prior lineage details and sequence state;
- records `created-from-template` lineage;
- preserves the template application digest.

### Selective fork

Only available when:

- all selected datasets permit `copy` or `prompt`;
- omitted datasets permit `omit` or an authenticated `reset`;
- dependency closure is complete;
- sensitive datasets are explicitly selected;
- resulting application checks pass.

The host also verifies that signed dependency declarations cover every actual
cross-dataset foreign key. Omitted state is rebuilt or compacted in private
storage so sensitive values are absent from freelist pages, mutable platform
tables, sequence metadata and sidecars before publication.

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
2. a complete signed reset contract plus an authenticated clean-state source
   whose canonical dataset digests match the signed template-state proof.

When neither exists, only Duplicate and Fork with all current data are offered.

## Lineage model

Every new logical revision records one event.

Suggested fields:

```json
{
  "event_id": "1474a850-0d13-4a96-b6ec-aa633d9bc320",
  "sequence": 4,
  "operation": "fork",
  "result_capsule_id": "40526b2e-867e-4380-a1e4-55b35658471b",
  "result_revision_id": "259a9386-41ad-49eb-88c2-abf0b87d6e51",
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
  "event_id": "1474a850-0d13-4a96-b6ec-aa633d9bc320",
  "ordinal": 1,
  "relation": "forked-from",
  "parent_capsule_id": "3cc3534d-dc8c-4ba4-b455-6a6aa3901ee3",
  "parent_revision_id": "980ea00f-1f80-46b5-90fa-477dc7bb6a8e",
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
invalid_contract
sensitive_confirmation_required
stale_plan
destination_exists
signature_changed
verification_failed
output_publish_failed
```

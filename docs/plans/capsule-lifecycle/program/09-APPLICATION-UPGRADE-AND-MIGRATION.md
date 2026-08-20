# Application upgrade and migration design

## Product definition

Application upgrade gives a working capsule a newer embedded application release
without losing its user-owned identity and data.

The implementation does **not** patch new assets into the old file. It starts
from the clean newer release and migrates the old instance into a new output.

```text
working capsule                    clean target release
old app + user data                new app + seed/schema
        │                                   │
        └──────── plan and verify ──────────┘
                         │
                 copy clean target
                         │
              carry instance/profile
                         │
             copy or migrate datasets
                         │
             add application-upgrade lineage
                         │
          verify target app digest unchanged
                         │
                    publish new copy
```

## Compatibility conditions

Required:

- source and target have the same `app_id`;
- target is a supported format and valid application release;
- source is a supported working capsule or a specifically declared legacy
  import source;
- target publisher continuity is accepted;
- target data contract is complete;
- exactly one data-schema migration path exists, or versions are equal;
- host version supports all required migration operations.

First implementation publisher continuity:

- at least one valid target signature key exactly matches an accepted valid
  source signature key.

Future key delegation is a separate signed extension and not inferred from
matching publisher names or IDs.

## Same-schema upgrade

When `data_schema_id` and version are equal:

- copy target release to private output;
- replace target instance metadata with source capsule ID/profile;
- generate a new revision ID;
- copy datasets according to target `upgrade_policy`;
- retain target seed/application datasets;
- omit/rebuild cache and derived datasets;
- preserve target application digest;
- add upgrade lineage referencing the source working revision and target release
  digest;
- show permission delta and require normal trust review for the new digest.

This is M07 and proves the architecture before adding migration complexity.

## Declarative migration engine

M08 adds migrations between schema versions.

### Principles

- migration definitions are signed target-application metadata;
- source is read-only and never attached to the writable output connection;
- no arbitrary SQL, JavaScript, WASM, shell commands or dynamic libraries;
- operations work on typed values and declared tables;
- every step has row/byte/time limits;
- the engine writes only declared target domain datasets and mutable instance
  tables;
- application platform tables and assets remain immutable.

### Migration graph

Each edge:

```json
{
  "migration_id": "diagram-data-3-to-4",
  "data_schema_id": "org.sqlite-capsule.diagram-studio-data",
  "from_version": 3,
  "to_version": 4,
  "description": "Add stable style identifiers.",
  "reversible": false
}
```

Planning rejects:

- cycles;
- duplicate edges;
- ambiguous paths;
- missing intermediate versions;
- migrations for another schema ID;
- unsupported operation versions.

### Migration DSL v1

The initial allowlist contains only:

- `copy_rows` — copy explicitly mapped source columns or typed literals into a
  validated target table;
- `copy_dataset` — copy one signed dataset only when source and target table
  contracts are structurally compatible; and
- `discard_dataset` — explicitly retain no source rows for a dataset whose
  signed target policy permits omission.

Preconditions and postconditions use the separate fixed assertion allowlist.
Literal and mapping values use exact SQLite storage-class wrappers, including
decimal i64 text, finite IEEE-754 bits, UTF-8 text and bounded base64 BLOBs.
There is no implicit JSON-to-SQLite coercion.

`rebuild_dataset` and `rebuild_endpoint` are not operations. A target dataset
whose upgrade policy is `rebuild` keeps the clean target release's declared
state without invoking application code. No loop, expression language,
regular-expression engine, endpoint, SQL, script, extension, attachment or
user-defined function is admitted by v1.

## Legacy v0.2 source — explicitly deferred

A signed v0.2 working capsule is never rewritten. V0.2-to-v0.3 upgrade is
unavailable in the currently frozen v0.3/migration-v1 contracts. No planner may
infer v0.2 datasets or treat generic authoring conversion as an upgrade.

Before M08 can implement or advertise this route, a separate accepted ADR and
versioned signed legacy-adapter contract must add at least:

- expected v0.2 app ID;
- source schema digest or exact table/column declarations;
- source data schema alias/version;
- migration path into v0.3.

The future host will read legacy domain rows from the exact plan-bound private
snapshot and create a new v0.3 output. Source signature remains historical
evidence; target signature authenticates the new application. Until that
contract exists, the safe result is `unsupported_operation`.

## Capability/trust behaviour

An application upgrade changes the exact application digest. Even with the same
publisher, the output is not automatically runnable unless current host policy
explicitly grants that exact release and its capability set.

The Overview shows:

- old/new version and digest;
- added/removed/changed requested capabilities;
- publisher continuity evidence;
- data migration path;
- datasets copied/reset/rebuilt/omitted;
- unresolved warnings.

## Rollback

Rollback is the original source capsule, which remains unchanged. No reverse
migration is required for the initial feature. The output lineage and result
report include the source path fingerprint/digest, but paths are not embedded in
the portable capsule.

## Error codes

```text
incompatible_application
publisher_mismatch
invalid_contract
incompatible_schema
migration_path_missing
migration_path_ambiguous
migration_operation_unsupported
migration_assertion_failed
limit_exceeded
signature_changed
capability_review_required
```

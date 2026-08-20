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

Allowlisted operations:

#### `copy_rows`

Copy source rows to a target table using a column map.

Sources for a target column:

- source column;
- literal JSON scalar;
- target default;
- generated host UUID;
- exact operation timestamp;
- allowlisted transform over source values.

#### `map_values`

Map an enumerated source scalar through an exact JSON mapping. Unknown values
use an explicit `error`, `keep`, `null` or literal policy.

#### `assert_source`

Verify source table/columns, key uniqueness, row count bounds or a canonical
schema digest before copying.

#### `assert_target`

Verify target row counts, non-null conditions and dataset digests after previous
steps.

#### `rebuild_dataset`

Mark a target dataset with `derived` role for application-defined deterministic
rebuild through a separately host-supported profile. In v1 this may simply
clear/retain target release data; it must not execute application code.

Allowlisted value transforms should remain small:

```text
identity
coalesce
to_text
to_integer_exact
to_real_finite
to_boolean
json_pointer
lower_ascii
upper_ascii
```

No loops, regular-expression engines, arithmetic expression language or
user-defined functions in v1.

## Legacy v0.2 source

A signed v0.2 working capsule is never rewritten. A v0.3 target release may
declare a signed legacy source profile containing:

- expected v0.2 app ID;
- source schema digest or exact table/column declarations;
- source data schema alias/version;
- migration path into v0.3.

The host reads legacy domain rows through its own read-only connections and
creates a new v0.3 output. Source signature remains historical evidence; target
signature authenticates the new application.

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
different_application
publisher_discontinuity
target_not_clean_release
data_schema_mismatch
migration_path_missing
migration_path_ambiguous
migration_operation_unsupported
migration_source_mismatch
migration_value_error
migration_limit
target_application_changed
capability_review_required
```

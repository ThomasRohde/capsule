# Draft lifecycle contracts

These files make the programme concrete enough for implementation and review. They
are **draft inputs**, not permission to overwrite the live repository format.

M00 reconciles them with the current source; M01 freezes versioned format and
signature contracts.

## Files

| File | Purpose |
| --- | --- |
| `capsule-v0.3-draft.sql` | Proposed base SQLite schema and platform tables |
| `capsule-signed-app-v0.3-draft.sql` | Publisher/signature extension |
| `capsule-lifecycle-v0.3.conformance.draft.json` | Proposed structural/signature compartments |
| `capsule-application-profile-v0.3.schema.json` | Signed application display projection |
| `capsule-instance-profile-v0.3.schema.json` | Mutable user-owned instance projection |
| `capsule-data-contract-v0.3.schema.json` | Signed dataset lifecycle policies |
| `capsule-lineage-v0.3.schema.json` | Mutable provenance projection |
| `lifecycle-plan-v1.schema.json` | Generic immutable plan envelope |
| `compare-report-v1.schema.json` | Bounded comparison result |
| `reconcile-plan-v1.schema.json` | Apply-to-copy selections/conflicts |
| `upgrade-plan-v1.schema.json` | Application rebase plan |
| `capsule-migration-v0.3.schema.json` | Restricted declarative migration |
| `lifecycle-error-codes-v1.json` | Stable host/CLI/Tauri error catalogue |

## Contract rules

- Profile/version names are part of compatibility.
- Signed and mutable compartments must be exhaustive and disjoint.
- JSON schemas describe serialised API/projection formats; SQLite tables remain
  the canonical capsule source.
- Canonical byte/digest rules require cross-language vectors before stabilisation.
- No migration field accepts arbitrary SQL or executable code.
- Unknown fields/operations fail closed unless a later versioned profile says
  otherwise.

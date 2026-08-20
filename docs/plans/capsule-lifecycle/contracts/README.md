# Draft lifecycle contracts

These files make the programme concrete enough for implementation and review. They
are **draft inputs**, not permission to overwrite the live repository format.

`x-maxUtf8Bytes` is a normative extension used alongside JSON Schema
`maxLength`: standard validators enforce code-point length, while hosts and
lifecycle validators must additionally enforce the stated UTF-8 byte limit.

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
| `compare-report-v1.schema.json` | Deterministic bounded counts/digests comparison summary |
| `compare-page-v1.schema.json` | Opaque-session-bound bounded row/field detail page |
| `compare-application-v1.schema.json` | Fixed-family, value-free application-compartment detail |
| `tauri-compare-v1.schema.json` | Trusted-shell opaque compare candidate/session projections |
| `reconcile-plan-v1.schema.json` | Value-free reconcile payload bound by a generic lifecycle plan |
| `tauri-reconcile-v1.schema.json` | Strict token-only trusted-shell reconcile requests, two-/three-way review, progress and status projections |
| `cli-reconcile-v1.schema.json` | Bounded value-free CLI reconciliation candidate, conflict, review and verified-result projections |
| `upgrade-plan-v1.schema.json` | Path/value-free same-schema application-upgrade review bound to a generic lifecycle plan |
| `capsule-migration-v0.3.schema.json` | Restricted declarative migration |
| `copy-preview-v1.schema.json` | Non-authoritative bounded copy review projection |
| `exact-copy-preview-v1.schema.json` | Path-free byte-exact duplicate review projection |
| `compact-copy-preview-v1.schema.json` | Non-authoritative v0.2/v0.3 compact-copy review projection |
| `semantic-copy-preview-v1.schema.json` | Signed v0.3 fork/template/selective operation review |
| `tauri-copy-v1.schema.json` | Trusted-shell copy choice/review/status/progress projections |
| `template-state-v1.schema.json` | Signed authenticated clean-template state proof |
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
- Reconciliation uses two layers. `lifecycle-plan/1` alone pins exact
  source/target/optional-ancestor inputs and owns the create-new destination;
  it binds exactly one canonical `reconcile-payload/1` digest. The payload has
  no paths, raw keys, raw values, SQL or output capability and cannot be
  executed independently.
- Same-schema upgrade uses the same separation: the serialised
  `upgrade-plan/1` is non-authoritative review evidence, while retained verified
  inputs, accepted publisher key and destination capability remain host-owned.

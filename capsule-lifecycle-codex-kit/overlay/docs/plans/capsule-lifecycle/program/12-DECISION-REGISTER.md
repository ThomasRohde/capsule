# Decision register

M00 must create repository ADRs for the accepted form of these decisions. Use
the next available ADR numbers in the live checkout.

## D-01: Application/instance split

Default: accepted.

The v0.3 signed application compartment excludes capsule instance identity,
profile, lineage and domain rows.

## D-02: New format/signature profile

Default: accepted.

Use format v0.3 and signed-app v0.3 rather than changing v0.2 semantics.

## D-03: Workspace service boundary

Default: accepted.

Add product-independent `capsule-workspace`. Keep `capsule-lifecycle` focused on
file identity, writer coordination, recovery and restore.

## D-04: Output-only transformations

Default: accepted.

All copy/fork/reconcile/upgrade operations use read-only inputs and create-new
outputs. No in-place merge or upgrade.

## D-05: Template-based blank creation

Default: accepted.

"Without data" means create from a clean application/template release or a
complete signed reset contract. Never infer by deleting non-platform tables.

## D-06: Safe icon profile

Default: PNG/WebP only, 512 KiB compressed, 1024×1024 decoded maximum. No SVG in
the first release.

## D-07: Compare identity

Default: declared primary keys and deterministic typed encoding. No `rowid`
identity.

## D-08: Reconcile semantics

Default: explicit two-way selection; automatic three-way only with an explicit
base. No claim of automatic merge from two snapshots.

## D-09: Upgrade construction

Default: copy the clean target release and migrate user state into it. Never
patch application assets into the old working capsule.

## D-10: Migration language

Default: restricted declarative typed-value operations. No arbitrary SQL or
executable migration code.

## D-11: Publisher continuity

Default: same accepted signing key for first implementation. Key rotation needs
a separately specified signed delegation extension.

## D-12: Cabinet cache

Default: host-local, rebuildable and separate from canonical capsule metadata
and trust decisions.

## Open implementation choices for M00

These may be adapted after code inspection:

- exact names/columns of v0.3 platform tables;
- whether canonical typed-row hashing is factored from `capsule-crypto` into a
  shared crate/module;
- exact Rust CLI command names;
- whether Cabinet persistence is a new crate or desktop-private module;
- temporary-file publication helper reuse;
- feature-flag mechanics during rollout.

Changes to defaults require a clear security/compatibility reason, ADR and plan
updates.

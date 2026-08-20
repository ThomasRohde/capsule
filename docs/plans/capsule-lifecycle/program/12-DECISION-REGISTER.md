# Decision register

M00 reconciled the register against the live checkout and accepted repository
ADRs 0021-0028. The mappings below are the durable decision sources.

| Register decisions | Accepted ADR |
| --- | --- |
| D-01, D-02 | [ADR 0021](../../../decisions/0021-format-v0.3-and-instance-identity.md) |
| signed v0.3 canonical stream | [ADR 0022](../../../decisions/0022-signed-application-v0.3-compartment.md) |
| D-03 | [ADR 0023](../../../decisions/0023-capsule-workspace-boundary.md) |
| D-04 | [ADR 0024](../../../decisions/0024-lifecycle-plan-execute-publication.md) |
| D-05, D-09, D-10, D-11 | [ADR 0025](../../../decisions/0025-data-contracts-and-restricted-migrations.md) |
| D-06, D-12 | [ADR 0026](../../../decisions/0026-host-owned-cabinet-and-safe-metadata.md) |
| D-07, D-08 | [ADR 0027](../../../decisions/0027-compare-and-reconcile-identity.md) |
| verification phases and size policy | [ADR 0028](../../../decisions/0028-verification-phases-and-size-policy.md) |
| copy operation profiles and template proof | [ADR 0029](../../../decisions/0029-copy-operation-profiles-and-template-proof.md) |

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

## Resolved implementation assignments

M00 leaves no architecture choice in this list open:

- v0.3 platform names and columns are frozen as M01 input by the draft SQL and
  ADRs 0021/0022; a semantic change requires a follow-up ADR and synchronized
  contract update;
- M01 may factor canonical typed-row hashing into a shared product-independent
  module only if v0.2 vectors remain byte-identical; this is an internal code
  placement choice, not a format choice;
- product-independent CLI operation/profile JSON is normative; M02 owns exact
  command spelling and may adapt the illustrative names in the target
  architecture without changing the contracts;
- ADR 0026 requires a separate protected, rebuildable Cabinet store; M03 owns
  whether its implementation is a desktop-private module or dedicated crate;
- M02 must extract or reuse the proven no-clobber publication mechanism under
  ADR 0024, never the authoring replacement path;
- rollout flags are host-owned compile-time or protected-local configuration as
  fixed by the rollout plan, never capsule-controlled.

None of these assignments can weaken a security/compatibility rule without a
new ADR and programme update.

# Diagram Studio lifecycle examples

These reviewable JSON examples turn the generic contracts into one visual
application scenario without placing Diagram Studio semantics in generic code.

They cover:

- signed application display metadata;
- mutable instance title/description/tags;
- dataset roles and fork/compare/reconcile/upgrade policies;
- lineage across template creation and application upgrade;
- same-schema upgrade planning;
- a restricted v1→v2 data migration.
- exact, compact and semantic copy reviews plus a main-shell progress event.
- two-layer reconciliation: a value-free payload and the lifecycle envelope
  that alone pins inputs and owns the create-new destination.
- strict trusted-shell reconciliation: opaque two-way selection tokens and
  optional verified-ancestor conflict/resolution tokens, with no paths, raw
  row keys, values or SQL in browser requests.

Hashes in illustrative plans/lineage are synthetic. M01-M09 must generate
deterministic SQLite fixtures and real digests from repository builders.
